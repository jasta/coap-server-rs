pub use app_builder::AppBuilder;
use core::fmt::Debug;
use core::hash::Hash;
pub use error::CoapError;
#[cfg(feature = "observable")]
pub use observable_resource::ObservableResource;
#[cfg(feature = "observable")]
pub use observers::Observers;
#[cfg(feature = "observable")]
pub use observers::ObserversHolder;
pub use request::Request;
pub use resource_builder::ResourceBuilder;
pub use response::Response;

pub mod app_builder;
pub mod app_handler;
mod block_handler_util;
mod coap_utils;
mod core_handler;
mod core_link;
pub mod error;
#[cfg(feature = "observable")]
pub mod observable_resource;
#[cfg(feature = "observable")]
mod observe_handler;
#[cfg(feature = "observable")]
mod observers;
mod path_matcher;
pub mod request;
pub mod request_handler;
mod request_type_key;
pub mod resource_builder;
mod resource_handler;
pub mod response;
mod retransmission_manager;
mod u24;

/// Main starting point to build a robust CoAP-based application
pub fn new<Endpoint: Debug + Clone + Ord + Eq + Hash>() -> AppBuilder<Endpoint> {
    AppBuilder::new()
}

/// Start builder a new resource handler
pub fn resource<Endpoint: Debug + Clone + Ord + Eq + Hash + Send + 'static>(
    path: &str,
) -> ResourceBuilder<Endpoint> {
    ResourceBuilder::new(path)
}
