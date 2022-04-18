use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use coap_lite::link_format::LINK_ATTR_RESOURCE_TYPE;
use log::info;
use tokio::sync::{oneshot, Mutex};
use tokio::time;

use coap_server::app::ObservableResource;
use coap_server::app::{AppBuilder, CoapError, Observers, ObserversHolder, Request, Response};
use coap_server::FatalServerError;
use coap_server::{app, CoapServer, UdpTransport};

#[tokio::main]
async fn main() -> Result<(), FatalServerError> {
    env_logger::init();
    let server = CoapServer::bind(UdpTransport::new("0.0.0.0:5683")).await?;
    server.serve(build_app()).await
}

fn build_app() -> AppBuilder<SocketAddr> {
    let counter_state = CounterState::default();
    let state_for_get = counter_state.clone();
    let state_for_put = counter_state.clone();
    app::new()
        .resource(
            app::resource("/counter")
                .link_attr(LINK_ATTR_RESOURCE_TYPE, "counter")
                // Try `coap-client -s 10 -m get coap://localhost/counter`.  You can also
                // in parallel run `coap-client -m put coap://localhost/counter/inc` to show the
                // values increment in response to user behaviour.
                .observable(counter_state)
                .get(move |req| handle_get_counter(req, state_for_get.clone())),
        )
        .resource(
            app::resource("/counter/inc")
                .put(move |req| handle_put_counter_inc(req, state_for_put.clone())),
        )
}

#[derive(Default, Clone)]
struct CounterState {
    counter: Arc<Mutex<u32>>,
    observers: ObserversHolder,
}

#[async_trait]
impl ObservableResource for CounterState {
    async fn on_active(&self, observers: Observers) -> Observers {
        info!("Observe active...");
        self.observers.attach(observers).await;
        let (tx, mut rx) = oneshot::channel();
        let counter = self.counter.clone();
        let observers = self.observers.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = &mut rx => {
                       return
                    }
                    _ = interval.tick() => {
                        *counter.lock().await += 1;
                        observers.notify_change().await;
                    }
                }
            }
        });
        self.observers.stay_active().await;
        tx.send(()).unwrap();
        info!("Observe no longer active!");
        self.observers.detach().await
    }
}

async fn handle_get_counter(
    request: Request<SocketAddr>,
    state: CounterState,
) -> Result<Response, CoapError> {
    let count = *state.counter.lock().await;
    let mut response = request.new_response();
    response.message.payload = format!("{count}\n").into_bytes();
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
    state.observers.notify_change().await;
    Ok(request.new_response())
}
