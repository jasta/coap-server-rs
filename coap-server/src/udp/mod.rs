use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use bytes::BytesMut;
use coap_lite::Packet;
use futures::{Sink, Stream};
use log::debug;
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
    mtu: Option<u32>,
    multicast: bool,
    multicast_joiner: MulticastJoiner,
}

#[derive(Default, Debug, Clone)]
struct MulticastJoiner {
    custom_group_joins: Vec<MulticastGroupJoin>,
    default_ipv4_interface: Option<Ipv4Addr>,
    default_ipv6_interface: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum MulticastGroupJoin {
    Ipv4(Ipv4Addr, Option<Ipv4Addr>),
    Ipv6(Ipv6Addr, Option<u32>),
}

impl<A: ToSocketAddrs> UdpTransport<A> {
    pub fn new(addresses: A) -> Self {
        let (mtu, multicast, multicast_joiner) = Default::default();
        Self {
            addresses,
            mtu,
            multicast,
            multicast_joiner,
        }
    }

    /// Manually set the MTU that will be used for block-wise transfer handling purposes.
    pub fn set_mtu(mut self, mtu: u32) -> Self {
        self.mtu = Some(mtu);
        self
    }

    pub fn enable_multicast(mut self) -> Self {
        self.multicast = true;
        self
    }

    pub fn set_multicast_default_ipv4_interface(mut self, interface: Ipv4Addr) -> Self {
        self.multicast_joiner.default_ipv4_interface = Some(interface);
        self
    }

    pub fn set_multicast_default_ipv6_interface(mut self, interface: u32) -> Self {
        self.multicast_joiner.default_ipv6_interface = Some(interface);
        self
    }

    pub fn add_multicast_join(mut self, join: MulticastGroupJoin) -> Self {
        self.multicast_joiner.custom_group_joins.push(join);
        self
    }
}

#[async_trait]
impl<A: ToSocketAddrs + Sync + Send> Transport for UdpTransport<A> {
    type Endpoint = SocketAddr;

    async fn bind(self) -> Result<BoxedFramedBinding<Self::Endpoint>, TransportError> {
        let socket = UdpSocket::bind(self.addresses).await?;
        if self.multicast {
            self.multicast_joiner.join(&socket)?;
        }
        let local_addr = socket.local_addr()?;
        let framed_socket = UdpFramed::new(socket, Codec::default());
        let binding = UdpBinding {
            framed_socket,
            local_addr,
            mtu: self.mtu,
        };
        Ok(Box::pin(binding))
    }
}

impl MulticastJoiner {
    fn join(self, socket: &UdpSocket) -> io::Result<()> {
        let ipv4_interface = match socket.local_addr()? {
            SocketAddr::V4(ipv4) => Some(*ipv4.ip()),
            _ => None,
        };
        let joins = if self.custom_group_joins.is_empty() {
            self.determine_default_joins(&socket)?
        } else {
            self.custom_group_joins
        };

        for join in joins {
            debug!("Joining {join:?}...");
            match join {
                MulticastGroupJoin::Ipv4(addr, interface) => {
                    let resolved_interface = interface
                        .or(self.default_ipv4_interface)
                        .or(ipv4_interface)
                        .unwrap();
                    socket.join_multicast_v4(addr, resolved_interface)?;
                }
                MulticastGroupJoin::Ipv6(addr, interface) => {
                    let resolved_interface = interface.or(self.default_ipv6_interface).unwrap_or(0);
                    socket.join_multicast_v6(&addr, resolved_interface)?;
                }
            }
        }

        Ok(())
    }

    fn determine_default_joins(&self, socket: &UdpSocket) -> io::Result<Vec<MulticastGroupJoin>> {
        let mut joins = Vec::new();
        match socket.local_addr()? {
            SocketAddr::V4(_) => {
                joins.push(MulticastGroupJoin::Ipv4(
                    "224.0.1.187".parse().unwrap(),
                    None,
                ));
            }
            SocketAddr::V6(_) => {
                joins.push(MulticastGroupJoin::Ipv6("ff02::fd".parse().unwrap(), None));
                joins.push(MulticastGroupJoin::Ipv6("ff05::fd".parse().unwrap(), None));
            }
        }
        Ok(joins)
    }
}

#[pin_project]
struct UdpBinding {
    #[pin]
    framed_socket: UdpFramed<Codec>,
    local_addr: SocketAddr,
    mtu: Option<u32>,
}

#[async_trait]
impl FramedBinding<SocketAddr> for UdpBinding {
    fn mtu(&self) -> Option<u32> {
        self.mtu
    }
}

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
