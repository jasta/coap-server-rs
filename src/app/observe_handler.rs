//! General purpose implementation of observe support (RFC 7641).
//!
//! Supports handling incoming requests and manipulating outgoing responses transparently to
//! semi-transparently handle Observe requests from clients in a reasonable and consistent way.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use coap_lite::{CoapRequest, ObserveOption};
use log::{debug, warn};
use rand::RngCore;
use tokio::sync::{oneshot, watch, Mutex};

use crate::app::observers::NotificationState;
use crate::app::path_matcher::key_from_path;
use crate::app::u24::u24;
use crate::app::{CoapError, ObservableResource, Observers};

#[derive(Clone)]
pub struct ObserveHandler<Endpoint> {
    path_prefix: String,
    resource: Arc<Box<dyn ObservableResource + Send + Sync>>,
    registrations_by_path: Arc<Mutex<HashMap<String, RegistrationsForPath<Endpoint>>>>,
}

#[derive(Debug)]
struct RegistrationsForPath<Endpoint> {
    notify_change_tx: Arc<watch::Sender<NotificationState>>,
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

impl<Endpoint: Eq + Hash + Clone + Send + 'static> ObserveHandler<Endpoint> {
    pub fn new(path_prefix: String, resource: Box<dyn ObservableResource + Send + Sync>) -> Self {
        Self {
            path_prefix,
            resource: Arc::new(resource),
            registrations_by_path: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn maybe_process_registration(
        &self,
        request_response_pair: &mut CoapRequest<Endpoint>,
    ) -> Result<RegistrationEvent, CoapError> {
        let observe_flag_result = request_response_pair.get_observe_flag();
        if observe_flag_result.is_none() {
            return Ok(RegistrationEvent::NoChange);
        }

        let observe_flag = observe_flag_result
            .unwrap()
            .map_err(CoapError::bad_request)?;

        let path = request_response_pair.get_path();

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
        let registration_key = RegistrationKey::new(peer, token);

        let mut by_path = self.registrations_by_path.lock().await;

        match observe_flag {
            ObserveOption::Register => {
                let for_path = by_path.entry(path).or_insert_with_key(|path_key| {
                    let mut u24_bytes = [0u8; 3];
                    rand::thread_rng().fill_bytes(&mut u24_bytes);
                    let relative_path = path_key.replace(&self.path_prefix, "");
                    let observers = Observers::new(
                        key_from_path(&relative_path),
                        u24::from_le_bytes(u24_bytes),
                    );
                    let notify_change_tx = observers.leak_notify_change_tx();

                    let self_clone = self.to_owned();
                    let path_key_clone = path_key.clone();
                    tokio::spawn(async move {
                        self_clone
                            .handle_on_active_lifecycle(path_key_clone, observers)
                            .await;
                    });

                    RegistrationsForPath {
                        registrations: HashMap::new(),
                        notify_change_tx,
                    }
                });

                let notify_rx = for_path.notify_change_tx.subscribe();
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

                let existing = for_path.registrations.insert(registration_key, internal);
                let _ = Self::maybe_shutdown_single_observer(existing);

                Ok(RegistrationEvent::Registered(handle))
            }
            ObserveOption::Deregister => {
                let existing = by_path
                    .get_mut(&path)
                    .and_then(|for_path| for_path.registrations.remove(&registration_key));
                match Self::maybe_shutdown_single_observer(existing) {
                    Ok(_) => Ok(RegistrationEvent::Unregistered),
                    Err(_) => Ok(RegistrationEvent::NoChange),
                }
            }
        }
    }

    fn maybe_shutdown_single_observer(existing: Option<InternalRegistration>) -> Result<(), ()> {
        if let Some(existing) = existing {
            let _ = existing.termination_tx.send(());
            return Ok(());
        }
        Err(())
    }

    async fn handle_on_active_lifecycle(&self, path_key: String, observers: Observers) {
        log::debug!("entered on_active for: {path_key}");
        self.resource.on_active(observers).await;
        match self.registrations_by_path.lock().await.remove(&path_key) {
            Some(for_path) => {
                let num_registrations = for_path.registrations.len();
                debug!("{path_key}: shutting down {num_registrations} observers...");
                for (_, internal_reg) in for_path.registrations {
                    let _ = Self::maybe_shutdown_single_observer(Some(internal_reg));
                }
            }
            None => {
                // The user is supposed to be able to hold onto the registration for as long
                // as they like, removing it only here after they release control in on_active.
                // This code path isn't supposed to be possible in practice...
                warn!("Internal registration record removed without user consent, how???");
            }
        }
        log::debug!("exit on_active for: {path_key}");
    }
}

impl<Endpoint> RegistrationKey<Endpoint> {
    pub fn new(peer: Endpoint, token: Vec<u8>) -> Self {
        Self { peer, token }
    }
}

#[derive(Debug)]
pub enum RegistrationEvent {
    NoChange,
    Unregistered,
    Registered(RegistrationHandle),
}
