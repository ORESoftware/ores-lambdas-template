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

#[cfg(feature = "page-aws")]
pub use page_http::aws;
#[cfg(feature = "page-gcp")]
pub use page_http::gcp;
#[cfg(feature = "page")]
pub use page_http::{
    admit_page_request, finish_page_response, IngressProvenance, PageHttpMethod, PageHttpRequest,
    PageHttpResponse, PageInvocation, RuntimeError, MAX_PAGE_BODY_BYTES,
};

/// Compatibility façade consumed by the executable sibling `lambda.rs` emitted
/// by api-docs. All request admission and response-envelope hardening stays in
/// `page_http`; this function only connects the generated typed page/finalizer
/// trampolines to those hardened primitives.
#[cfg(feature = "page")]
pub async fn invoke_page(
    request: PageHttpRequest,
    state: ores_api_docs_client::PageState,
    _canonical_route: &'static str,
    axum_paths: &'static [&'static str],
    page: ores_api_docs_client::PageFn,
    finalize: ores_api_docs_client::PageFinalizeFn,
) -> Result<PageHttpResponse, RuntimeError> {
    let invocation = match admit_page_request(request, state, axum_paths) {
        Ok(invocation) => invocation,
        Err(response) => return Ok(response),
    };
    let PageInvocation {
        context,
        wasm_have,
        head,
    } = invocation;
    let rendered = page(context).await;
    let finalized = finalize(
        rendered,
        ores_api_docs_client::PageResponseRequestHints {
            wasm_have: wasm_have.as_deref(),
            dev_reload_script: None,
        },
    );
    Ok(finish_page_response(finalized, head))
}
