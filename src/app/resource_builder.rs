use crate::app::core_link::{CoreLink, LinkAttributeValue};
use crate::app::error::CoapError;
use crate::app::observe::ObservableResource;
use crate::app::request::Request;
use crate::app::request_type_key::RequestTypeKey;
use crate::app::response::Response;
use async_trait::async_trait;
use coap_lite::link_format::LINK_ATTR_OBSERVABLE;
use coap_lite::RequestType;
use dyn_clone::DynClone;
use std::collections::HashMap;
use std::future::Future;

impl<Endpoint> ResourceBuilder<Endpoint> {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            attributes: CoreLink::new(path),
            handlers: HashMap::new(),
            observable: None,
        }
    }

    pub fn link_attr(
        mut self,
        attr_name: &'static str,
        value: impl LinkAttributeValue<String> + 'static,
    ) -> Self {
        self.attributes.attr(attr_name, value);
        self
    }

    pub fn default_handler(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::UnKnown, handler)
    }

    pub fn get(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Get, handler)
    }

    pub fn put(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Put, handler)
    }

    fn handler(
        mut self,
        request_type: RequestType,
        handler: impl RequestHandler<Endpoint> + Send + Sync,
    ) -> Self {
        self.handlers.insert(request_type.into(), Box::new(handler));
        self
    }

    pub fn observable(mut self, observable: impl ObservableResource + Send + Sync) -> Self {
        self.observable = Some(Box::new(observable));
        self.link_attr(LINK_ATTR_OBSERVABLE, ())
    }

    pub(crate) fn build(self) -> Resource<Endpoint> {
        let discoverable = DiscoverableResource::from(self.attributes);
        let handler = ResourceHandler {
            handlers: self.handlers,
            observable: self.observable,
        };
        Resource {
            path: self.path,
            discoverable,
            handler,
        }
    }
}

#[derive(Clone)]
pub(crate) struct DiscoverableResource {
    /// Preformatted individual link str so that we can just join them all by "," to form
    /// the `/.well-known/core` response.  This prevents us from having to deal with
    /// whacky lifetime issues and improves performance of the core query.
    pub link_str: String,

    /// Attributes converted to Strings that can be efficiently compared with query parameters
    /// we might get in a filter request like `GET /.well-known/core?rt=foo`
    pub attributes_as_string: Vec<(&'static str, String)>,
}

pub struct ResourceBuilder<Endpoint> {
    path: String,
    attributes: CoreLink,
    handlers: HashMap<RequestTypeKey, Box<dyn RequestHandler<Endpoint> + Send + Sync>>,
    observable: Option<Box<dyn ObservableResource + Send + Sync>>,
}

pub(crate) struct Resource<Endpoint> {
    pub path: String,
    pub discoverable: DiscoverableResource,
    pub handler: ResourceHandler<Endpoint>,
}

#[async_trait]
pub trait RequestHandler<Endpoint>: DynClone + 'static {
    async fn handle(&self, request: Request<Endpoint>) -> Result<Response, CoapError>;
}

#[async_trait]
impl<Endpoint, F, R> RequestHandler<Endpoint> for F
where
    Endpoint: Send + Sync + 'static,
    F: Fn(Request<Endpoint>) -> R + Sync + Send + Clone + 'static,
    R: Future<Output = Result<Response, CoapError>> + Send,
{
    async fn handle(&self, request: Request<Endpoint>) -> Result<Response, CoapError> {
        (self)(request).await
    }
}

pub(crate) struct ResourceHandler<Endpoint> {
    pub handlers: HashMap<RequestTypeKey, Box<dyn RequestHandler<Endpoint> + Send + Sync>>,
    pub observable: Option<Box<dyn ObservableResource + Send + Sync>>,
}

impl<Endpoint> Clone for ResourceHandler<Endpoint> {
    fn clone(&self) -> Self {
        let handlers: HashMap<_, _> = self
            .handlers
            .iter()
            .map(|(req_type, handler)| (*req_type, dyn_clone::clone_box(handler.as_ref())))
            .collect();
        let observable = self
            .observable
            .as_ref()
            .map(|x| dyn_clone::clone_box(x.as_ref()));
        Self {
            handlers,
            observable,
        }
    }
}

dyn_clone::clone_trait_object!(<Endpoint> RequestHandler<Endpoint>);
