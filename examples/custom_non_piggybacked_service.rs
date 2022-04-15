use std::net::SocketAddr;
use std::pin::Pin;
use std::time::{Duration, Instant};

use coap_lite::ResponseType::Content;
use coap_lite::{MessageClass, MessageType, Packet};
use futures::Stream;
use log::info;

use coap_server::packet_handler::PacketHandler;
use coap_server::server::FatalServerError;
use coap_server::{CoapServer, UdpTransport};

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    env_logger::init();

    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    info!("Server is bound to 0.0.0.0:5683!");
    server.serve(NonPiggybackedService::default()).await
}

/// Highly customized service that manually implements advanced features like not using
/// a piggy backed response + ack in order to allow very slow running responses to not
/// confuse clients.  These features will eventually make their way into the app-based
/// approach (if they aren't already there!), but this provides a good example of what is
/// possible with the lowest level API.
#[derive(Default, Clone)]
struct NonPiggybackedService {}

impl NonPiggybackedService {
    fn should_process(&self, request: &Packet) -> bool {
        match request.header.get_type() {
            MessageType::Confirmable => true,
            MessageType::NonConfirmable => true,
            _ => false,
        }
    }

    fn create_ack(&self, request: &Packet) -> Packet {
        let mut packet = Packet::new();

        packet.header.set_version(1);
        packet.header.set_type(MessageType::Acknowledgement);
        packet.header.message_id = request.header.message_id;
        packet.header.code = MessageClass::Empty;
        packet.set_token(request.get_token().to_vec());

        packet
    }

    fn create_response(&self, request: &Packet, elapsed: Duration) -> Packet {
        let mut packet = Packet::new();

        packet.header.set_version(1);
        packet.header.set_type(MessageType::NonConfirmable);
        packet.header.message_id = request.header.message_id;
        packet.header.code = MessageClass::Response(Content);
        packet.set_token(request.get_token().to_vec());

        let elapsed_ms = elapsed.as_millis();
        packet.payload = format!("Response available in {elapsed_ms} ms").into_bytes();

        packet
    }
}

impl PacketHandler<SocketAddr> for NonPiggybackedService {
    fn handle(
        &self,
        packet: Packet,
        peer: SocketAddr,
    ) -> Pin<Box<dyn Stream<Item = Packet> + Send + '_>> {
        let stream = async_stream::stream! {
          if self.should_process(&packet) {
            let start = Instant::now();
            yield self.create_ack(&packet);
            tokio::time::sleep(Duration::from_secs(4)).await;
            yield self.create_response(&packet, start.elapsed());
          }
        };
        Box::pin(stream)
    }
}
