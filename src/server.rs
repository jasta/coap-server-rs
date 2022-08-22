use core::fmt::{self, Debug};
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use coap_lite::Packet;
use embassy_executor::executor::Spawner;
use embassy_util::Either;
#[cfg(feature = "embassy")]
use embassy_util::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::mpmc::{DynamicReceiver, DynamicSender},
};
use futures::stream::Fuse;
use futures::{SinkExt, StreamExt};
use log::{error, trace, warn};
use rand::Rng;
#[cfg(feature = "tokio")]
use tokio::sync::mpsc::{Receiver, Sender};

use crate::packet_handler::{IntoHandler, PacketHandler};
use crate::transport::{FramedBinding, FramedItem, FramedReadError, Transport, TransportError};

/// Primary server API to configure, bind, and ultimately run the CoAP server.
pub struct CoapServer<'a, Handler, Endpoint> {
    binding: Fuse<Pin<Box<dyn FramedBinding<Endpoint>>>>,
    packet_relay_rx: DynamicReceiver<'a, FramedItem<Endpoint>>,
    packet_relay_tx: DynamicSender<'a, FramedItem<Endpoint>>,
    handler: Option<Handler>,
}

impl<'a, Handler, Endpoint: Debug + Send + Clone + 'static> CoapServer<'a, Handler, Endpoint>
where
    Handler: PacketHandler<Endpoint> + Send + 'static,
{
    /// Bind the server to a specific source of incoming packets in a transport-agnostic way.  Most
    /// customers will wish to use [`crate::udp::UdpTransport`].
    pub async fn bind<T: Transport<Endpoint = Endpoint>>(
        transport: T,
    ) -> Result<CoapServer<'a, Handler, Endpoint>, TransportError> {
        let binding = transport.bind().await?;
        let channel = Box::new(embassy_util::channel::mpmc::Channel::<
            CriticalSectionRawMutex,
            _,
            32,
        >::new());
        // FIXME: avoid memory leak
        let channel = Box::leak(channel);
        let packet_tx = channel.sender().into();
        let packet_rx = channel.receiver().into();
        Ok(Self {
            binding: binding.fuse(),
            packet_relay_rx: packet_rx,
            packet_relay_tx: packet_tx,
            handler: None,
        })
    }

    /// Run the server "forever".  Note that the function may return a fatal error if the server
    /// encounters unrecoverable issues, typically due to programmer error in this crate itself
    /// or transport errors not related to a specific peer.  The intention is that this crate
    /// should be highly reliable and run indefinitely for properly configured use cases.
    pub async fn serve<R: Rng + Send + Clone>(
        mut self,
        handler: impl IntoHandler<Handler, Endpoint, R>,
        rng: R,
    ) -> Result<(), FatalServerError> {
        let mtu = self.binding.get_ref().mtu();
        self.handler = Some(handler.into_handler(mtu, rng));

        let spawner = embassy_executor::executor::Spawner::for_current_executor().await;
        loop {
            match embassy_util::select(self.binding.select_next_some(), self.packet_relay_rx.recv())
                .await
            {
                Either::First(event) => self.handle_rx_event(event, spawner).await?,
                Either::Second(item) => self.handle_packet_relay(item).await,
            }
        }
    }

    async fn handle_rx_event(
        &self,
        result: Result<FramedItem<Endpoint>, FramedReadError<Endpoint>>,
        spawner: Spawner,
    ) -> Result<(), FatalServerError> {
        match result {
            Ok((packet, peer)) => {
                trace!("Incoming packet from {peer:?}: {packet:?}");
                self.do_handle_request(packet, peer, spawner).await?
            }
            Err((transport_err, peer)) => {
                warn!("Error from {peer:?}: {transport_err}");
                if peer.is_none() {
                    return Err(transport_err.into());
                }
            }
        }

        Ok(())
    }

    async fn do_handle_request(
        &self,
        packet: Packet,
        peer: Endpoint,
        _spawner: Spawner,
    ) -> Result<(), FatalServerError> {
        let handler = self
            .handler
            .as_ref()
            .ok_or_else(|| FatalServerError::InternalError("handler not set".to_string()))?;
        let reply_stream = Self::gen_and_send_responses(
            handler.clone(),
            self.packet_relay_tx.clone(),
            packet,
            peer,
        );
        // FIXME: should spawn the task onto executor but embassy can spawn only
        // static futures.
        reply_stream.await;
        Ok(())
    }

    async fn gen_and_send_responses(
        handler: Handler,
        packet_tx: DynamicSender<'_, FramedItem<Endpoint>>,
        packet: Packet,
        peer: Endpoint,
    ) {
        let mut stream = handler.handle(packet, peer.clone());
        while let Some(response) = stream.next().await {
            let cloned_peer = peer.clone();
            packet_tx.send((response, cloned_peer)).await;
        }
    }

    async fn handle_packet_relay(&mut self, item: FramedItem<Endpoint>) {
        let peer = item.1.clone();
        trace!("Outgoing packet to {:?}: {:?}", peer, item.0);
        if let Err(e) = self.binding.send(item).await {
            error!("Error sending to {peer:?}: {e:?}");
        }
    }
}

/// Fatal error preventing the server from starting or continuing.  Typically the result of
/// programmer error or misconfiguration.
#[derive(Debug)]
pub enum FatalServerError {
    /// Programmer error within this crate, file a bug!
    InternalError(String),

    /// Transport error that is not related to any individual peer but would prevent any future
    /// packet exchanges on the transport.  Must abort the server.
    Transport(TransportError),
}

impl fmt::Display for FatalServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InternalError(err) => write!(f, "internal error: {}", err),
            Self::Transport(err) => write!(f, "fatal transport error: {}", err),
        }
    }
}

impl From<TransportError> for FatalServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}
