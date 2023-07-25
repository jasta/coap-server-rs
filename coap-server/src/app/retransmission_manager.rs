use anyhow::anyhow;
use hashbrown::HashMap;
use core::fmt::Debug;
use core::hash::Hash;
use core::ops::RangeInclusive;
use core::time::Duration;
use alloc::format;
use alloc::string::String;
use std::ops::Deref;

use coap_lite::{MessageType, Packet};
use log::debug;
use rand::Rng;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;
use tokio::time;

pub type MessageId = u16;

/// Applies appropriate ack timeouts and retry policies for Confirmable messages that are
/// sent through it.
pub struct RetransmissionManager<Endpoint: Debug + Clone + Eq + Hash> {
    next_message_id: MessageId,
    unacknowledged_messages: HashMap<MessageKey<Endpoint>, ReplyHandle>,
    parameters: TransmissionParameters,
}

#[derive(Debug, Clone, Copy)]
pub struct TransmissionParameters {
    ack_timeout: Duration,
    ack_random_factor: f32,
    max_retransmit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Hash)]
struct MessageKey<Endpoint: Debug + Clone + Eq + Hash> {
    message_id: MessageId,
    peer: Endpoint,
}

struct ReplyHandle {
    reply_tx: watch::Sender<ReplyEvent>,
}

#[derive(Debug, Clone)]
enum ReplyEvent {
    None,
    PeerResponse(MessageType),
    InternalError(String),
}

impl<Endpoint: Debug + Clone + Eq + Hash> RetransmissionManager<Endpoint> {
    pub fn new(parameters: TransmissionParameters) -> Self {
        Self {
            next_message_id: rand::thread_rng().gen(),
            unacknowledged_messages: Default::default(),
            parameters,
        }
    }

    /// Attempts to handle either the Acknowledgement or Reset message that we expect as a reply
    /// to our Confirmable message send attempts.
    pub fn maybe_handle_reply(&mut self, packet: Packet, peer: &Endpoint) -> Result<(), Packet> {
        match packet.header.get_type() {
            MessageType::Acknowledgement | MessageType::Reset => {}
            _ => return Err(packet),
        }
        let key = MessageKey::new(&packet, peer.clone());
        if let Some(ack_handle) = self.unacknowledged_messages.remove(&key) {
            ack_handle
                .reply_tx
                .send(ReplyEvent::PeerResponse(packet.header.get_type()))
                .unwrap();
            Ok(())
        } else {
            Err(packet)
        }
    }

    /// Long running send operation that will handle all the timeout and retry logic internally.
    /// This design makes it trivial for each individual call to manage its own
    /// error behaviour without dealing with clumsy callbacks.
    ///
    /// Note that this method mutates the packet that is to be sent to ensure it is Confirmable
    /// and has an appropriate message ID.  This ensures that the method is infallible.
    pub fn send_reliably(
        &mut self,
        mut packet: Packet,
        peer: Endpoint,
        packet_tx: UnboundedSender<Packet>,
    ) -> SendReliably<Endpoint> {
        packet.header.message_id = self.next_message_id;
        self.next_message_id = self.next_message_id.wrapping_add(1);
        packet.header.set_type(MessageType::Confirmable);

        let (reply_tx, reply_rx) = watch::channel(ReplyEvent::None);
        let ack_handle = ReplyHandle { reply_tx };
        let key = MessageKey::new(&packet, peer.clone());
        if let Some(existing_send) = self.unacknowledged_messages.insert(key.clone(), ack_handle) {
            let _ = existing_send
                .reply_tx
                .send(ReplyEvent::InternalError(format!(
                    "Re-used message key {key:?} by another send!"
                )));
        }

        SendReliably {
            packet,
            packet_tx,
            peer,
            parameters: self.parameters,
            reply_rx,
        }
    }
}

/// Default values come from the
/// [CoAP RFC](https://datatracker.ietf.org/doc/html/rfc7252#section-4.8.2).
impl Default for TransmissionParameters {
    fn default() -> Self {
        Self {
            ack_timeout: Duration::from_secs(2),
            ack_random_factor: 1.5,
            max_retransmit: 4,
        }
    }
}

impl TransmissionParameters {
    pub fn new(
        ack_timeout: Duration,
        ack_random_factor: f32,
        max_retransmit: usize,
    ) -> anyhow::Result<Self> {
        if ack_random_factor < 1.0 {
            return Err(anyhow!("Invalid ack_random_factor={ack_random_factor}"));
        }
        if ack_timeout.is_zero() {
            return Err(anyhow!("Invalid ack_timeout={ack_timeout:?}"));
        }
        Ok(Self {
            ack_timeout,
            ack_random_factor,
            max_retransmit,
        })
    }

    pub fn ack_timeout_range(&self) -> RangeInclusive<Duration> {
        let timeout_low = self.ack_timeout;
        if self.ack_random_factor != 1.0 {
            let timeout_high = timeout_low.mul_f32(self.ack_random_factor);
            timeout_low..=timeout_high
        } else {
            timeout_low..=timeout_low
        }
    }
}

#[must_use = "don't forget to call into_future() and await it!"]
pub struct SendReliably<Endpoint> {
    packet: Packet,
    peer: Endpoint,
    packet_tx: UnboundedSender<Packet>,
    parameters: TransmissionParameters,
    reply_rx: watch::Receiver<ReplyEvent>,
}

impl<Endpoint: Debug> SendReliably<Endpoint> {
    pub fn get_message_id(&self) -> MessageId {
        self.packet.header.message_id
    }

    pub async fn into_future(self) -> Result<(), SendFailed> {
        let mut next_timeout = rand::thread_rng().gen_range(self.parameters.ack_timeout_range());
        for attempt in 0..=self.parameters.max_retransmit {
            if attempt > 0 {
                let retransmits = attempt - 1;
                let message_id = self.packet.header.message_id;
                let peer = &self.peer;
                debug!("Attempting retransmission #{retransmits} of message ID {message_id} to {peer:?}");
            }
            self.packet_tx
                .send(self.packet.clone())
                .map_err(anyhow::Error::msg)?;
            let curr_timeout = next_timeout;
            next_timeout *= 2;
            loop {
                let mut reply_rx = self.reply_rx.clone();
                let timeout = time::sleep(curr_timeout);

                tokio::select! {
                    _ = reply_rx.changed() => {
                        match reply_rx.borrow().deref() {
                            ReplyEvent::None => {}
                            ReplyEvent::PeerResponse(t) if t == &MessageType::Acknowledgement => {
                                return Ok(());
                            }
                            ReplyEvent::PeerResponse(t) if t == &MessageType::Reset => {
                                return Err(SendFailed::Reset);
                            }
                            ReplyEvent::PeerResponse(t) => {
                                return Err(SendFailed::InternalError(format!("unexpected t={t:?}")));
                            }
                            ReplyEvent::InternalError(e) => return Err(SendFailed::InternalError(e.to_owned())),
                        }
                    }
                    _ = timeout => break,
                }
            }
        }
        Err(SendFailed::NoReply(self.parameters.max_retransmit + 1))
    }
}

#[derive(thiserror_no_std::Error, Debug)]
pub enum SendFailed {
    #[error("no remote reply after {0} attempts")]
    NoReply(usize),

    #[error("reset message received")]
    Reset,

    #[error(transparent)]
    TransmissionError(#[from] anyhow::Error),

    #[error("internal error: {0}")]
    InternalError(String),
}

impl<Endpoint: Debug + Clone + Eq + Hash> MessageKey<Endpoint> {
    fn new(packet: &Packet, peer: Endpoint) -> Self {
        Self {
            message_id: packet.header.message_id,
            peer,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::coap_utils::new_pong_message;
    use crate::app::retransmission_manager::{
        RetransmissionManager, SendFailed, TransmissionParameters,
    };
    use coap_lite::{MessageType, Packet};
    use futures::StreamExt;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    #[derive(Default, Debug, Clone, PartialEq, Eq, Hash)]
    struct TestEndpoint(i32);

    #[tokio::test(start_paused = true)]
    async fn test_timeout() {
        let ack_timeout = Duration::from_secs(5);
        let mut manager =
            RetransmissionManager::new(TransmissionParameters::new(ack_timeout, 1.0, 1).unwrap());
        let (packet_tx, packet_rx) = mpsc::unbounded_channel();

        let mut sent_packet = Packet::new();
        let mut message_id = None;
        let result = {
            let handle = manager.send_reliably(sent_packet, &TestEndpoint(123), packet_tx);
            message_id = Some(handle.get_message_id());
            handle.into_future().await
        };

        if let Err(SendFailed::NoReply(2)) = result {
        } else {
            panic!("Expected send failed!");
        }

        let received: Vec<_> = UnboundedReceiverStream::new(packet_rx).collect().await;

        assert_eq!(received.len(), 2);
        assert_eq!(received[0].header.message_id, message_id.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn test_happy_path() {
        let ack_timeout = Duration::from_secs(999);
        let mut manager =
            RetransmissionManager::new(TransmissionParameters::new(ack_timeout, 1.0, 0).unwrap());
        let (packet_tx, _packet_rx) = mpsc::unbounded_channel();

        let mut sent_packet = Packet::new();
        sent_packet.header.message_id = 5;

        let mut ack_packet = Packet::new();
        ack_packet.header.set_type(MessageType::Acknowledgement);
        ack_packet.header.message_id = sent_packet.header.message_id;

        let result = {
            let handle = manager.send_reliably(sent_packet, TestEndpoint(123), packet_tx);
            ack_packet.header.message_id = handle.get_message_id();
            manager
                .maybe_handle_reply(ack_packet, &TestEndpoint(123))
                .unwrap();
            handle.into_future().await
        };

        result.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn test_reset() {
        let ack_timeout = Duration::from_secs(5);
        let mut manager =
            RetransmissionManager::new(TransmissionParameters::new(ack_timeout, 1.0, 0).unwrap());
        let (packet_tx, _packet_rx) = mpsc::unbounded_channel();

        let mut sent_packet = Packet::new();
        sent_packet.header.message_id = 5;

        let mut reset_packet = new_pong_message(&sent_packet);

        let result = {
            let handle = manager.send_reliably(sent_packet, TestEndpoint(123), packet_tx);
            reset_packet.header.message_id = handle.get_message_id();
            manager
                .maybe_handle_reply(reset_packet, &TestEndpoint(123))
                .unwrap();
            handle.into_future().await
        };

        if let Err(SendFailed::Reset) = result {
        } else {
            panic!("Expected send failed!");
        }
    }
}
