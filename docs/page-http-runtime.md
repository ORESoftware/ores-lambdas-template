# Generated web-page HTTP runtime — draft

> This is the provider-host side of the `*-web-server.rs` page-Lambda design. It is not the API/RPC operation-Lambda dispatcher.

## Why this belongs in `*-lambdas`

A standalone web server owns a long-lived Axum listener and one router containing many pages. A deployed page Lambda has a different outer lifecycle: the cloud provider owns invocation, concurrency, deadlines, request envelopes, and trusted ingress metadata.

The generated `src/pages/**/lambda.rs` should therefore call a small provider runtime facade supplied by the owning `*-lambdas` package instead of embedding AWS/GCP code in every web server.

The product page remains:

```text
provider lifecycle -> normalized PageHttpRequest -> page-route middleware -> PageContext -> page.rs
```

not:

```text
provider lifecycle -> whole application Axum router -> RPC router -> page
```

## Stable crate alias

`ores-stack` should alias the organization-specific package to the same generated-code name:

```toml
ores_page_lambda_runtime = { package = "__PREFIX__-lambdas", ... }
```

Generated `lambda.rs` can then be byte-identical across organizations.

## Proposed provider-neutral API

Conceptually:

```rust
pub struct PageHttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
    pub ingress: IngressProvenance,
    pub request_id: String,
}

pub struct PageHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub async fn invoke_page<F, Fut>(
    request: PageHttpRequest,
    canonical_route: &'static str,
    axum_paths: &'static [&'static str],
    page: F,
) -> Result<PageHttpResponse, RuntimeError>
where
    F: FnOnce(PageContext) -> Fut,
    Fut: Future<Output = PageResult>;
```

The concrete API should reuse shared `api-docs` page types instead of duplicating them when that dependency boundary is available.

## Middleware order

The page runtime should make ordering explicit and testable:

1. provider envelope admission / size bounds;
2. provider identity + trusted ingress provenance;
3. canonical lowercase request-header normalization;
4. method admission (`GET` and `HEAD` first);
5. path match + route-param extraction using the same route grammar as `api-docs`;
6. web auth/session middleware profile;
7. request tracing / deadline propagation;
8. construction of `PageContext`;
9. page invocation;
10. common page-response finalization;
11. provider response conversion.

A rejection before step 9 must prove the page function was not called.

Provider adapters must not trust arbitrary `x-forwarded-*`, identity, or authorization claims merely because a client supplied the header. Trusted ingress/proxy identity is explicit provider context.

## AWS host

The AWS implementation should use the Rust Lambda runtime client on the OS-only `provided.al2023` runtime.

First supported ingress modes:

- API Gateway HTTP API payload v2;
- Lambda Function URL HTTP events.

Later adapters can add ALB if needed. Direct/SQS/event invocation belongs to the operation/background-function runtime, not to a browser page Lambda unless a future page-specific use case is documented.

Conceptually generated `main` calls:

```rust
#[tokio::main]
async fn main() -> Result<(), RuntimeError> {
    ores_page_lambda_runtime::aws::run_page(__ores_handle_page).await
}
```

`run_page` owns the invocation loop and AWS event/response translation. It does not start an Axum TCP listener.

## GCP host

The GCP implementation exposes the same generated page handler through the currently supported HTTP/custom-runtime/OS-only deployment path. Rust page source must not depend on a managed language runtime being present.

Conceptually:

```rust
#[tokio::main]
async fn main() -> Result<(), RuntimeError> {
    ores_page_lambda_runtime::gcp::run_page(__ores_handle_page).await
}
```

For an HTTP-hosted GCP artifact, `run_page` may own the `PORT` listener because the platform invokes the container over HTTP. That listener is provider-host infrastructure, not the product's full standalone Axum application router.

The GCP adapter should preserve Cloud Trace/request metadata only after validating the provider boundary.

## Page router vs provider router

Do not duplicate a full application routing tree inside every Lambda. Each generated binary represents one page source and one validated route shape.

The small page matcher needs only:

- exact static segments;
- one-segment dynamic params;
- terminal catch-all;
- optional terminal catch-all;
- HEAD behavior consistent with GET;
- rejection of mismatched paths.

Its fixtures should be generated from the same `api-docs::FsRoute` test vectors used by standalone Axum projection.

## Response finalization

The existing generated page router already handles HTML status/content type plus CSS/JS/WASM injection. That logic should be extracted behind a shared page-response finalizer in `api-docs` and called by both:

- standalone generated Axum page router;
- this provider page runtime.

Provider adapters add only provider-specific envelope and headers after common finalization.

## Static pages and assets

`gen.rs` remains build-time only. The runtime receives finalized immutable asset metadata from the build artifact; it never executes `gen.rs` during a request.

AWS packaging can either embed small immutable assets with the binary or point at immutable CDN/object-store URLs recorded by the finalized page manifest. GCP packaging follows the same semantic manifest even when the physical artifact is an OCI image.

## Tests required before template rollout

- same normalized request fixture produces the same page status/body/semantic headers under local, AWS, and GCP hosts;
- route params match standalone Axum fixtures for static/dynamic/catch-all/optional-catch-all paths;
- invalid method/path/body bound fails before page invocation;
- untrusted headers cannot forge provider identity/provenance;
- HEAD does not return a body but preserves GET metadata as defined;
- page errors become stable 5xx responses without leaking secrets;
- deadlines/cancellation are propagated where provider APIs expose them;
- provider adapter code contains no product business/domain routing;
- generated `lambda.rs` contains no credentials or mutable deployment config.

## Relationship to existing template runtime

The existing command-envelope runtime (`worker-aws`, `worker-http`, portable workers) remains appropriate for bounded background/integration commands. Page HTTP runtime should be an additional surface with its own narrow types rather than overloading the command envelope with browser HTTP semantics.
