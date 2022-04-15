use std::mem;
use std::net::SocketAddr;
use std::sync::Arc;

use coap_lite::link_format::LINK_ATTR_RESOURCE_TYPE;
use tokio::sync::Mutex;

use coap_server::app::AppBuilder;
use coap_server::app::CoapError;
use coap_server::app::{ObservableResource, Observers};
use coap_server::app::Request;
use coap_server::app::Response;
use coap_server::FatalServerError;
use coap_server::{app, CoapServer, UdpTransport};

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    env_logger::init();
    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    server.serve(build_router()).await
}

fn build_router() -> AppBuilder<SocketAddr> {
    let counter_state = CounterState::default();
    let state_for_get = counter_state.clone();
    let state_for_put = counter_state.clone();
    app::new()
        .resource(
            app::resource("/counter")
                .link_attr(LINK_ATTR_RESOURCE_TYPE, "counter")
                .observable(counter_state)
                .get(move |req| handle_get_counter(req, state_for_get.clone())),
        )
        .resource(
            app::resource("/counter/inc")
                .put(move |req| handle_put_counter_inc(req, state_for_put.clone())),
        )
}

#[derive(Clone)]
struct CounterState {
    counter: Arc<Mutex<u32>>,
    observers: Option<Observers>,
}

impl Default for CounterState {
    fn default() -> Self {
        Self {
            counter: Arc::new(Mutex::new(0)),
            observers: None,
        }
    }
}

impl ObservableResource for CounterState {
    fn on_first_observer(&mut self, observers: Observers) {
        self.observers = Some(observers);
    }

    fn on_last_observer(&mut self) -> Observers {
        mem::take(&mut self.observers).unwrap()
    }
}

async fn handle_get_counter(
    request: Request<SocketAddr>,
    state: CounterState,
) -> Result<Response, CoapError> {
    let count = *state.counter.lock().await;
    let mut response = request.new_response();
    response.message.payload = format!("{count}").into_bytes();
    Ok(response)
}

async fn handle_put_counter_inc(
    request: Request<SocketAddr>,
    state: CounterState,
) -> Result<Response, CoapError> {
    {
        let mut count = state.counter.lock().await;
        *count += 1;
    }
    if let Some(observers) = state.observers {
        observers.notify_change();
    }
    Ok(request.new_response())
}
