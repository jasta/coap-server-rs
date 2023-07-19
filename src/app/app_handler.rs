use hashbrown::HashMap;
use core::fmt::Debug;
use core::hash::Hash;
use core::pin::Pin;
use alloc::sync::Arc;
use alloc::boxed::Box;
use alloc::vec::Vec;

use coap_lite::{BlockHandler, CoapRequest, MessageClass, MessageType, Packet};
use futures::Stream;
use log::{debug, warn};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use tokio_stream::wrappers::UnboundedReceiverStream;

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
pub struct AppHandler<Endpoint: Debug + Clone + Ord + Eq + Hash> {
    retransmission_manager: Arc<Mutex<RetransmissionManager<Endpoint>>>,

    /// Special internal [`coap_lite::BlockHandler`] that we use only for formatting errors
    /// that might be larger than MTU.
    error_block_handler: Arc<Mutex<BlockHandler<Endpoint>>>,

    /// Full set of handlers registered for this app, grouped by path but searchable using inexact
    /// matching.  See [`PathMatcher`] for more.
    handlers_by_path: Arc<PathMatcher<ResourceHandler<Endpoint>>>,
}

impl<Endpoint: Debug + Clone + Ord + Eq + Hash> Clone for AppHandler<Endpoint> {
    fn clone(&self) -> Self {
        Self {
            retransmission_manager: self.retransmission_manager.clone(),
            error_block_handler: self.error_block_handler.clone(),
            handlers_by_path: self.handlers_by_path.clone(),
        }
    }
}

impl<Endpoint: Debug + Clone + Ord + Eq + Hash + Send + 'static> PacketHandler<Endpoint>
    for AppHandler<Endpoint>
{
    fn handle<'a>(
        &'a self,
        packet: Packet,
        peer: Endpoint,
    ) -> Pin<Box<dyn Stream<Item = Packet> + Send + 'a>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // TODO: This spawn is technically unnecessary as we could implement a Stream ourselves
        // similar to how async-stream crate does it, but the boiler plate doesn't really seem
        // worth it for now.
        tokio::spawn({
            let cloned_self = self.clone();
            async move {
                cloned_self.handle_packet(tx, packet, peer).await;
            }
        });
        Box::pin(UnboundedReceiverStream::new(rx))
    }
}

impl<Endpoint: Debug + Clone + Ord + Eq + Hash + Send + 'static> AppHandler<Endpoint> {
    pub fn from_builder(builder: AppBuilder<Endpoint>, mtu: Option<u32>) -> Self {
        let retransmission_manager = Arc::new(Mutex::new(RetransmissionManager::new(
            TransmissionParameters::default(),
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
        }
    }

    async fn handle_packet(&self, tx: UnboundedSender<Packet>, packet: Packet, peer: Endpoint) {
        match packet.header.code {
            MessageClass::Request(_) => {
                self.handle_get(tx, packet, peer).await;
            }
            MessageClass::Response(_) => {
                warn!("Spurious response message from {peer:?}, ignoring...");
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
                    }
                    MessageType::Confirmable => {
                        // A common way in CoAP to trigger a cheap "ping" to make sure
                        // the server is alive.
                        tx.send(new_pong_message(&packet)).unwrap();
                    }
                    MessageType::NonConfirmable => {
                        debug!("Ignoring Non-Confirmable Empty message from {peer:?}");
                    }
                }
            }
            code => {
                warn!("Unhandled message code {code} from {peer:?}, ignoring...");
            }
        }
    }

    async fn handle_get(&self, tx: UnboundedSender<Packet>, packet: Packet, peer: Endpoint) {
        let mut request = CoapRequest::from_packet(packet, peer);
        if let Err(e) = self.try_handle_get(&tx, &mut request).await {
            if request.apply_from_error(e.into_handling_error()) {
                // If the error happens to need block2 handling, let's do that here...
                let _ = self
                    .error_block_handler
                    .lock()
                    .await
                    .intercept_response(&mut request);
                tx.send(request.response.unwrap().message).unwrap();
            }
        }
    }

    async fn try_handle_get(
        &self,
        tx: &UnboundedSender<Packet>,
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

                value.handle(tx, wrapped_request).await
            }
            None => Err(CoapError::not_found()),
        }
    }
}
