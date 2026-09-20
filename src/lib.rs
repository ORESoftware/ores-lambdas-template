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

/// Execute one provider-normalized browser-page request through the shared
/// product admission ABI before generated page code is callable.
///
/// The provider adapter owns bounded request normalization. `admit_page_request`
/// then validates the method, body, route pattern and sensitive-header shape.
/// Only after those checks does this function build `PageAdmissionInput` and
/// invoke the generated module's `admit` function. A rejected request returns a
/// provider-neutral HTTP response without invoking `run`.
///
/// `run` receives the admitted `PageContext` plus an owned WASM hint so the
/// generated wrapper can construct `PageResponseRequestHints` without exposing
/// provider lifecycle or credential handling to server-owned `lambda.rs`.
#[cfg(feature = "page")]
pub async fn invoke_page<F, Fut>(
    request: PageHttpRequest,
    state: ores_api_docs_client::PageState,
    auth: &'static str,
    axum_paths: &'static [&'static str],
    admit: ores_api_docs_client::PageAdmissionFn,
    run: F,
) -> Result<PageHttpResponse, RuntimeError>
where
    F: FnOnce(ores_api_docs_client::PageContext, Option<String>) -> Fut + Send,
    Fut: std::future::Future<Output = ores_api_docs_client::FinalizedPageResponse> + Send,
{
    let admission_request = request.clone();
    let structural = match admit_page_request(request, state.clone(), axum_paths) {
        Ok(invocation) => invocation,
        Err(response) => return Ok(response),
    };

    let method = match admission_request.method {
        PageHttpMethod::Get => ores_api_docs_client::PageRequestMethod::Get,
        PageHttpMethod::Head => ores_api_docs_client::PageRequestMethod::Head,
        PageHttpMethod::Unsupported => {
            unreachable!("unsupported methods fail structural admission before product admission")
        }
    };

    let input = ores_api_docs_client::PageAdmissionInput {
        auth: auth.to_owned(),
        route_params: structural.context.route_params.clone(),
        request: ores_api_docs_client::PageRequestContext {
            method,
            raw_path: admission_request.raw_path,
            raw_query: admission_request.raw_query,
            headers: admission_request.headers,
            cookies: admission_request.cookies,
        },
        state,
    };

    let context = match admit(input).await {
        Ok(context) => context,
        Err(rejection) => return Ok(admission_rejection_response(rejection, structural.head)),
    };

    let finalized = run(context, structural.wasm_have).await;
    Ok(finish_page_response(finalized, structural.head))
}

#[cfg(feature = "page")]
fn admission_rejection_response(
    rejection: ores_api_docs_client::PageAdmissionRejection,
    head: bool,
) -> PageHttpResponse {
    let mut headers = Vec::with_capacity(rejection.headers.len());
    let mut set_cookies = Vec::new();
    for (name, value) in rejection.headers {
        if name.eq_ignore_ascii_case("set-cookie") {
            set_cookies.push(value);
        } else {
            headers.push((name.to_ascii_lowercase(), value));
        }
    }
    PageHttpResponse {
        status: rejection.status,
        headers,
        set_cookies,
        body: if head { Vec::new() } else { rejection.body },
    }
}

#[cfg(all(test, feature = "page"))]
mod page_invoke_tests {
    use super::*;
    use ores_api_docs_client::{
        FinalizedPageResponse, PageAdmissionFuture, PageAdmissionInput, PageAdmissionRejection,
        PageContext, PageState,
    };
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static PAGE_RUNS: AtomicUsize = AtomicUsize::new(0);

    fn request(method: PageHttpMethod) -> PageHttpRequest {
        PageHttpRequest {
            method,
            raw_path: "/private/42".to_owned(),
            raw_query: Some("view=full".to_owned()),
            headers: BTreeMap::from([(
                "authorization".to_owned(),
                vec!["Bearer sentinel-secret".to_owned()],
            )]),
            cookies: vec!["session=sentinel-cookie".to_owned()],
            body: Vec::new(),
            provenance: IngressProvenance::provider("test", "request-1"),
        }
    }

    fn reject_session(input: PageAdmissionInput) -> PageAdmissionFuture {
        assert_eq!(input.auth, "session");
        assert_eq!(input.route_params.get("id").map(String::as_str), Some("42"));
        assert_eq!(input.request.raw_path, "/private/42");
        assert_eq!(input.request.raw_query.as_deref(), Some("view=full"));
        assert!(input.request.headers.contains_key("authorization"));
        assert_eq!(input.request.cookies.len(), 1);
        Box::pin(async move { Err(PageAdmissionRejection::text(401, "authentication required")) })
    }

    fn allow_session(input: PageAdmissionInput) -> PageAdmissionFuture {
        assert_eq!(input.auth, "session");
        Box::pin(async move {
            let mut context = PageContext::new(input.route_params, input.request.raw_path);
            context.state = input.state;
            Ok(context)
        })
    }

    #[tokio::test]
    async fn rejected_session_admission_never_executes_page_code() {
        PAGE_RUNS.store(0, Ordering::SeqCst);
        let response = invoke_page(
            request(PageHttpMethod::Get),
            PageState::default(),
            "session",
            &["/private/{id}"],
            reject_session,
            |_context, _wasm_have| async {
                PAGE_RUNS.fetch_add(1, Ordering::SeqCst);
                FinalizedPageResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: b"should-not-render".to_vec(),
                }
            },
        )
        .await
        .expect("runtime response");

        assert_eq!(response.status, 401);
        assert_eq!(response.body, b"authentication required");
        assert_eq!(PAGE_RUNS.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn admitted_session_context_is_the_only_context_given_to_page_code() {
        PAGE_RUNS.store(0, Ordering::SeqCst);
        let response = invoke_page(
            request(PageHttpMethod::Get),
            PageState::default(),
            "session",
            &["/private/{id}"],
            allow_session,
            |context, _wasm_have| async move {
                PAGE_RUNS.fetch_add(1, Ordering::SeqCst);
                assert_eq!(
                    context.route_params.get("id").map(String::as_str),
                    Some("42")
                );
                FinalizedPageResponse {
                    status: 200,
                    headers: vec![("content-type".to_owned(), "text/html".to_owned())],
                    body: b"rendered".to_vec(),
                }
            },
        )
        .await
        .expect("runtime response");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"rendered");
        assert_eq!(PAGE_RUNS.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn head_admission_rejection_drops_only_the_body() {
        let response = invoke_page(
            request(PageHttpMethod::Head),
            PageState::default(),
            "session",
            &["/private/{id}"],
            reject_session,
            |_context, _wasm_have| async {
                panic!("rejected HEAD request must not execute page code")
            },
        )
        .await
        .expect("runtime response");

        assert_eq!(response.status, 401);
        assert!(response.body.is_empty());
        assert!(response
            .headers
            .iter()
            .any(|(name, value)| name == "cache-control" && value == "no-store"));
    }
}
