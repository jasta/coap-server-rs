use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use coap_lite::{CoapRequest, CoapResponse, Packet};
use futures::{Sink, Stream, StreamExt};
use async_trait::async_trait;
use log::warn;
use crate::tokio_udp::UdpTransport;

#[tokio::main]
async fn main() {
  let server = CoapServer::bind(UdpTransport::new("0.0.0.0"));
  server.serve(handler);
}

pub struct CoapServer<E, T: PacketExchanger<Endpoint = E>, H: RawRequestHandler<T>> {
  transport: T,
  handler: Option<H>,
  _marker: PhantomData<E>,
}

impl<E, T: PacketExchanger<Endpoint = E>, H: RawRequestHandler<T>> CoapServer<E, T, H> {
  pub async fn bind(mut transport: T) -> Result<Self, TransportError> {
    transport.perform_bind().await?;
    Ok(Self { transport, handler: None, _marker: PhantomData::default() })
  }

  pub async fn serve(self, handler: H) {

  }
}

pub trait RawRequestHandler<T: PacketExchanger> {
  fn handle(&mut self, packet: Packet, peer: T::Endpoint) -> Pin<Box<dyn Stream<Item=Packet>>>;
}

#[async_trait]
pub trait Transport {
  type Binding;
  async fn bind(&self) -> Result<Self::Binding, TransportError>;
}

pub trait Binding:
    Send +
    Stream<Item = Result<(Packet, Self::Endpoint), (TransportError, Self::Endpoint)>> +
    Sink<(Packet, Self::Endpoint), Error = TransportError> {
  type Endpoint;
}

#[derive(thiserror::Error, Debug)]
pub enum TransportError {
  #[error("generic I/O error")]
  IoError(#[from] Option<io::Error>),

  #[error("unspecified: {0}")]
  Unspecified(String),
}

mod tokio_udp {
  use std::net::{SocketAddr, ToSocketAddrs};
  use std::pin::Pin;
  use std::task::{Context, Poll};
  use async_trait::async_trait;
  use coap_lite::Packet;
  use tokio::net::UdpSocket;
  use crate::{Stream, PacketExchanger, TransportError, Transport, Binding, Sink};

  pub struct UdpTransport<A: ToSocketAddrs> {
    addresses: A,
  }

  impl<A: ToSocketAddrs> UdpTransport<A> {
    pub fn new(addresses: A) -> Self {
      Self { addresses }
    }
  }

  #[async_trait]
  impl<A: ToSocketAddrs> Transport for UdpTransport<A> {
    type Binding = UdpBinding;

    async fn bind(&self) -> Result<UdpBinding, TransportError> {
      let socket = UdpSocket::bind(self.addresses.clone()).await?;
      Ok(UdpBinding { socket })
    }
  }

  struct UdpBinding {
    socket: UdpSocket,
  }

  #[async_trait]
  impl Binding for UdpBinding {
    type Endpoint = SocketAddr;
  }

  impl Stream for UdpBinding {
    type Item = Result<(Packet, SocketAddr), (TransportError, SocketAddr)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
      todo!()
    }
  }

  impl Sink<(Packet, SocketAddr)> for UdpBinding {
    type Error = TransportError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      todo!()
    }

    fn start_send(self: Pin<&mut Self>, item: (Packet, ::Endpoint)) -> Result<(), Self::Error> {
      todo!()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      todo!()
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      todo!()
    }
  }
}

mod simple {
  use coap_lite::CoapRequest;
  use async_trait::async_trait;

  #[async_trait]
  pub trait SimpleRequestHandler<Endpoint> {
    async fn handle(&mut self, request: &mut CoapRequest<Endpoint>) -> ResponseBehaviour;
  }

  #[derive(Debug, Clone, Copy)]
  pub enum ResponseBehaviour {
    /// Respond normally as per the protocol definition (i.e. when a response is expected,
    /// provided one; otherwise do not).
    Normal,

    /// Suppress the response in all cases, even if it would otherwise be warranted or appropriate
    /// according to the base protocol specification.  For example, this might be used to
    /// suppress a /.well-known/core discovery request over multicast if the filter criteria is
    /// not met.
    Suppress,
  }
}
