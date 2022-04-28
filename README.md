# Robust, ergonomic Rust CoAP server

[![Build Status](https://github.com/jasta/coap-server-rs/workflows/Rust/badge.svg)](https://github.com/jasta/coap-server-rs/actions)
[![Coverage Status](https://coveralls.io/repos/github/jasta/coap-server-rs/badge.svg?branch=main)](https://coveralls.io/github/jasta/coap-server-rs?branch=main)
[![Crates.io](https://img.shields.io/crates/v/coap-server.svg)](https://crates.io/crates/coap-server)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/tokio-rs/tokio/blob/master/LICENSE)

An asynchronous [CoAP](https://coap.technology/) server with a modern and
ergonomic API for larger scale applications, inspired by warp and actix.  CoAP
offers an excellent alternative to HTTP for resource constrained environments
like IoT devices.

* **Ergonomic**: Fluent app-builder API makes it easy to compose rich
  applications, including those that use more advanced CoAP features.
* **Concurrent**: High concurrency is supported by using a separate spawned
  task for each request, allowing long running requests to not interfere with
  shorter running ones at scale.
* **Feature-rich**: Supports a wide range of CoAP server features including
  [Observe](https://datatracker.ietf.org/doc/html/rfc7641), and [Block-wise
  Transfer](https://datatracker.ietf.org/doc/html/rfc7959).
* **Flexible**: Supports pluggable transport backends with goals of supporting
  alternative async runtimes like
  [embassy](https://github.com/embassy-rs/embassy).

## Example

```rust
use coap_server::app::{CoapError, Request, Response};
use coap_server::{app, CoapServer, FatalServerError, UdpTransport};

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    server.serve(
        app::new().resource(
            app::resource("/hello").get(handle_get_hello))
    ).await
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
```

To experiment, I recommend using the excellent [coap-client](https://libcoap.net/doc/reference/develop/man_coap-client.html) command-line tool, as with:

```
$ coap-client -m get coap://localhost/hello
Hello, world
```

See [examples](https://github.com/jasta/coap-server-rs/tree/main/examples) for more.

## Features

This project aims to be a robust and complete CoAP server, and in particular a more convenient alternative to MQTT for Rust-based projects:

- [x] Correct and convenient Observe support ([RFC 7641](https://datatracker.ietf.org/doc/html/rfc7641))
- [x] Block-wise transfer support ([RFC 7959](https://datatracker.ietf.org/doc/html/rfc7959))
- [x] Resource discovery and filtering via `/.well-known/core` ([RFC 6690](https://datatracker.ietf.org/doc/html/rfc6690))
- [x] Multicast UDP
- [x] Fully concurrent request handling (no head-of-line blocking or scaling surprises!)
- [x] Ping/Pong keep-alive messages

Desired but not implemented:

- [ ] [Non-piggybacked responses](https://github.com/jasta/coap-server-rs/issues/4)
- [ ] Secure transports ([OSCORE](https://github.com/jasta/coap-server-rs/issues/5) and [DTLS](https://github.com/jasta/coap-server-rs/issues/6))

## Related Projects

- [martindisch/coap-lite](https://github.com/martindisch/coap-lite): used by
  this project as the low-level basis for CoAP protocol support
- [Covertness/coap-rs](https://github.com/Covertness/coap-rs): original server
  I used but outgrew when I needed more robust features like generic Observe
  support and `/.well-known/core` filtering.
- [ryankurte/rust-coap-client](https://raw.githubusercontent.com/ryankurte/rust-coap-client):
  inspired the creation of this crate based on the excellent generalization of
  client backends
- [google/rust-async-coap](https://github.com/google/rust-async-coap)
