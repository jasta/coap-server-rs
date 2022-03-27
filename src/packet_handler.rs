use std::pin::Pin;

use coap_lite::Packet;
use futures::Stream;

/// "Low-level" raw packet handler intended to support the full range of CoAP features.  This
/// is little more than a callback informing the user that a packet has arrived, allowing for
/// an arbitrary number of arbitrary responses to be delivered back to this Endpoint.
///
/// Most customers should steer clear of this footgun.  It can be used in such a way that
/// breaks protocol norms and could confuse clients easily.  Prefer [`crate::service::Service`]
/// instead.
pub trait PacketHandler<Endpoint>: Clone {
    fn handle<'a>(
        &'a self,
        packet: Packet,
        peer: Endpoint,
    ) -> Pin<Box<dyn Stream<Item = Packet> + Send + 'a>>;
}

// #[async_trait]
// pub trait SimpleRequestHandler<'a, Endpoint> {
//   async fn handle(&'a mut self, request: &mut CoapRequest<Endpoint>) -> ResponseBehaviour;
// }
//
// pub fn to_raw<'a, Endpoint, H>(simple_handler: H) -> impl RawRequestHandler<Endpoint> + 'a
// where
//     H: SimpleRequestHandler<'a, Endpoint> + 'a + Sync + 'static,
//     Endpoint: Send + 'a {
//   SimpleToRawAdapter { simple_handler: Box::new(simple_handler) }
// }
//
// struct SimpleToRawAdapter<'a, Endpoint> {
//   simple_handler: Box<dyn SimpleRequestHandler<'a, Endpoint> + Sync + 'a>
// }
//
// impl<'a, Endpoint> SimpleToRawAdapter<'a, Endpoint> {
//   async fn handle_internal(&'a mut self, mut request: CoapRequest<Endpoint>) -> Option<Packet> {
//     match self.simple_handler.handle(&mut request).await {
//       ResponseBehaviour::Normal => request.response.map(|r| r.message),
//       ResponseBehaviour::Suppress => None,
//     }
//   }
// }
//
// impl<'a, Endpoint: Send> RawRequestHandler<Endpoint> for SimpleToRawAdapter<'a, Endpoint> {
//   fn handle(&'a mut self, packet: Packet, peer: Endpoint) -> Pin<Box<dyn Stream<Item=Packet> + Send + 'a>> {
//     Box::pin(async_stream::stream! {
//       let request = CoapRequest::from_packet(packet, peer);
//       if let Some(packet) = self.handle_internal(request).await {
//         yield packet;
//       }
//     })
//   }
// }
//
// #[derive(Debug, Clone, Copy)]
// pub enum ResponseBehaviour {
//   /// Respond normally as per the protocol definition (i.e. when a response is expected,
//   /// provided one; otherwise do not).
//   Normal,
//
//   /// Suppress the response in all cases, even if it would otherwise be warranted or appropriate
//   /// according to the base protocol specification.  For example, this might be used to
//   /// suppress a /.well-known/core discovery request over multicast if the filter criteria is
//   /// not met.
//   Suppress,
// }
