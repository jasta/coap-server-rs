use std::collections::HashMap;
use std::sync::Arc;

use coap_lite::{BlockHandler, CoapRequest, Packet};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

use crate::app::observe::ObservableResource;
use crate::app::request_handler::RequestHandler;
use crate::app::request_type_key::RequestTypeKey;
use crate::app::{CoapError, Request};

pub(crate) struct ResourceHandler<Endpoint> {
    pub handlers: HashMap<RequestTypeKey, Box<dyn RequestHandler<Endpoint> + Send + Sync>>,
    pub observable: Option<Box<dyn ObservableResource + Send + Sync>>,
    pub disable_block_transfer: bool,
}

impl<Endpoint> Clone for ResourceHandler<Endpoint> {
    fn clone(&self) -> Self {
        let handlers: HashMap<_, _> = self
            .handlers
            .iter()
            .map(|(req_type, handler)| (*req_type, dyn_clone::clone_box(handler.as_ref())))
            .collect();
        let observable = self
            .observable
            .as_ref()
            .map(|x| dyn_clone::clone_box(x.as_ref()));
        Self {
            handlers,
            observable,
            disable_block_transfer: self.disable_block_transfer,
        }
    }
}

impl<Endpoint: Ord + Clone + 'static> ResourceHandler<Endpoint> {
    pub async fn handle(
        &self,
        tx: &UnboundedSender<Packet>,
        wrapped_request: Request<Endpoint>,
        block_handler_arc: Option<Arc<Mutex<BlockHandler<Endpoint>>>>,
    ) -> Result<(), CoapError> {
        // TODO: This method is very clone happy and it really doesn't need to be.  Cloning
        // these parts are especially expensive as it contains the request/response payloads.
        let mut request = wrapped_request.original.clone();
        let method_handler = self
            .handlers
            .get(&RequestTypeKey::from(*request.get_method()));

        match method_handler {
            Some(handler) => {
                let block_handler = if self.disable_block_transfer {
                    None
                } else {
                    block_handler_arc.as_ref()
                };
                if !self
                    .maybe_handle_block_request(&mut request, block_handler)
                    .await?
                {
                    let response = handler.handle(wrapped_request).await?;
                    request.response = Some(response);
                    let _ = self
                        .maybe_handle_block_response(&mut request, block_handler)
                        .await?;
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

    async fn maybe_handle_block_request(
        &self,
        request: &mut CoapRequest<Endpoint>,
        block_handler: Option<&Arc<Mutex<BlockHandler<Endpoint>>>>,
    ) -> Result<bool, CoapError> {
        if let Some(block_handler) = block_handler {
            if block_handler.lock().await.intercept_request(request)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn maybe_handle_block_response(
        &self,
        request: &mut CoapRequest<Endpoint>,
        block_handler: Option<&Arc<Mutex<BlockHandler<Endpoint>>>>,
    ) -> Result<bool, CoapError> {
        if let Some(block_handler) = block_handler {
            Ok(block_handler.lock().await.intercept_response(request)?)
        } else {
            Ok(false)
        }
    }
}
