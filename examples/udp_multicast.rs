use std::net::SocketAddr;

use coap_lite::link_format::LINK_ATTR_RESOURCE_TYPE;
use coap_server::app::{CoapError, Request, Response};
use coap_server::{app, CoapServer, FatalServerError, UdpTransport};

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    env_logger::init();

    // Try `coap-client -B 5 -N -m get coap://224.0.1.187/.well-known/core`.
    let transport = UdpTransport::new("0.0.0.0:5683").enable_multicast();
    let server = CoapServer::bind(transport).await?;
    server
        .serve(
            app::new().resource(
                app::resource("/hello")
                    .link_attr(LINK_ATTR_RESOURCE_TYPE, "hello")
                    .get(handle_get_hello),
            ),
        )
        .await
}

async fn handle_get_hello(request: Request<SocketAddr>) -> Result<Response, CoapError> {
    let mut response = request.new_response();
    response.message.payload = b"Hello.".to_vec();
    Ok(response)
}
