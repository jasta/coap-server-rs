#[cfg(feature = "std")]
pub type IoError = std::io::Error;

#[cfg(not(feature = "std"))]
mod no_std {
  use core::fmt::{Debug, Formatter};

  /// Alternative to io::Error that integration layers can use to provide errors to the coap-server
  /// internals.
  pub enum IoError {
    Undefined,
  }

  impl Debug for IoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
      write!(f, "Undefined error!")
    }
  }
}

#[cfg(not(feature = "std"))]
pub use self::no_std::IoError;