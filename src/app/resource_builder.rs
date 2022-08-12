use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use core::fmt::Debug;
use core::hash::Hash;
use hashbrown::HashMap;

#[cfg(feature = "observable")]
use coap_lite::link_format::LINK_ATTR_OBSERVABLE;
use coap_lite::RequestType;
#[cfg(feature = "embassy")]
use embassy_util::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
#[cfg(feature = "tokio")]
use tokio::sync::Mutex;

use crate::app::app_builder::ConfigBuilder;
use crate::app::block_handler_util::new_block_handler;
use crate::app::core_link::{CoreLink, LinkAttributeValue};
use crate::app::request_handler::RequestHandler;
use crate::app::request_type_key::RequestTypeKey;
use crate::app::resource_handler::ResourceHandler;
use crate::app::retransmission_manager::RetransmissionManager;
#[cfg(feature = "observable")]
use crate::app::{observable_resource::ObservableResource, observe_handler::ObserveHandler};

/// Configure a specific resource handler, potentially with distinct per-method handlers.
pub struct ResourceBuilder<Endpoint: Ord + Clone> {
    path: String,
    config: ConfigBuilder,
    attributes: CoreLink,
    handlers: HashMap<RequestTypeKey, Box<dyn RequestHandler<Endpoint> + Send + Sync>>,
    #[cfg(feature = "observable")]
    observable: Option<Box<dyn ObservableResource + Send + Sync>>,
}

impl<Endpoint: Debug + Clone + Eq + Hash + Ord + Send + 'static> ResourceBuilder<Endpoint> {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            config: ConfigBuilder::default(),
            attributes: CoreLink::new(path),
            handlers: HashMap::new(),
            #[cfg(feature = "observable")]
            observable: None,
        }
    }

    /// See [`crate::app::AppBuilder::not_discoverable`].
    pub fn not_discoverable(mut self) -> Self {
        self.config.discoverable = Some(false);
        self
    }

    /// See [`crate::app::AppBuilder::enable_block_transfer`].
    pub fn enable_block_transfer(mut self) -> Self {
        self.config.block_transfer = Some(true);
        self
    }

    /// See [`crate::app::AppBuilder::disable_block_transfer`].
    pub fn disable_block_transfer(mut self) -> Self {
        self.config.block_transfer = Some(false);
        self
    }

    /// Add a new attribute to the CoRE Link response, assuming that this resource will be
    /// discoverable.  For more information, see
    /// [RFC 5785](https://datatracker.ietf.org/doc/html/rfc5785)
    pub fn link_attr(
        mut self,
        attr_name: &'static str,
        value: impl LinkAttributeValue<String> + 'static,
    ) -> Self {
        self.attributes.attr(attr_name, value);
        self
    }

    /// Set a default catch-all handler if another more specific request handler does not match.
    /// This can be useful if you wish to override the default behaviour of responding with
    /// "4.05 Method not allowed".
    pub fn default_handler(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestTypeKey::new_match_all(), handler)
    }

    /// Set a request handler for "Get" requests.
    pub fn get(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Get.into(), handler)
    }

    /// Set a request handler for "Post" requests.
    pub fn post(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Post.into(), handler)
    }

    /// Set a request handler for "Put" requests.
    pub fn put(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Put.into(), handler)
    }

    /// Set a request handler for "Delete" requests.
    pub fn delete(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Delete.into(), handler)
    }

    /// Set a request handler for "Fetch" requests.
    pub fn fetch(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Fetch.into(), handler)
    }

    /// Set a request handler for "Patch" requests.
    pub fn patch(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::Patch.into(), handler)
    }

    /// Set a request handler for "iPatch" requests.
    pub fn ipatch(self, handler: impl RequestHandler<Endpoint> + Send + Sync) -> Self {
        self.handler(RequestType::IPatch.into(), handler)
    }

    /// Set a request handler for arbitrary request types.  This method is provided to enable
    /// convenient dynamic building of resource handlers, but is not preferred for most
    /// applications.
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

    /// Enable Observe support for Get and/or Fetch requests in this resource.  An `observable`
    /// argument is required to usefully take advantage of this support which requires
    /// callers to invoke [`crate::app::Observers::notify_change`] in order to trigger
    /// updates to be delivered to registered observers.
    ///
    /// For more information, see [RFC 7641](https://datatracker.ietf.org/doc/html/rfc7641)
    #[cfg(feature = "observable")]
    pub fn observable(mut self, observable: impl ObservableResource + Send + Sync) -> Self {
        self.observable = Some(Box::new(observable));
        self.link_attr(LINK_ATTR_OBSERVABLE, ())
    }

    #[cfg(feature = "observable")]
    pub(crate) fn build(self, params: BuildParameters<Endpoint>) -> Resource<Endpoint> {
        let discoverable = if self.config.discoverable.unwrap_or(true) {
            Some(DiscoverableResource::from(self.attributes))
        } else {
            None
        };

        let observe_handler = self
            .observable
            .map(|x| Arc::new(Mutex::new(ObserveHandler::new(self.path.clone(), x))));

        let block_transfer = self
            .config
            .block_transfer
            .unwrap_or(crate::app::app_handler::DEFAULT_BLOCK_TRANSFER);
        let block_handler = if block_transfer {
            Some(Arc::new(Mutex::new(new_block_handler(params.mtu))))
        } else {
            None
        };

        let handler = ResourceHandler {
            handlers: self.handlers,
            observe_handler,
            block_handler,
            retransmission_manager: params.retransmission_manager,
        };
        Resource {
            path: self.path,
            discoverable,
            handler,
        }
    }

    #[cfg(not(feature = "observable"))]
    pub(crate) fn build(self, params: BuildParameters<Endpoint>) -> Resource<Endpoint> {
        let discoverable = if self.config.discoverable.unwrap_or(true) {
            Some(DiscoverableResource::from(self.attributes))
        } else {
            None
        };

        let block_transfer = self
            .config
            .block_transfer
            .unwrap_or(crate::app::app_handler::DEFAULT_BLOCK_TRANSFER);
        let block_handler = if block_transfer {
            Some(Arc::new(Mutex::new(new_block_handler(params.mtu))))
        } else {
            None
        };

        let handler = ResourceHandler {
            handlers: self.handlers,
            block_handler,
            retransmission_manager: params.retransmission_manager,
        };
        Resource {
            path: self.path,
            discoverable,
            handler,
        }
    }
}

#[derive(Clone)]
pub(crate) struct BuildParameters<Endpoint: Debug + Clone + Eq + Hash> {
    pub mtu: Option<u32>,
    #[cfg(feature = "tokio")]
    pub retransmission_manager: Arc<Mutex<RetransmissionManager<Endpoint>>>,
    #[cfg(feature = "embassy")]
    pub retransmission_manager:
        Arc<Mutex<CriticalSectionRawMutex, RetransmissionManager<Endpoint>>>,
}

#[derive(Clone)]
pub(crate) struct DiscoverableResource {
    /// Preformatted individual link str so that we can just join them all by "," to form
    /// the `/.well-known/core` response.  This prevents us from having to deal with
    /// whacky lifetime issues and improves performance of the core query.
    pub link_str: String,

    /// Attributes converted to Strings that can be efficiently compared with query parameters
    /// we might get in a filter request like `GET /.well-known/core?rt=foo`
    pub attributes_as_string: HashMap<&'static str, String>,
}

pub(crate) struct Resource<Endpoint: Debug + Clone + Ord + Eq + Hash> {
    pub path: String,
    pub discoverable: Option<DiscoverableResource>,
    pub handler: ResourceHandler<Endpoint>,
}
