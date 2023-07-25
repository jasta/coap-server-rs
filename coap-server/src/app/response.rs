use coap_lite::CoapResponse;

/// Response type that for now is just aliasing to the lower level `coap_lite` version.  Future
/// versions are likely to see this API encapsulated more carefully.
pub type Response = CoapResponse;
