use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Debug;
use core::hash::Hash;
use hashbrown::HashMap;
use rand::Rng;

use coap_lite::{BlockHandler, CoapRequest, Packet};
#[cfg(feature = "embassy")]
use embassy_util::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
#[cfg(feature = "tokio")]
use tokio::sync::{mpsc::UnboundedSender, Mutex};

#[cfg(feature = "observable")]
use crate::app::observe_handler::{ObserveHandler, RegistrationEvent};
use crate::app::request_handler::RequestHandler;
use crate::app::request_type_key::RequestTypeKey;
use crate::app::retransmission_manager::RetransmissionManager;
use crate::app::{CoapError, Request};

pub struct ResourceHandler<Endpoint: Debug + Clone + Ord + Eq + Hash> {
    pub handlers: HashMap<RequestTypeKey, Box<dyn RequestHandler<Endpoint> + Send + Sync>>,
    #[cfg(all(feature = "tokio", feature = "observable"))]
    pub observe_handler: Option<Arc<Mutex<ObserveHandler<Endpoint>>>>,
    #[cfg(feature = "tokio")]
    pub block_handler: Option<Arc<Mutex<BlockHandler<Endpoint>>>>,
    #[cfg(feature = "tokio")]
    pub retransmission_manager: Arc<Mutex<RetransmissionManager<Endpoint>>>,
    #[cfg(all(feature = "embassy", feature = "observable"))]
    pub observe_handler: Option<Arc<Mutex<CriticalSectionRawMutex, ObserveHandler<Endpoint>>>>,
    #[cfg(feature = "embassy")]
    pub block_handler: Option<Arc<Mutex<CriticalSectionRawMutex, BlockHandler<Endpoint>>>>,
    #[cfg(feature = "embassy")]
    pub retransmission_manager:
        Arc<Mutex<CriticalSectionRawMutex, RetransmissionManager<Endpoint>>>,
}

impl<Endpoint: Debug + Clone + Ord + Eq + Hash> Clone for ResourceHandler<Endpoint> {
    fn clone(&self) -> Self {
        let handlers: HashMap<_, _> = self
            .handlers
            .iter()
            .map(|(req_type, handler)| (*req_type, dyn_clone::clone_box(handler.as_ref())))
            .collect();
        Self {
            handlers,
            #[cfg(feature = "observable")]
            observe_handler: self.observe_handler.clone(),
            block_handler: self.block_handler.clone(),
            retransmission_manager: self.retransmission_manager.clone(),
        }
    }
}

impl<Endpoint: Debug + Clone + Eq + Hash + Ord + Send + 'static> ResourceHandler<Endpoint> {
    pub async fn handle<R: Rng>(
        &self,
        out: &mut Vec<Packet>,
        wrapped_request: Request<Endpoint>,
        rng: &mut R,
    ) -> Result<(), CoapError> {
        let method = *wrapped_request.original.get_method();
        let method_handler = self
            .handlers
            .get(&RequestTypeKey::from(method))
            .or_else(|| self.handlers.get(&RequestTypeKey::new_match_all()));

        // Loop here so we can "park" to wait for notify_change calls from an Observers
        // instances.  For non-observe cases, this loop breaks after its first iteration.
        match method_handler {
            Some(handler) => self.do_handle(handler, out, wrapped_request, rng).await,
            None => Err(CoapError::method_not_allowed()),
        }
    }

    // TODO: This method is clunky and generally inefficient but doesn't need to be.  Cloning
    // these parts are especially expensive as it contains the request/response payloads and this
    // can be avoided by rethinking the Request/Response type system a bit and divorcing ourselves
    // from CoapRequest/CoapResponse.
    #[cfg(feature = "observable")]
    async fn do_handle<R: Rng>(
        &self,
        handler: &Box<dyn RequestHandler<Endpoint> + Send + Sync>,
        out: &mut Vec<Packet>,
        wrapped_request: Request<Endpoint>,
        rng: &mut R,
    ) -> Result<(), CoapError> {
        let mut initial_pair = wrapped_request.original.clone();
        if !self.maybe_handle_block_request(&mut initial_pair).await? {
            let fut = {
                self.generate_and_assign_response(
                    handler,
                    &mut initial_pair,
                    wrapped_request.clone(),
                )
            };
            fut.await?
        }
        let registration = self
            .maybe_handle_observe_registration(&mut initial_pair, rng)
            .await?;
        out.push(initial_pair.response.as_ref().unwrap().message.clone());

        if let RegistrationEvent::Registered(mut receiver) = registration {
            debug!("Observe initiated by {:?}", initial_pair.source);
            loop {
                futures_util::select! {
                    _ = &mut receiver.termination_rx => {
                        debug!("Observe terminated by peer: {:?}", initial_pair.source);
                        break
                    }
                    _ = receiver.notify_rx.changed() => {
                        let state = *receiver.notify_rx.borrow();
                        match state {
                            NotificationState::InitialSequence(_) => {
                                // Nothing to do, we already handled the initial sequence in a
                                // nominal reply.
                            }
                            NotificationState::ResourceChanged(change_num) => {
                                let mut current_pair = initial_pair.clone();
                                current_pair.message.clear_option(CoapOption::Observe);
                                let fut = {
                                    self.generate_and_assign_response(
                                        handler,
                                        &mut current_pair,
                                        wrapped_request.clone())
                                };
                                fut.await?;
                                let response_packet = &mut current_pair.response.unwrap().message;
                                response_packet.header.set_type(MessageType::Confirmable);
                                if response_packet.get_observe_value().is_none() {
                                    response_packet.set_observe_value(u32::from(change_num));
                                }

                                // TODO: This logic means that we unintentionally have
                                // a kind of head-of-line blocking for any given observe
                                // registration.  Specifically, we wait for each Ack before
                                // we send the next notification which means the peer will
                                // always be behind the latest update by at least the RTT time.
                                // Fortunately this is not cumulative because of how we're using
                                // a watcher so the next time we see ResourceChanged it'll be
                                // for the latest resource update, skipping any that might have
                                // happened while we waited for the Ack.  The good news is that
                                // this means for any given observation we will only have 1
                                // in flight confirmable message at a time.
                                let send_handle = {
                                    let peer = initial_pair.source.clone().unwrap();
                                    self.retransmission_manager
                                        .lock().await
                                        .send_reliably(response_packet.clone(), peer.clone(), tx.clone())
                                };
                                let send_result = send_handle.into_future().await;
                                if let Err(e) = send_result {
                                    let peer = initial_pair.source.as_ref();
                                    log::warn!("Error sending notification to {peer:?}: {e:?}, unregistering observer...");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "observable"))]
    async fn do_handle<R: Rng>(
        &self,
        handler: &Box<dyn RequestHandler<Endpoint> + Send + Sync>,
        out: &mut Vec<Packet>,
        wrapped_request: Request<Endpoint>,
        _rng: &mut R,
    ) -> Result<(), CoapError> {
        let mut initial_pair = wrapped_request.original.clone();
        if !self.maybe_handle_block_request(&mut initial_pair).await? {
            let fut = {
                self.generate_and_assign_response(
                    handler,
                    &mut initial_pair,
                    wrapped_request.clone(),
                )
            };
            fut.await?
        }
        out.push(initial_pair.response.as_ref().unwrap().message.clone());

        Ok(())
    }

    async fn generate_and_assign_response(
        &self,
        handler: &Box<dyn RequestHandler<Endpoint> + Send + Sync>,
        current_pair: &mut CoapRequest<Endpoint>,
        wrapped_request: Request<Endpoint>,
    ) -> Result<(), CoapError> {
        let wrapped_request_once = wrapped_request.clone();
        let response = handler.handle(wrapped_request_once).await?;
        current_pair.response = Some(response);
        let _ = self.maybe_handle_block_response(current_pair).await?;
        Ok(())
    }

    async fn maybe_handle_block_request(
        &self,
        request: &mut CoapRequest<Endpoint>,
    ) -> Result<bool, CoapError> {
        if let Some(block_handler) = &self.block_handler {
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
        if let Some(block_handler) = &self.block_handler {
            Ok(block_handler.lock().await.intercept_response(request)?)
        } else {
            Ok(false)
        }
    }

    #[cfg(feature = "observable")]
    async fn maybe_handle_observe_registration<R: Rng>(
        &self,
        request: &mut CoapRequest<Endpoint>,
        rng: &mut R,
    ) -> Result<RegistrationEvent, CoapError> {
        if let Some(observe_handler) = &self.observe_handler {
            observe_handler
                .lock()
                .await
                .maybe_process_registration(request, rng)
                .await
        } else {
            Ok(RegistrationEvent::NoChange)
        }
    }
}
