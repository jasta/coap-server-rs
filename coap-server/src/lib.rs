//! Robust, ergonomic CoAP server in Rust.
//!
//! # Examples
//! ```no_run
//! use std::net::SocketAddr;
//!
//! use coap_server::app::{CoapError, Request, Response};
//! use coap_server::{app, CoapServer, FatalServerError, UdpTransport};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), FatalServerError> {
//!     let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
//!     server.serve(
//!         app::new().resource(
//!             app::resource("/hello").get(handle_get_hello))
//!     ).await
//! }
//!
//! async fn handle_get_hello(request: Request<SocketAddr>) -> Result<Response, CoapError> {
//!     let whom = request
//!         .unmatched_path
//!         .first()
//!         .cloned()
//!         .unwrap_or_else(|| "world".to_string());
//!
//!     let mut response = request.new_response();
//!     response.message.payload = format!("Hello, {whom}").into_bytes();
//!     Ok(response)
//! }
//! ```
//!
//! See other [examples](https://github.com/jasta/coap-server-rs/tree/main/examples) for more information.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate core;
extern crate alloc;

pub use server::CoapServer;
pub use server::FatalServerError;
#[cfg(feature = "std")]
pub use udp::UdpTransport;

pub mod app;
pub mod packet_handler;
pub mod server;
pub mod transport;
#[cfg(feature = "std")]
pub mod udp;
pub mod io_error;
