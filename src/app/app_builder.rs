use core::fmt::Debug;
use core::hash::Hash;

use alloc::vec::Vec;
use rand::Rng;

use crate::app::app_handler::AppHandler;
use crate::app::ResourceBuilder;
use crate::packet_handler::IntoHandler;

/// Main builder API to configure how the CoAP server should respond to requests
pub struct AppBuilder<Endpoint: Ord + Clone> {
    pub(crate) config: ConfigBuilder,
    pub(crate) resources: Vec<ResourceBuilder<Endpoint>>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ConfigBuilder {
    pub discoverable: Option<bool>,
    pub block_transfer: Option<bool>,
}

impl<Endpoint: Ord + Clone> Default for AppBuilder<Endpoint> {
    fn default() -> Self {
        Self {
            config: ConfigBuilder::default(),
            resources: Vec::new(),
        }
    }
}

impl<Endpoint: Ord + Clone> AppBuilder<Endpoint> {
    pub fn new() -> Self {
        Default::default()
    }

    /// Enable resource discovery by default for all resources in the app.  To disable this
    /// on a per-resource level, see [`ResourceBuilder::not_discoverable`].
    ///
    /// For more information refer to [RFC 5785](https://datatracker.ietf.org/doc/html/rfc5785).
    pub fn discoverable(mut self) -> Self {
        self.config.discoverable = Some(true);
        self
    }

    /// Disable resource discovery by default for all resources in the app.  This can be
    /// overridden on a per-resource level.
    ///
    /// See [`AppBuilder::discoverable`].
    pub fn not_discoverable(mut self) -> Self {
        self.config.discoverable = Some(false);
        self
    }

    /// Enable block-wise transfer by default for all resources in the app.
    ///
    /// Block-wise transfer is defaulted to a transparent handler that buffers in memory large
    /// requests or responses as they are being transferred from/to the peer, expiring after
    /// some time if the client does not gracefully exhaust the payload (e.g. if the client
    /// downloads only a portion of a large response then goes away).
    ///
    /// To disable this completely on a per-resource level, see
    /// [`ResourceBuilder::disable_block_transfer`].  Alternatively you may implement a request
    /// handler that responds with Block2 option values that will cause the transparent handler to
    /// defer the handling to your custom logic.
    ///
    /// For more information refer to [RFC 7959](https://datatracker.ietf.org/doc/html/rfc7959).
    pub fn block_transfer(mut self) -> Self {
        self.config.block_transfer = Some(true);
        self
    }

    /// Disable block-wise transfer by default for all resources in the app.  This can be overriden
    /// on a per-resource level.
    ///
    /// See [`AppBuilder::block_transfer`].
    pub fn disable_block_transfer(mut self) -> Self {
        self.config.block_transfer = Some(false);
        self
    }

    /// Add a resource handler to the app by the configured path.  See [`crate::app::resource`]
    /// to start building one.
    pub fn resource(mut self, resource: ResourceBuilder<Endpoint>) -> Self {
        self.resources.push(resource);
        self
    }

    /// Convenience method to add multiple resources at once.
    pub fn resources(mut self, resources: Vec<ResourceBuilder<Endpoint>>) -> Self {
        for resource in resources {
            self = self.resource(resource);
        }
        self
    }
}

impl<
        Endpoint: Debug + Clone + Ord + Eq + Hash + Send + 'static,
        R: Rng + Send + Sync + Clone + 'static,
    > IntoHandler<AppHandler<Endpoint, R>, Endpoint, R> for AppBuilder<Endpoint>
{
    fn into_handler(self, mtu: Option<u32>, rng: R) -> AppHandler<Endpoint, R> {
        AppHandler::from_builder(self, mtu, rng)
    }
}
