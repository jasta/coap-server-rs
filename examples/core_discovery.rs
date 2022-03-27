use coap_server::packet_handler::PacketHandler;
use coap_server::server::FatalServerError;
use coap_server::{CoapServer, UdpTransport};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    server.serve(build_service()).await
}

fn build_service() -> impl PacketHandler<SocketAddr> {
    todo!()
}
