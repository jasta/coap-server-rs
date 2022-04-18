//! General purpose implementation of observe support (RFC 7641).
//!
//! Supports handling incoming requests and manipulating outgoing responses transparently to
//! semi-transparently handle Observe requests from clients in a reasonable and consistent way.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use coap_lite::{CoapRequest, ObserveOption};
use futures::StreamExt;
use rand::RngCore;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::app::observers::NotificationState;
use crate::app::u24::u24;
use crate::app::{CoapError, ObservableResource, Observers};

#[derive(Debug)]
pub struct ObserveHandler<Endpoint> {
    observers_change_dispatcher: UnboundedSender<ObserverDispatchEvent>,
    notify_change_tx: Option<Arc<watch::Sender<NotificationState>>>,
    registrations: HashMap<RegistrationKey<Endpoint>, InternalRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegistrationKey<Endpoint> {
    peer: Endpoint,
    token: Vec<u8>,
}

#[derive(Debug)]
pub struct InternalRegistration {
    pub termination_tx: oneshot::Sender<()>,
}

#[derive(Debug)]
pub struct RegistrationHandle {
    pub termination_rx: oneshot::Receiver<()>,
    pub notify_rx: watch::Receiver<NotificationState>,
}

impl<Endpoint: Eq + Hash + Clone> ObserveHandler<Endpoint> {
    pub fn new(resource: Box<dyn ObservableResource + Send + Sync>) -> Self {
        let observers_change_dispatcher = OnObserversChangeDispatcher::start(resource);
        Self {
            observers_change_dispatcher,
            notify_change_tx: None,
            registrations: HashMap::new(),
        }
    }

    pub fn maybe_process_registration(
        &mut self,
        request_response_pair: &mut CoapRequest<Endpoint>,
    ) -> Result<RegistrationEvent, CoapError> {
        let observe_flag_result = request_response_pair.get_observe_flag();
        if observe_flag_result.is_none() {
            return Ok(RegistrationEvent::NoChange);
        }

        let observe_flag = observe_flag_result
            .unwrap()
            .map_err(CoapError::bad_request)?;

        let response = request_response_pair
            .response
            .as_mut()
            .ok_or_else(|| CoapError::internal("No response prepared"))?;
        let response_sequence =
            if let Some(observe_value) = response.message.get_observe_value() {
                Some(observe_value.map_err(|_| {
                    CoapError::internal("Invalid Observe option in prepared response")
                })?)
            } else {
                None
            };

        let peer = request_response_pair
            .source
            .as_ref()
            .ok_or_else(|| CoapError::internal("Missing source!"))?
            .to_owned();
        let token = request_response_pair.message.get_token().to_vec();
        let registration = RegistrationKey::new(peer, token);

        match observe_flag {
            ObserveOption::Register => {
                let receiver_count = self
                    .notify_change_tx
                    .as_ref()
                    .map(|s| s.receiver_count())
                    .unwrap_or(0);
                let notify_change_tx = if receiver_count > 0 {
                    self.notify_change_tx.as_ref().unwrap()
                } else {
                    let mut u24_bytes = [0u8; 3];
                    rand::thread_rng().fill_bytes(&mut u24_bytes);
                    let observers = Observers::new(u24::from_le_bytes(u24_bytes));
                    let notify_change_tx = observers.leak_notify_change_tx();
                    self.observers_change_dispatcher
                        .send(ObserverDispatchEvent::OnFirstObserver(observers))
                        .unwrap();
                    self.notify_change_tx.insert(notify_change_tx)
                };
                let notify_rx = notify_change_tx.subscribe();
                let current_state = *notify_rx.borrow();

                let (termination_tx, termination_rx) = oneshot::channel();
                let internal = InternalRegistration { termination_tx };
                let handle = RegistrationHandle {
                    notify_rx,
                    termination_rx,
                };

                let change_num = match current_state {
                    NotificationState::InitialSequence(change_num) => change_num,
                    NotificationState::ResourceChanged(change_num) => change_num,
                };
                if response_sequence.is_none() {
                    response.message.set_observe_value(u32::from(change_num));
                }

                let existing = self.registrations.insert(registration, internal);
                let _ = Self::maybe_shutdown_existing(existing);

                Ok(RegistrationEvent::Registered(handle))
            }
            ObserveOption::Deregister => {
                let existing = self.registrations.remove(&registration);
                match Self::maybe_shutdown_existing(existing) {
                    Ok(_) => Ok(RegistrationEvent::Unregistered),
                    Err(_) => Ok(RegistrationEvent::NoChange),
                }
            }
        }
    }

    fn maybe_shutdown_existing(existing: Option<InternalRegistration>) -> Result<(), ()> {
        if let Some(existing) = existing {
            existing.termination_tx.send(()).unwrap();
            return Ok(());
        }
        Err(())
    }
}

impl<Endpoint> RegistrationKey<Endpoint> {
    pub fn new(peer: Endpoint, token: Vec<u8>) -> Self {
        Self { peer, token }
    }
}

pub trait ContextFactory<Context> {
    fn new_context(&self, initial_sequence: u32) -> Context;
}

/// Responsible for ensuring that we dispatch calls to [`ObservableResource`] are serial (i.e.
/// the customer can expect that two concurrent `on_active` calls will not be possible.
struct OnObserversChangeDispatcher {
    resource: Box<dyn ObservableResource + Send + Sync>,
    rx: UnboundedReceiverStream<ObserverDispatchEvent>,
}

impl OnObserversChangeDispatcher {
    pub fn start(
        resource: Box<dyn ObservableResource + Send + Sync>,
    ) -> UnboundedSender<ObserverDispatchEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let me = Self {
            resource,
            rx: UnboundedReceiverStream::new(rx),
        };
        tokio::spawn(async move {
            me.run().await;
        });
        tx
    }

    pub async fn run(mut self) {
        while let Some(event) = self.rx.next().await {
            match event {
                ObserverDispatchEvent::OnFirstObserver(observers) => {
                    log::debug!("entered OnFirstObserver");
                    self.resource.on_active(observers).await;

                    // TODO: We should probably be mutating the state here to drop all of the observers and
                    // cleanup state but it technically isn't necessary (or so I assume hehe) because
                    // we can just cleanup the registrations cheaply next time the resource is
                    // Registered (and we check that the Observers instance has no subscribers).
                    // This does mean however that an app cannot signal that for some reason
                    // there's a problem observing the resource and we must forcefully
                    // cleanup with our peers.
                    log::debug!("exit OnFirstObserver");
                }
            }
        }
    }
}

#[derive(Debug)]
enum ObserverDispatchEvent {
    OnFirstObserver(Observers),
}

#[derive(Debug)]
pub enum RegistrationEvent {
    NoChange,
    Unregistered,
    Registered(RegistrationHandle),
}
