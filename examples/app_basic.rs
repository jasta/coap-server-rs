use std::net::SocketAddr;

use coap_lite::link_format::LINK_ATTR_RESOURCE_TYPE;
use coap_server::app::{CoapError, Request, Response};
use coap_server::{app, CoapServer, FatalServerError};
use coap_server_tokio::transport::udp::UdpTransport;

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    env_logger::init();
    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    server
        .serve(
            app::new()
                .resource(
                    app::resource("/hello")
                        // Try `coap-client -m get coap://localhost/.well-known/core` to see this!
                        .link_attr(LINK_ATTR_RESOURCE_TYPE, "hello")
                        .get(handle_get_hello),
                )
                .resource(
                    app::resource("/hidden")
                        .not_discoverable()
                        .get(handle_get_hidden),
                ),
        )
        .await
}

async fn handle_get_hello(request: Request<SocketAddr>) -> Result<Response, CoapError> {
    let whom = request
        .unmatched_path
        .first()
        .cloned()
        .unwrap_or_else(|| "world".to_string());

    let mut response = request.new_response();
    response.message.payload = format!("Hello, {whom}").into_bytes();
    Ok(response)
}

async fn handle_get_hidden(request: Request<SocketAddr>) -> Result<Response, CoapError> {
    let mut response = request.new_response();
    response.message.payload = b"sshh!".to_vec();
    Ok(response)
}
