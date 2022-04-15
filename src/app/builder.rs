use std::collections::HashMap;
use std::fmt::Debug;

use crate::app::handler::AppHandler;
use crate::app::resource_builder::DiscoverableResource;
use crate::app::resource_handler::ResourceHandler;
use crate::app::ResourceBuilder;
use crate::packet_handler::IntoHandler;

pub struct AppBuilder<Endpoint> {
    pub(crate) config: ConfigBuilder,
    pub(crate) discoverable_resources: Vec<DiscoverableResource>,
    pub(crate) resources_by_path: HashMap<String, ResourceHandler<Endpoint>>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ConfigBuilder {
    pub discoverable: Option<bool>,
    pub block_transfer: Option<bool>,
}

impl<Endpoint> Default for AppBuilder<Endpoint> {
    fn default() -> Self {
        Self {
            config: ConfigBuilder::default(),
            discoverable_resources: Vec::new(),
            resources_by_path: HashMap::new(),
        }
    }
}

impl<Endpoint> AppBuilder<Endpoint> {
    pub fn new() -> Self {
        Default::default()
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

    pub fn resource(mut self, resource: ResourceBuilder<Endpoint>) -> Self {
        let built = resource.build();
        if let Some(discoverable) = built.discoverable {
            self.discoverable_resources.push(discoverable);
        }
        self.resources_by_path.insert(built.path, built.handler);
        self
    }

    pub fn resources(mut self, resources: Vec<ResourceBuilder<Endpoint>>) -> Self {
        for resource in resources {
            self = self.resource(resource);
        }
        self
    }
}

impl<Endpoint: Debug + Ord + Clone + Send + 'static> IntoHandler<AppHandler<Endpoint>, Endpoint>
    for AppBuilder<Endpoint>
{
    fn into_handler(self, mtu: Option<u32>) -> AppHandler<Endpoint> {
        AppHandler::from_builder(self, mtu)
    }
}
