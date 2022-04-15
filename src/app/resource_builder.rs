use std::collections::HashMap;

use coap_lite::link_format::LINK_ATTR_OBSERVABLE;
use coap_lite::RequestType;

use crate::app::app_builder::ConfigBuilder;
use crate::app::core_link::{CoreLink, LinkAttributeValue};
use crate::app::observe::ObservableResource;
use crate::app::request_handler::RequestHandler;
use crate::app::request_type_key::RequestTypeKey;
use crate::app::resource_handler::ResourceHandler;

pub struct ResourceBuilder<Endpoint> {
    path: String,
    config: ConfigBuilder,
    attributes: CoreLink,
    handlers: HashMap<RequestTypeKey, Box<dyn RequestHandler<Endpoint> + Send + Sync>>,
    observable: Option<Box<dyn ObservableResource + Send + Sync>>,
}

impl<Endpoint> ResourceBuilder<Endpoint> {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            config: ConfigBuilder::default(),
            attributes: CoreLink::new(path),
            handlers: HashMap::new(),
            observable: None,
        }
    }

    pub fn discoverable(mut self) -> Self {
        self.config.discoverable = Some(true);
        self
    }

    pub fn not_discoverable(mut self) -> Self {
        self.config.discoverable = Some(false);
        self
    }

    pub fn block_transfer(mut self) -> Self {
        self.config.block_transfer = Some(true);
        self
    }

    pub fn disable_block_transfer(mut self) -> Self {
        self.config.block_transfer = Some(false);
        self
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
        self.handler(RequestTypeKey::new_match_all(), handler)
    }

    pub fn get(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Get.into(), handler)
    }

    pub fn post(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Post.into(), handler)
    }

    pub fn put(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Put.into(), handler)
    }

    pub fn delete(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Delete.into(), handler)
    }

    pub fn method_handler(
        self,
        request_type: RequestType,
        handler: impl RequestHandler<Endpoint> + Send + Sync,
    ) -> Self {
        self.handler(request_type.into(), handler)
    }

    fn handler(
        mut self,
        key: RequestTypeKey,
        handler: impl RequestHandler<Endpoint> + Send + Sync,
    ) -> Self {
        self.handlers.insert(key, Box::new(handler));
        self
    }

    pub fn observable(mut self, observable: impl ObservableResource + Send + Sync) -> Self {
        self.observable = Some(Box::new(observable));
        self.link_attr(LINK_ATTR_OBSERVABLE, ())
    }

    pub(crate) fn build(self) -> Resource<Endpoint> {
        let discoverable = if self.config.discoverable.unwrap_or(true) {
            Some(DiscoverableResource::from(self.attributes))
        } else {
            None
        };
        let handler = ResourceHandler {
            handlers: self.handlers,
            observable: self.observable,
            disable_block_transfer: self.config.block_transfer.unwrap_or(true),
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

pub(crate) struct Resource<Endpoint> {
    pub path: String,
    pub discoverable: Option<DiscoverableResource>,
    pub handler: ResourceHandler<Endpoint>,
}

dyn_clone::clone_trait_object!(<Endpoint> RequestHandler<Endpoint>);
