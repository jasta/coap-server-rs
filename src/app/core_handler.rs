use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use coap_lite::{ContentFormat, ResponseType};

use crate::app::app_builder::AppBuilder;
use crate::app::error::CoapError;
use crate::app::request_handler::RequestHandler;
use crate::app::resource_builder::DiscoverableResource;
use crate::app::{coap_utils, Request, ResourceBuilder, Response};

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
        builder
            .resources_by_path
            .insert(resource.path, resource.handler);
    }
}

#[async_trait]
impl<Endpoint: Send + 'static> RequestHandler<Endpoint> for CoreRequestHandler {
    async fn handle(&self, request: Request<Endpoint>) -> Result<Response, CoapError> {
        let queries = coap_utils::request_get_queries(&request.original);

        let mut response = request.new_response();
        response.message.payload = self
            .resources
            .iter()
            .filter(|&r| filter_by_query(r, &queries))
            .map(|k| k.link_str.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        response.set_status(ResponseType::Content);
        response
            .message
            .set_content_format(ContentFormat::ApplicationLinkFormat);
        Ok(response)
    }
}

fn filter_by_query(resource: &DiscoverableResource, queries: &HashMap<String, String>) -> bool {
    for (key, value) in queries {
        if Some(value) != resource.attributes_as_string.get(key.as_str()) {
            return false;
        }
    }
    true
}
