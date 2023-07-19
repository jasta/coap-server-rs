use alloc::string::String;
use alloc::vec::Vec;

use coap_lite::{CoapRequest, CoapResponse, RequestType, ResponseType};

use crate::app::response::Response;

/// Wrapper type for CoAP requests carrying extra context from filtering.  This API is not
/// considered stable and in particular the inclusion of the [`CoapRequest`] reference is
/// likely to be removed in future versions when matching/filtering is more robust.
#[derive(Debug, Clone)]
pub struct Request<Endpoint> {
    pub original: CoapRequest<Endpoint>,
    pub unmatched_path: Vec<String>,
}

impl<Endpoint> Request<Endpoint> {
    /// Construct a new, but incomplete, [`Response`] instance that is appropriately paired
    /// with the request.  For example, the response token is set-up to match the request.
    ///
    /// Note that after the response is returned no further associated with the request can be
    /// guaranteed and callers are free to override these values, thus potentially violating
    /// the CoAP protocol spec.
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
