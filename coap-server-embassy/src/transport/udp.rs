use core::pin::Pin;
use core::task::{ready, Context, Poll};
use std::pin::pin;
use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use coap_lite::Packet;
use embassy_net::IpListenEndpoint;
use embassy_net::udp::{BindError, Error, UdpSocket};
use futures::{Sink, Stream};
use futures::Future;
use pin_project::pin_project;
use smoltcp::wire::{IpEndpoint};
use coap_server::transport::{BoxedFramedBinding, FramedBinding, Transport, TransportError};

/// Taken from RFC 7252
const DEFAULT_MTU_SIZE: usize = 1152;

pub struct UdpTransport<'a, A> {
  socket: UdpSocket<'a>,
  address: A,
  mtu: Option<u32>,
}

// SAFETY: user is expected to not use a multi-threaded executor
unsafe impl<'a, A> Sync for UdpTransport<'a, A> {}

// SAFETY: user is expected to not use a multi-threaded executor
unsafe impl<'a, A> Send for UdpTransport<'a, A> {}

impl<'a, A: Into<IpListenEndpoint>> UdpTransport<'a, A> {
  pub fn new(socket: UdpSocket<'a>, address: A) -> Self {
    Self { socket, address, mtu: None }
  }

  pub fn set_mtu(mut self, mtu: u32) -> Self {
    self.mtu = Some(mtu);
    self
  }
}

#[async_trait]
impl<A: Into<IpListenEndpoint> + Sync + Send> Transport for UdpTransport<'static, A> {
  type Endpoint = IpEndpoint;

  async fn bind(mut self) -> Result<BoxedFramedBinding<Self::Endpoint>, TransportError> {
    let resolved_mtu = self.mtu
        .or_else(|| u32::try_from(DEFAULT_MTU_SIZE).ok())
        .unwrap();
    let resolved_mtu_usize = usize::try_from(resolved_mtu)
        .map_err(|_| {
          TransportError::Unspecified(format!("unable to apply mtu of {resolved_mtu}!"))
        })?;

    self.socket.bind(self.address).map_err(map_bind_err)?;
    let local_addr = self.socket.endpoint();
    let framed_socket = UdpFramed::new(self.socket, resolved_mtu_usize);
    let binding = UdpBinding {
      framed_socket,
      local_addr,
      mtu: resolved_mtu,
    };
    Ok(Box::pin(binding))
  }
}

#[pin_project]
struct UdpBinding<'a> {
  #[pin]
  framed_socket: UdpFramed<'a>,
  local_addr: IpListenEndpoint,
  mtu: u32,
}

#[async_trait]
impl<'a> FramedBinding<IpEndpoint> for UdpBinding<'a> {
  fn mtu(&self) -> Option<u32> {
    Some(self.mtu)
  }
}

impl<'a> Stream for UdpBinding<'a> {
  type Item = Result<(Packet, IpEndpoint), (TransportError, Option<IpEndpoint>)>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.project().framed_socket.poll_next(cx)
  }
}

impl<'a> Sink<(Packet, IpEndpoint)> for UdpBinding<'a> {
  type Error = TransportError;

  fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    self.project().framed_socket.poll_ready(cx)
  }

  fn start_send(self: Pin<&mut Self>, item: (Packet, IpEndpoint)) -> Result<(), Self::Error> {
    self.project().framed_socket.start_send(item)
  }

  fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    self.project().framed_socket.poll_flush(cx)
  }

  fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    self.project().framed_socket.poll_close(cx)
  }
}

struct UdpFramed<'a> {
  socket: UdpSocket<'a>,
  recv_buffer: BytesMut,
  send_buffer: BytesMut,
  destination: Option<IpEndpoint>,
  flushed: bool,
}

impl<'a> UdpFramed<'a> {
  pub fn new(socket: UdpSocket<'a>, resolved_mtu: usize) -> Self {
    Self {
      socket,
      recv_buffer: BytesMut::with_capacity(resolved_mtu),
      send_buffer: BytesMut::with_capacity(resolved_mtu),
      destination: None,
      flushed: true,
    }
  }

  pub fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<(Packet, IpEndpoint), (TransportError, Option<IpEndpoint>)>>> {
    match ready!(self.as_mut().poll_fill_recv_buffer(cx)) {
      Ok(peer) => {
        let pin = self.get_mut();
        let result = Packet::from_bytes(&mut pin.recv_buffer)
            .map(|p| (p, peer))
            .map_err(|e| (TransportError::MalformedPacket(e), Some(peer)));
        pin.recv_buffer.clear();
        Poll::Ready(Some(result))
      }
      Err(e) => Poll::Ready(Some(Err((e, None)))),
    }
  }

  fn poll_fill_recv_buffer(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<IpEndpoint, TransportError>> {
    let pin = self.get_mut();
    pin.recv_buffer.resize(pin.recv_buffer.capacity(), 0);
    let recv_result = ready!(pin!(pin.socket.recv_from(&mut pin.recv_buffer)).poll(cx));
    match recv_result {
      Ok((n, peer)) => {
        pin.recv_buffer.truncate(n);
        Poll::Ready(Ok(peer))
      },
      Err(e) => {
        let translated_e = map_sendrecv_err(e);
        Poll::Ready(Err(translated_e))
      }
    }
  }

  pub fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), TransportError>> {
    if !self.flushed {
      ready!(self.poll_flush(cx)?)
    }

    Poll::Ready(Ok(()))
  }

  pub fn start_send(self: Pin<&mut Self>, item: (Packet, IpEndpoint)) -> Result<(), TransportError> {
    let (packet, destination) = item;

    let pin = self.get_mut();

    pin.send_buffer.clear();
    pin.flushed = false;

    match packet.to_bytes() {
      Ok(encoded) => {
        pin.send_buffer.extend(encoded);
        pin.destination = Some(destination);
        Ok(())
      }
      Err(e) => {
        pin.destination = None;
        Err(TransportError::MalformedPacket(e))
      }
    }
  }

  fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), TransportError>> {
    if !self.flushed {
      match ready!(self.as_mut().poll_send_to(cx)) {
        Ok(()) => {
          let pin = self.get_mut();
          pin.send_buffer.clear();
          pin.flushed = true;
          Poll::Ready(Ok(()))
        }
        Err(e) => Poll::Ready(Err(e)),
      }
    } else {
      Poll::Ready(Ok(()))
    }
  }

  fn poll_send_to(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), TransportError>> {
    match ready!(pin!(self.socket.send_to(&self.send_buffer, self.destination.unwrap())).poll(cx)) {
      Ok(()) => Poll::Ready(Ok(())),
      Err(e) => Poll::Ready(Err(map_sendrecv_err(e))),
    }
  }

  fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), TransportError>> {
    ready!(self.poll_flush(cx))?;
    Poll::Ready(Ok(()))
  }
}

fn map_bind_err(e: BindError) -> TransportError {
  match e {
    BindError::InvalidState => TransportError::Unspecified("invalid state".to_owned()),
    BindError::NoRoute => TransportError::Unspecified("no route".to_owned()),
  }
}

fn map_sendrecv_err(e: embassy_net::udp::Error) -> TransportError {
  match e {
    Error::NoRoute => TransportError::Unspecified("no route".to_owned()),
  }
}

/// Woof.  The tests here are really janky due to missing CI friendly test infrastructure (see
/// https://github.com/embassy-rs/embassy/issues/1695).
///
/// WARNING: These tests leak memory!  Run them in a dedicated, short-lived process!
#[cfg(test)]
mod tests {
  use std::collections::VecDeque;
  use std::io::Read;
  use std::process::exit;
  use std::sync::atomic::AtomicBool;
  use std::sync::mpsc::Sender;
  use std::thread;
  use std::time::Duration;
  use coap_lite::{CoapOption, MessageClass, Packet, RequestType};
  use embassy_executor::Executor;
  use embassy_futures::select::{Either, select};
  use embassy_net::{Config, StackResources, StaticConfigV4};
  use embassy_net::driver::Driver;
  use embassy_net::Stack;
  use embassy_net::udp::UdpSocket;
  use embassy_net_driver_channel::{Device, Runner, State};
  use embassy_net_driver_channel::driver::LinkState;
  use embassy_sync::blocking_mutex::raw::NoopRawMutex;
  use embassy_sync::signal::Signal;
  use futures::{SinkExt, StreamExt};
  use smoltcp::socket::udp::PacketMetadata;
  use smoltcp::wire::{IpEndpoint, IpListenEndpoint, Ipv4Address, Ipv4Cidr};
  use smoltcp::wire::IpAddress;
  use coap_server::transport::Transport;
  use crate::transport::udp::UdpTransport;

  type TestDevice = Device<'static, 1024>;

  #[test]
  pub fn udp_smoke_harness() {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

    // Leak this thread, it's stuck running forever...
    thread::spawn(move || {
      udp_smoke_setup_and_run_forever(shutdown_tx)
    });

    shutdown_rx.recv_timeout(Duration::from_secs(10)).unwrap();
  }

  fn udp_smoke_setup_and_run_forever(shutdown_tx: Sender<()>) {
    let local_address = [0, 1, 2, 3, 4, 5];

    let state = Box::new(State::<1024, 24, 24>::new());
    let stack_resources = Box::new(StackResources::<3>::new());

    let (runner, device) = embassy_net_driver_channel::new(
      Box::leak(state),
      local_address);

    let my_addr = Ipv4Address::new(192, 168, 1, 2);

    let config = Config::ipv4_static(StaticConfigV4 {
      address: Ipv4Cidr::new(my_addr, 24),
      dns_servers: heapless::Vec::from_slice(&[Ipv4Address::new(8, 8, 4, 4).into(), Ipv4Address::new(8, 8, 8, 8).into()]).unwrap(),
      gateway: Some(Ipv4Address::new(192, 168, 1, 1)),
    });

    let seed = rand::random();

    let stack = Box::new(Stack::new(
      device,
      config,
      Box::leak(stack_resources),
      seed,
    ));

    let leaked_stack = Box::leak(stack);
    let socket_factory = SocketFactory::new(leaked_stack);

    let mut executor = Box::new(Executor::new());
    Box::leak(executor).run(|spawner| {
      spawner.spawn(net_driver(runner)).unwrap();
      spawner.spawn(net_task(leaked_stack)).unwrap();
      spawner.spawn(udp_smoke(my_addr, socket_factory, shutdown_tx)).unwrap();
    });
  }

  #[embassy_executor::task]
  async fn net_driver(runner: Runner<'static, 1024>) -> ! {
    let fake_device_data_ready = Signal::<NoopRawMutex, ()>::new();
    let mut fake_device_buf = VecDeque::<u8>::new();

    let (state, mut rx_chan, mut tx_chan) = runner.split();
    state.set_link_state(LinkState::Up);

    loop {
      let rx_future = async {
        fake_device_data_ready.wait().await;
        rx_chan.rx_buf().await
      };
      let tx_future = tx_chan.tx_buf();

      match select(rx_future, tx_future).await {
        Either::First(buf) => {
          let n = fake_device_buf.read(buf).unwrap();
          rx_chan.rx_done(n);
        }
        Either::Second(buf) => {
          fake_device_buf.extend(buf.iter().copied());
          fake_device_data_ready.signal(());
          tx_chan.tx_done();
        }
      }
    }
  }

  #[embassy_executor::task]
  async fn net_task(stack: &'static Stack<TestDevice>) -> ! {
    stack.run().await
  }

  #[embassy_executor::task]
  async fn udp_smoke(
      my_addr: Ipv4Address,
      socket_factory: SocketFactory<'static, TestDevice>,
      shutdown_tx: Sender<()>
  ) {
    let listener_sock = socket_factory.create_leaked_socket();
    let mut sender_sock = socket_factory.create_leaked_socket();

    let listen_port = 12345;

    let listen_addr = IpListenEndpoint {
      addr: Some(IpAddress::Ipv4(my_addr)),
      port: listen_port,
    };
    let target_addr = IpEndpoint::new(IpAddress::Ipv4(my_addr), listen_port);
    let listener = UdpTransport::new(listener_sock, listen_addr);

    let mut binding = listener.bind().await.unwrap();
    let mut expected = Packet::new();
    expected.header.code = MessageClass::Request(RequestType::Get);
    expected.add_option(CoapOption::UriPath, b"hello".to_vec());
    let expected_bytes = expected.to_bytes().unwrap();

    let connect_addr = IpListenEndpoint {
      addr: Some(IpAddress::Ipv4(my_addr)),
      port: 0,
    };
    sender_sock.bind(connect_addr).unwrap();
    sender_sock.send_to(&expected_bytes, target_addr).await.unwrap();

    // Test the receive path...
    let received = binding.next().await.unwrap().unwrap();
    let received_bytes = received.0.to_bytes().unwrap();
    assert_eq!(received.1.addr.to_string(), my_addr.to_string());
    assert_eq!(received_bytes, expected_bytes);

    // Test the send path...
    binding.send((expected, received.1)).await.unwrap();
    let mut recv_buf = [0u8; 4096];
    let (n, peer) = sender_sock.recv_from(&mut recv_buf).await.unwrap();
    assert_eq!(peer.addr.to_string(), my_addr.to_string());
    assert_eq!(&recv_buf[0..n], expected_bytes);

    shutdown_tx.send(()).unwrap();
  }

  struct SocketFactory<'a, D: Driver> {
    stack: &'a Stack<D>,
  }

  impl<'a, D: Driver> SocketFactory<'a, D> {
    pub fn new(stack: &'a Stack<D>) -> Self {
      Self { stack }
    }

    pub fn create_leaked_socket(&self) -> UdpSocket<'a> {
      let mut buffers = Box::new(Buffers::default());
      let leaked = Box::leak(buffers);
      UdpSocket::new(
        &self.stack,
        &mut leaked.rx_meta,
        &mut leaked.rx_buffer,
        &mut leaked.tx_meta,
        &mut leaked.tx_buffer)
    }
  }

  struct Buffers {
    rx_meta: [PacketMetadata; 16],
    tx_meta: [PacketMetadata; 16],
    rx_buffer: [u8; 4096],
    tx_buffer: [u8; 4096],
  }

  impl Default for Buffers {
    fn default() -> Self {
      Self {
        rx_meta: [PacketMetadata::EMPTY; 16],
        tx_meta: [PacketMetadata::EMPTY; 16],
        rx_buffer: [0; 4096],
        tx_buffer: [0; 4096],
      }
    }
  }
}