use std::sync::Arc;

use async_trait::async_trait;
use coap_lite::{ContentFormat, ResponseType};

use crate::app::{Request, ResourceBuilder, Response};
use crate::app::builder::AppBuilder;
use crate::app::error::CoapError;
use crate::app::resource_builder::{DiscoverableResource, RequestHandler};

#[derive(Clone)]
pub struct CoreRequestHandler {
    resources: Arc<Vec<DiscoverableResource>>,
}

impl CoreRequestHandler {
    pub fn install<Endpoint: Send + 'static>(builder: &mut AppBuilder<Endpoint>) {
        let request_handler = Self {
            resources: Arc::new(builder.discoverable_resources.clone()),
        };
        let resource = ResourceBuilder::new("/.well-known/core")
            .get(request_handler)
            .build();
        builder.resources_by_path.insert(resource.path, resource.handler);
    }
}

#[async_trait]
impl<Endpoint: Send + 'static> RequestHandler<Endpoint> for CoreRequestHandler {
    async fn handle(&self, request: Request<Endpoint>) -> Result<Response, CoapError> {
        let mut response = request.new_response();
        response.message.payload = self.resources
            .iter()
            .map(|k| k.link_str.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        response.set_status(ResponseType::Content);
        response.message.set_content_format(ContentFormat::ApplicationLinkFormat);
        Ok(response)
    }
}
