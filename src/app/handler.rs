use std::fmt::Debug;
use std::pin::Pin;
use std::sync::Arc;

use coap_lite::{BlockHandler, BlockHandlerConfig, CoapRequest, MessageClass, Packet};
use futures::Stream;
use log::{debug, warn};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::app::builder::AppBuilder;
use crate::app::core_handler::CoreRequestHandler;
use crate::app::error::CoapError;
use crate::app::path_matcher::{MatchedResult, PathMatcher};
use crate::app::Request;
use crate::app::request_type_key::RequestTypeKey;
use crate::app::resource_builder::ResourceHandler;
use crate::packet_handler::PacketHandler;

const DEFAULT_DISCOVERABLE: bool = true;
const DEFAULT_BLOCK_TRANSFER: bool = true;

pub struct AppHandler<Endpoint: Ord + Clone> {
    block_handler: Option<Arc<Mutex<BlockHandler<Endpoint>>>>,
    resources_by_path: Arc<PathMatcher<ResourceHandler<Endpoint>>>,
}

impl<Endpoint: Ord + Clone> Clone for AppHandler<Endpoint> {
    fn clone(&self) -> Self {
        Self {
            block_handler: self.block_handler.clone(),
            resources_by_path: self.resources_by_path.clone(),
        }
    }
}

impl<Endpoint: Debug + Ord + Send + Clone + 'static> PacketHandler<Endpoint>
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

impl<Endpoint: Debug + Ord + Clone + Send + 'static> AppHandler<Endpoint> {
    pub fn from_builder(mut builder: AppBuilder<Endpoint>, mtu: Option<u32>) -> Self {
        let discoverable = builder.config.discoverable.unwrap_or(DEFAULT_DISCOVERABLE);
        if discoverable {
            CoreRequestHandler::install(&mut builder);
        }

        let resources_by_path = Arc::new(PathMatcher::from_path_strings(builder.resources_by_path));

        let block_transfer = builder
            .config
            .block_transfer
            .unwrap_or(DEFAULT_BLOCK_TRANSFER);
        let block_handler = if block_transfer {
            let mut block_config = BlockHandlerConfig::default();
            if let Some(mtu) = mtu {
                if let Ok(mtu) = usize::try_from(mtu) {
                    block_config.max_total_message_size = mtu;
                }
            }
            Some(Arc::new(Mutex::new(BlockHandler::new(block_config))))
        } else {
            None
        };

        Self {
            block_handler,
            resources_by_path,
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
                debug!("Empty message from {peer:?} not handled: not implemented!");
            }
            n => {
                warn!("Unhandled message code {n} from {peer:?}, ignoring...");
            }
        }
    }

    async fn handle_get(&self, tx: UnboundedSender<Packet>, packet: Packet, peer: Endpoint) {
        let mut request = CoapRequest::from_packet(packet, peer);
        if let Err(e) = self.try_handle_get(&tx, &mut request).await {
            if request.apply_from_error(e.into_handling_error()) {
                // If the error happens to need block2 handling, let's do that here...
                if let Some(ref block_handler) = self.block_handler {
                    let _ = block_handler.lock().await.intercept_response(&mut request);
                }
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

        let resource = self.resources_by_path.lookup(&paths);
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
                let unmatched_path = Vec::from(&paths[matched_index..]);

                let actual_handler = value
                    .handlers
                    .get(&RequestTypeKey::from(*request.get_method()));

                match actual_handler {
                    Some(handler) => {
                        if !self.maybe_handle_block_request(request).await? {
                            let original = request.clone();
                            let response = handler
                                .handle(Request {
                                    unmatched_path,
                                    original,
                                })
                                .await?;
                            request.response = Some(response);
                            let _ = self.maybe_handle_block_response(request).await?;
                        }
                        if let Some(ref response) = request.response {
                            // TODO: We can avoid this clone by refactoring this a bit internally
                            tx.send(response.message.clone()).unwrap();
                        }
                        Ok(())
                    }
                    None => Err(CoapError::method_not_allowed()),
                }
            }
            None => Err(CoapError::not_found()),
        }
    }

    async fn maybe_handle_block_request(
        &self,
        request: &mut CoapRequest<Endpoint>,
    ) -> Result<bool, CoapError> {
        if let Some(ref block_handler) = self.block_handler {
            if block_handler.lock().await.intercept_request(request)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn maybe_handle_block_response(
        &self,
        request: &mut CoapRequest<Endpoint>,
    ) -> Result<bool, CoapError> {
        if let Some(ref block_handler) = self.block_handler {
            Ok(block_handler.lock().await.intercept_response(request)?)
        } else {
            Ok(false)
        }
    }
}
