use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use bytes::BytesMut;
use coap_lite::Packet;
use futures::{Sink, Stream};
use pin_project::pin_project;
use tokio::net::{ToSocketAddrs, UdpSocket};
use tokio_util::codec::{Decoder, Encoder};

use crate::transport::{BoxedFramedBinding, FramedBinding, Transport, TransportError};
use crate::udp::udp_framed_fork::UdpFramed;

pub(crate) mod udp_framed_fork;

/// Default CoAP transport as originally defined in RFC 7252.  Likely this is what you want if
/// you're new to CoAP.
pub struct UdpTransport<A: ToSocketAddrs> {
    addresses: A,
}

impl<A: ToSocketAddrs> UdpTransport<A> {
    pub fn new(addresses: A) -> Self {
        Self { addresses }
    }
}

#[async_trait]
impl<A: ToSocketAddrs + Sync + Send> Transport for UdpTransport<A> {
    type Endpoint = SocketAddr;

    async fn bind(self) -> Result<BoxedFramedBinding<Self::Endpoint>, TransportError> {
        let socket = UdpSocket::bind(self.addresses).await?;
        let local_addr = socket.local_addr()?;
        let framed_socket = UdpFramed::new(socket, Codec::default());
        let binding = UdpBinding {
            framed_socket,
            local_addr,
        };
        Ok(Box::pin(binding))
    }
}

#[pin_project]
struct UdpBinding {
    #[pin]
    framed_socket: UdpFramed<Codec>,
    local_addr: SocketAddr,
}

#[async_trait]
impl FramedBinding<SocketAddr> for UdpBinding {}

impl Stream for UdpBinding {
    type Item = Result<(Packet, SocketAddr), (TransportError, Option<SocketAddr>)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().framed_socket.poll_next(cx)
    }
}

impl Sink<(Packet, SocketAddr)> for UdpBinding {
    type Error = TransportError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().framed_socket.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: (Packet, SocketAddr)) -> Result<(), Self::Error> {
        self.project().framed_socket.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().framed_socket.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().framed_socket.poll_close(cx)
    }
}

#[derive(Default)]
struct Codec;

impl Decoder for Codec {
    type Item = Packet;
    type Error = TransportError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Packet>, TransportError> {
        if buf.is_empty() {
            return Ok(None);
        }
        let result = (|| Ok(Some(Packet::from_bytes(buf)?)))();
        buf.clear();
        result
    }
}

impl Encoder<Packet> for Codec {
    type Error = TransportError;

    fn encode(&mut self, my_packet: Packet, buf: &mut BytesMut) -> Result<(), TransportError> {
        buf.extend_from_slice(&my_packet.to_bytes()?[..]);
        Ok(())
    }
}
