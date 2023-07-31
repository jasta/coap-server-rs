#[cfg(feature = "std")]
pub(crate) use thiserror::Error as ThisError;

#[cfg(not(feature = "std"))]
pub(crate) use thiserror_no_std::Error as ThisError;
