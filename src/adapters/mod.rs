//! Thin provider adapters. Each converts one invocation shape into `runtime::handle` and back.
#[cfg(feature = "aws")]
pub mod aws;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "portable")]
pub mod portable;
