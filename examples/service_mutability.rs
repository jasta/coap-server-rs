use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use coap_lite::{CoapResponse, Packet};
use futures::Stream;
use log::info;
use tokio::sync::Mutex;

use coap_server::packet_handler::PacketHandler;
use coap_server::server::FatalServerError;
use coap_server::{CoapServer, UdpTransport};

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    env_logger::init();

    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    info!("Server is bound to 0.0.0.0:5683!");
    server.serve(CountingService::default()).await
}

#[derive(Clone)]
struct CountingService {
    counter: Arc<Mutex<u32>>,
}

impl Default for CountingService {
    fn default() -> Self {
        Self {
            counter: Arc::new(Mutex::new(0)),
        }
    }
}

impl PacketHandler<SocketAddr> for CountingService {
    fn handle(
        &self,
        packet: Packet,
        peer: SocketAddr,
    ) -> Pin<Box<dyn Stream<Item = Packet> + Send + '_>> {
        let stream = async_stream::stream! {
          if let Some(mut response) = CoapResponse::new(&packet) {
            let count = {
              let mut counter = self.counter.lock().await;
              *counter += 1;
              *counter
            };
            response.message.payload = format!("Hello {peer}, count is now {count}").into_bytes();
            yield response.message;
          }
        };
        Box::pin(stream)
    }
}
