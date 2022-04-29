use std::net::SocketAddr;

use coap_lite::link_format::LINK_ATTR_RESOURCE_TYPE;
use coap_server::app::{CoapError, Request, Response};
use coap_server::{app, CoapServer, FatalServerError, UdpTransport};

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    env_logger::init();
    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    server
        .serve(app::new().resource(app::resource("/").default_handler(handle_all)))
        .await
}

async fn handle_all(request: Request<SocketAddr>) -> Result<Response, CoapError> {
    let method = *request.original.get_method();
    let path = request.original.get_path();
    let peer = request.original.source.unwrap();
    let mut response = request.new_response();
    response.message.payload = format!("Received from {peer:?}: {method:?} /{path}").into_bytes();
    Ok(response)
}
