use core::fmt;
use core::fmt::Debug;

use alloc::string::{String, ToString};
use coap_lite::error::HandlingError;
use coap_lite::ResponseType;

/// Error type which can be converted to a [`crate::app::Response`] as a convenience for allowing
/// Rust's `?` operator to work naturally in handler code without violating the protocol by
/// failing to respond to requests.
#[derive(Debug, Clone)]
pub struct CoapError {
    pub code: Option<ResponseType>,
    pub message: String,
}

impl CoapError {
    pub fn internal(msg: impl ToString) -> Self {
        Self::for_code(ResponseType::InternalServerError, msg)
    }

    pub fn bad_request(msg: impl ToString) -> Self {
        Self::for_code(ResponseType::BadRequest, msg)
    }

    pub fn not_found() -> Self {
        Self::for_code(ResponseType::NotFound, "Not found")
    }

    pub fn method_not_allowed() -> Self {
        Self::for_code(ResponseType::MethodNotAllowed, "Method not allowed")
    }

    pub fn for_code(code: ResponseType, msg: impl ToString) -> Self {
        Self {
            code: Some(code),
            message: msg.to_string(),
        }
    }

    pub(crate) fn into_handling_error(self) -> HandlingError {
        HandlingError {
            code: self.code,
            message: self.message,
        }
    }
}

impl fmt::Display for CoapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handling error {:?}: {}", self.code, self.message)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CoapError {}

impl From<HandlingError> for CoapError {
    fn from(src: HandlingError) -> Self {
        Self {
            message: src.message,
            code: src.code,
        }
    }
}
