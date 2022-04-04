use coap_lite::{CoapRequest, CoapResponse, RequestType, ResponseType};

use crate::app::response::Response;

#[derive(Debug, Clone)]
pub struct Request<Endpoint> {
    pub original: CoapRequest<Endpoint>,
    pub unmatched_path: Vec<String>,
}

impl<Endpoint> Request<Endpoint> {
    pub fn new_response(&self) -> Response {
        let mut response = CoapResponse::new(&self.original.message)
            .expect("cannot get a Request without an input message that can make a Response!");
        response.message.payload = Vec::new();
        let default_code = match &self.original.get_method() {
            RequestType::Get => ResponseType::Content,
            RequestType::Post => ResponseType::Created,
            RequestType::Put => ResponseType::Changed,
            // Err, I'm actually not sure what these should be yet, need to check the RFC!
            _ => ResponseType::Valid,
        };
        response.set_status(default_code);
        response
    }
}
