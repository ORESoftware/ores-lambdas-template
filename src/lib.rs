//! __PREFIX__-lambdas — provider-neutral function core.
//!
//! `runtime` is the only command-envelope validation and dispatch authority.
//! Browser-page HTTP hosting is a separate narrow surface under `page_http` and
//! never turns a page into a command or RPC operation.
#![forbid(unsafe_code)]

pub mod adapters;
#[cfg(feature = "page")]
pub mod page_http;
pub mod runtime;

pub use runtime::{
    dispatch, Invocation, Operation, Provider, Receipt, MAX_INVOCATION_BYTES, SCHEMA_VERSION,
};

#[cfg(feature = "page")]
pub use page_http::{
    invoke_page, IngressProvenance, PageHttpMethod, PageHttpRequest, PageHttpResponse, RuntimeError,
    MAX_PAGE_BODY_BYTES,
};
#[cfg(feature = "page-aws")]
pub use page_http::aws;
#[cfg(feature = "page-gcp")]
pub use page_http::gcp;
