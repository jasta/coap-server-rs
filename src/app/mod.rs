use builder::AppBuilder;
pub use request::Request;
pub use resource_builder::ResourceBuilder;
pub use response::Response;

pub mod builder;
mod core_handler;
mod core_link;
pub mod error;
pub mod handler;
pub mod observe;
mod path_matcher;
pub mod request;
mod request_type_key;
pub mod resource_builder;
pub mod response;

pub fn new<Endpoint>() -> AppBuilder<Endpoint> {
    AppBuilder::new()
}

pub fn resource<Endpoint>(path: &str) -> ResourceBuilder<Endpoint> {
    ResourceBuilder::new(path)
}
