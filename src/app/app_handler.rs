use alloc::boxed::Box;
use alloc::fmt::Debug;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::pin::Pin;
use core::{hash::Hash, task::Poll};
use hashbrown::HashMap;
use pin_project::pin_project;
use rand::Rng;

use coap_lite::{BlockHandler, CoapRequest, MessageClass, MessageType, Packet};
#[cfg(feature = "embassy")]
use embassy_util::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use futures::{Future, Stream};
use log::{debug, warn};
#[cfg(feature = "tokio")]
use tokio::sync::{mpsc::UnboundedSender, Mutex};

use crate::app::app_builder::AppBuilder;
use crate::app::block_handler_util::new_block_handler;
use crate::app::coap_utils::new_pong_message;
use crate::app::core_handler::CoreRequestHandler;
use crate::app::error::CoapError;
use crate::app::path_matcher::{MatchedResult, PathMatcher};
use crate::app::resource_builder::BuildParameters;
use crate::app::resource_handler::ResourceHandler;
use crate::app::retransmission_manager::{RetransmissionManager, TransmissionParameters};
use crate::app::Request;
use crate::packet_handler::PacketHandler;

const DEFAULT_DISCOVERABLE: bool = true;
pub(crate) const DEFAULT_BLOCK_TRANSFER: bool = true;

/// Main PacketHandler for an application suite of handlers.  Efficiency and concurrency are
/// the primary goals of this implementation, but with the need to balance developer friendliness
/// of the main API.
pub struct AppHandler<Endpoint: Debug + Clone + Ord + Eq + Hash, R: Clone> {
    #[cfg(feature = "tokio")]
    retransmission_manager: Arc<Mutex<RetransmissionManager<Endpoint>>>,
    #[cfg(feature = "embassy")]
    retransmission_manager: Arc<Mutex<CriticalSectionRawMutex, RetransmissionManager<Endpoint>>>,

    /// Special internal [`coap_lite::BlockHandler`] that we use only for formatting errors
    /// that might be larger than MTU.
    #[cfg(feature = "tokio")]
    error_block_handler: Arc<Mutex<BlockHandler<Endpoint>>>,
    #[cfg(feature = "embassy")]
    error_block_handler: Arc<Mutex<CriticalSectionRawMutex, BlockHandler<Endpoint>>>,

    /// Full set of handlers registered for this app, grouped by path but searchable using inexact
    /// matching.  See [`PathMatcher`] for more.
    handlers_by_path: Arc<PathMatcher<ResourceHandler<Endpoint>>>,
    rng: R,
}

impl<Endpoint: Debug + Clone + Ord + Eq + Hash, R: Clone> Clone for AppHandler<Endpoint, R> {
    fn clone(&self) -> Self {
        Self {
            retransmission_manager: self.retransmission_manager.clone(),
            error_block_handler: self.error_block_handler.clone(),
            handlers_by_path: self.handlers_by_path.clone(),
            rng: self.rng.clone(),
        }
    }
}

#[pin_project]
struct PacketStream<F: Send> {
    #[pin]
    fut: F,
    response: Vec<Packet>,
    fut_complete: bool,
}

impl<F: Future<Output = Vec<Packet>> + Send> Stream for PacketStream<F> {
    type Item = Packet;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        let this = self.project();

        if !*this.fut_complete {
            match this.fut.poll(cx) {
                Poll::Ready(response) => {
                    *this.response = response;
                    *this.fut_complete = true;
                    // FIXME: should use some more efficient container
                    this.response.reverse();
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }

        Poll::Ready(this.response.pop())
    }
}

impl<Endpoint: Debug + Clone + Ord + Eq + Hash + Send + 'static, R: Rng + Send + Sync + Clone>
    PacketHandler<Endpoint> for AppHandler<Endpoint, R>
{
    fn handle<'a>(
        &'a self,
        packet: Packet,
        peer: Endpoint,
    ) -> Pin<Box<dyn Stream<Item = Packet> + Send + 'a>> {
        let handler = self.handle_packet(packet, peer);
        Box::pin(PacketStream {
            fut: handler,
            response: vec![],
            fut_complete: false,
        })
    }
}

impl<Endpoint: Debug + Clone + Ord + Eq + Hash + Send + 'static, R: Rng + Clone>
    AppHandler<Endpoint, R>
{
    pub fn from_builder(builder: AppBuilder<Endpoint>, mtu: Option<u32>, mut rng: R) -> Self {
        let retransmission_manager = Arc::new(Mutex::new(RetransmissionManager::new(
            TransmissionParameters::default(),
            &mut rng,
        )));

        let build_params = BuildParameters {
            mtu,
            retransmission_manager: retransmission_manager.clone(),
        };

        let error_block_handler = Arc::new(Mutex::new(new_block_handler(mtu)));

        let mut discoverable_resources = Vec::new();
        let mut handlers = HashMap::new();
        for resource_builder in builder.resources {
            let resource = resource_builder.build(build_params.clone());
            if let Some(discoverable) = resource.discoverable {
                discoverable_resources.push(discoverable);
            }
            handlers.insert(resource.path, resource.handler);
        }

        let discoverable = builder.config.discoverable.unwrap_or(DEFAULT_DISCOVERABLE);
        if discoverable {
            let core_resource = CoreRequestHandler::new_resource_builder(discoverable_resources)
                .build(build_params.clone());
            handlers.insert(core_resource.path, core_resource.handler);
        }

        let handlers_by_path = Arc::new(PathMatcher::from_path_strings(handlers));

        Self {
            retransmission_manager,
            error_block_handler,
            handlers_by_path,
            rng,
        }
    }

    async fn handle_packet(&self, packet: Packet, peer: Endpoint) -> Vec<Packet> {
        match packet.header.code {
            MessageClass::Request(_) => {
                let mut packets = vec![];
                self.handle_get(&mut packets, packet, peer).await;
                packets
            }
            MessageClass::Response(_) => {
                warn!("Spurious response message from {peer:?}, ignoring...");
                vec![]
            }
            MessageClass::Empty => {
                match packet.header.get_type() {
                    t @ (MessageType::Acknowledgement | MessageType::Reset) => {
                        let mut retransmission_manager = self.retransmission_manager.lock().await;
                        if let Err(packet) =
                            retransmission_manager.maybe_handle_reply(packet, &peer)
                        {
                            let message_id = packet.header.message_id;
                            debug!(
                                "Got {t:?} from {peer:?} for unrecognized message ID {message_id}"
                            );
                        }

                        vec![]
                    }
                    MessageType::Confirmable => {
                        // A common way in CoAP to trigger a cheap "ping" to make sure
                        // the server is alive.
                        vec![new_pong_message(&packet)]
                    }
                    MessageType::NonConfirmable => {
                        debug!("Ignoring Non-Confirmable Empty message from {peer:?}");
                        vec![]
                    }
                }
            }
            code => {
                warn!("Unhandled message code {code} from {peer:?}, ignoring...");
                vec![]
            }
        }
    }

    async fn handle_get(&self, out: &mut Vec<Packet>, packet: Packet, peer: Endpoint) {
        let mut request = CoapRequest::from_packet(packet, peer);
        if let Err(e) = self.try_handle_get(out, &mut request).await {
            if request.apply_from_error(e.into_handling_error()) {
                // If the error happens to need block2 handling, let's do that here...
                let _ = self
                    .error_block_handler
                    .lock()
                    .await
                    .intercept_response(&mut request);
                out.push(request.response.unwrap().message);
            }
        }
    }

    async fn try_handle_get(
        &self,
        out: &mut Vec<Packet>,
        request: &mut CoapRequest<Endpoint>,
    ) -> Result<(), CoapError> {
        let paths = request.get_path_as_vec().map_err(CoapError::bad_request)?;

        let resource = self.handlers_by_path.lookup(&paths);
        if log::log_enabled!(log::Level::Debug) {
            let peer = &request.source;
            let method = request.get_method();
            let path = request.get_path();
            let handler_label = resource
                .as_ref()
                .map_or_else(|| ": <no resource>!", |_| ": matched resource...");
            debug!("Received from [{peer:?}]: {method:?} /{path}{handler_label}");
        }

        match resource {
            Some(MatchedResult {
                matched_index,
                value,
            }) => {
                let wrapped_request = Request {
                    original: request.clone(),
                    unmatched_path: Vec::from(&paths[matched_index..]),
                };

                let mut rng = self.rng.clone();
                value.handle(out, wrapped_request, &mut rng).await
            }
            None => Err(CoapError::not_found()),
        }
    }
}
