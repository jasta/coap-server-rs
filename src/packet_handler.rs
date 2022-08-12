use core::pin::Pin;

use alloc::boxed::Box;
use coap_lite::Packet;
use futures::Stream;
use rand::Rng;

/// "Low-level" raw packet handler intended to support the full range of CoAP features.  This
/// is little more than a callback informing the user that a packet has arrived, allowing for
/// an arbitrary number of arbitrary responses to be delivered back to this Endpoint.
///
/// Most customers should steer clear of this footgun.  It can be used in such a way that
/// breaks protocol norms and could confuse clients easily.  Prefer [`crate::app::new`] instead.
pub trait PacketHandler<Endpoint>: Clone {
    fn handle<'a>(
        &'a self,
        packet: Packet,
        peer: Endpoint,
    ) -> Pin<Box<dyn Stream<Item = Packet> + Send + 'a>>;
}

pub trait IntoHandler<Handler, Endpoint, R>
where
    Handler: PacketHandler<Endpoint> + Send + 'static,
    R: Rng + Send + Clone,
{
    fn into_handler(self, mtu: Option<u32>, rng: R) -> Handler;
}

impl<Handler, Endpoint, R> IntoHandler<Handler, Endpoint, R> for Handler
where
    Handler: PacketHandler<Endpoint> + Send + 'static,
    R: Rng + Send + Clone,
{
    fn into_handler(self, _mtu: Option<u32>, _rng: R) -> Handler {
        self
    }
}
