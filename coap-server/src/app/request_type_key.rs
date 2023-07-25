use coap_lite::RequestType;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestTypeKey(usize);

impl From<RequestType> for RequestTypeKey {
    fn from(t: RequestType) -> Self {
        Self(match t {
            RequestType::Get => 1,
            RequestType::Post => 2,
            RequestType::Put => 3,
            RequestType::Delete => 4,
            RequestType::Fetch => 5,
            RequestType::Patch => 6,
            RequestType::IPatch => 7,
            _ => 0,
        })
    }
}

impl RequestTypeKey {
    pub fn new_match_all() -> Self {
        Self(0)
    }
}
