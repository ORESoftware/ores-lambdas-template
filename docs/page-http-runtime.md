# Generated web-page HTTP runtime — draft

> Scope: provider hosting for generated `*-web-server.rs` / `*-admin-web-server.rs` page Lambdas. This is not the API/RPC operation-Lambda dispatcher.

## Coordinated contract

This runtime is one part of the five-PR web-page Lambda set:

| PR | Owns |
| --- | --- |
| `api-docs#177` | page/API boundary and page manifest/docs direction |
| `api-docs#176` | page invocation/state/finalization ABI and generated sibling `lambda.rs` |
| `ores-stack#69` | repository-role admission, manifest/docs/deployment IR |
| `ores-stack#68` | Lambda materialization/build/package |
| `ores-lambdas-template#35` | provider lifecycle, request normalization and middleware |

The route folder is:

```text
src/pages/x/y/z/
  page.rs      # authored
  gen.rs       # optional authored build-time enumeration
  lambda.rs    # generated; never authored
```

The runtime never turns a page into an RPC operation and never constructs the product's full API/RPC router.

## Cross-crate ABI

`api-docs#176` now exports three typed surfaces from the web-server library for each generated page Lambda:

```rust
const __ORES_PAGE: ::ores_api_docs_client::PageFn =
    ::ores_web_app::ores_pages::__ores_invoke_page_<stem>_<digest>;

const __ORES_PAGE_FINALIZE: ::ores_api_docs_client::PageFinalizeFn =
    ::ores_web_app::ores_pages::__ores_finalize_page_<stem>_<digest>;

const __ORES_PAGE_STATE: ::ores_api_docs_client::PageLambdaStateFn =
    ::ores_web_app::ores_page_lambda_state;
```

Page modules themselves remain private. The digest suffix prevents source-path normalization collisions.

The per-page `PageFinalizeFn` closes over the **same admitted CSS/WASM/JS build metadata** used by standalone Axum. Provider runtimes therefore receive a finalizer function; they do not rediscover page assets or duplicate HTML mutation logic.

## Provider-neutral API

Conceptually:

```rust
pub struct PageHttpRequest {
    pub method: HttpMethod,
    pub raw_path: String,
    pub raw_query: Option<String>,
    pub headers: ClientHeaders,
    pub cookies: ClientCookies,
    pub body: Vec<u8>,
    pub provenance: IngressProvenance,
    pub request_id: String,
    pub deadline: Option<Deadline>,
}

pub struct PageHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub set_cookies: Vec<String>,
    pub body: Vec<u8>,
}

pub async fn invoke_page(
    request: PageHttpRequest,
    state: ores_api_docs_client::PageState,
    canonical_route: &'static str,
    axum_paths: &'static [&'static str],
    page: ores_api_docs_client::PageFn,
    finalize: ores_api_docs_client::PageFinalizeFn,
) -> Result<PageHttpResponse, RuntimeError>;

impl RuntimeError {
    pub fn state_init(error: ores_api_docs_client::PageLambdaStateError) -> Self;
}

pub mod aws {
    pub async fn run_page<H, Fut>(
        state: PageState,
        handler: H,
    ) -> Result<(), RuntimeError>;
}

pub mod gcp {
    pub async fn run_page<H, Fut>(
        state: PageState,
        handler: H,
    ) -> Result<(), RuntimeError>;
}
```

`invoke_page` performs request admission, route matching, middleware and page invocation, then calls the supplied `PageFinalizeFn` with request-scoped finalization hints. Only after that does the provider adapter translate the finalized response into AWS/GCP wire format.

## Execution order

The order is explicit and fail-closed:

1. provider envelope size/shape admission;
2. trusted provider identity and ingress provenance;
3. request-header normalization;
4. GET/HEAD admission;
5. filesystem-route match and route-param extraction;
6. web/admin-web auth and session middleware;
7. tracing/deadline propagation;
8. `PageContext` construction;
9. `PageFn` invocation;
10. `PageFinalizeFn` invocation;
11. provider response conversion.

Any rejection before step 9 must prove the page function was not called. Provider conversion must never re-run page finalization.

## Shared finalization contract

`PageFinalizeFn` is generated per page by `page_router_glue`. It calls the Axum-free `ores_api_docs_client::finalize_page_response` with the page's admitted:

- CSS public path;
- finalized WASM SHA-256;
- JS public path.

The provider runtime supplies only request-scoped hints such as `x-ores-wasm-have`; development reload hints are local standalone concerns and normally absent in cloud runtimes.

The result is a provider-neutral `FinalizedPageResponse { status, headers, body }`. The same finalizer is called by standalone Axum and the page Lambda, so page status/body/content-type/CSS/WASM behavior has one implementation path.

Render failures become stable non-cacheable 500 responses without reflecting `PageError` details to browsers. Detailed causes belong in server-side telemetry.

## Raw URI and percent-decoding

Raw-path semantics must be identical across AWS and GCP:

- Preserve `raw_path` and `raw_query`; never reconstruct them from decoded provider fields.
- Split the raw path on `/` **before** percent-decoding.
- Percent-decode each segment exactly once.
- `%2F` inside a segment is data, not a separator.
- Reject invalid percent escapes, invalid UTF-8 after decoding, NUL, and decoded `.` or `..` segments.
- A decoded `%25` remains a literal `%`; no later component decodes again.
- Catch-all params are decoded segments rejoined with `/`.
- `PageContext.request_path` retains the raw path.

Route-matching fixtures must be shared with `api-docs::FsRoute`/standalone Axum tests for static, dynamic, catch-all and optional-catch-all routes.

## Bodies

AWS payloads may carry `isBase64Encoded` + `body`.

The adapter:

- bounds encoded size before allocating;
- decodes exactly once;
- enforces the configured bound on decoded bytes;
- rejects invalid base64 with 400 before page invocation;
- rejects GET/HEAD bodies for the first contract;
- decides AWS response base64 encoding from the finalized response content, never from page-controlled provider flags.

## Headers

`ClientHeaders` is untrusted, lowercase-normalized and multi-valued.

Do not blindly split comma-containing provider values: API Gateway v2 may already coalesce duplicates, and several legal header values contain commas. Security-sensitive inputs such as `host`, `authorization`, `content-length`, and the configured session cookie must have one unambiguous semantic value or the request is rejected.

Provider identity is never inferred from headers.

## Cookies

Request normalization has one cookie source:

- API Gateway v2 `cookies[]` is parsed into `ClientCookies`;
- Function URL/GCP `cookie` header is parsed into the same structure;
- the raw `cookie` header is then removed from the generic header view.

Response `Set-Cookie` is never comma-folded. Repeated finalized `set-cookie` headers are separated before provider translation:

- AWS payload v2 -> `cookies[]`;
- GCP HTTP -> repeated `Set-Cookie` headers.

Fixtures must include multiple cookies and an `Expires=Wed, 21 Oct ...` value containing a comma.

## HEAD parity

HEAD follows the same matcher, auth, page invocation and `PageFinalizeFn` path as GET. The provider adapter drops the body only after finalization.

Status and semantic headers must match GET. If `content-length` is emitted, it describes the GET body length before the HEAD body is dropped. A page cannot observe whether the request was GET or HEAD.

## Trusted provenance

`IngressProvenance` is a provider-created capability, not a parsed header bag.

Requirements:

- no public constructor usable by product/page code;
- no `From<&ClientHeaders>` conversion;
- AWS builds it from validated provider event context;
- GCP builds it from validated platform/request context;
- middleware that needs scheme, client IP or caller identity receives provenance, not forwarded headers.

Client-supplied `x-forwarded-*`, `forwarded`, `x-amzn-*`, `x-goog-*`, and trace headers remain untrusted strings unless a provider adapter explicitly validates the corresponding platform context.

## AWS host

Initial AWS support:

- Rust Lambda runtime on `provided.al2023`;
- API Gateway HTTP API payload v2;
- Lambda Function URLs.

Generated `main` calls:

```rust
#[tokio::main]
async fn main() -> Result<(), RuntimeError> {
    let state = __ORES_PAGE_STATE().await.map_err(RuntimeError::state_init)?;
    ores_page_lambda_runtime::aws::run_page(state, __ores_handle_page).await
}
```

`run_page` owns the invocation loop and provider event/response translation. It does not start the product Axum TCP listener.

ALB can be added later with its own explicit normalization fixtures. Direct/SQS/background invocation belongs to the operation/background-function runtime, not this browser-page surface.

## GCP host

GCP exposes the same page handler through the supported HTTP/custom-runtime/OS-only hosting path. The generated page source must not assume a managed Rust language runtime.

For container/HTTP hosting, `gcp::run_page` may own the required `PORT` listener. That listener is provider-host infrastructure for one page artifact, not the product's complete application router.

Cloud Trace/request metadata is propagated only from validated provider context.

## State lifecycle

`__ORES_PAGE_STATE` is a `PageLambdaStateFn`. The generated binary calls it once before entering the provider invocation loop, giving normal cold-start reuse.

The constructor must be the same semantic application-state constructor used by the standalone web server. If local Axum and Lambda initialize different auth/database/config state, parity has already failed before page invocation.

State initialization errors are logged server-side and converted to stable provider/runtime errors; secrets and internal error text are not sent to browsers.

## Static generation and assets

`gen.rs` remains build-time only and is never executed during a request.

The per-page finalizer receives immutable finalized asset references from generated page glue. Packaging may embed immutable assets or point to digest-addressed CDN/object-store paths, but the semantic asset metadata is build-time authority and is digest-bound in the deployment receipt.

## Tests required before template rollout

At minimum:

- standalone/AWS/GCP normalized request fixtures produce the same finalized status/body/semantic headers;
- route params match for static, dynamic, catch-all and optional catch-all paths;
- malformed URI/percent encodings fail before page invocation;
- invalid/oversized base64 bodies fail before page invocation;
- untrusted headers cannot forge `IngressProvenance`;
- duplicate security-sensitive headers fail closed;
- request and response cookie normalization preserves multiple cookies;
- HEAD matches GET metadata and returns no body;
- auth/middleware rejection proves the page was not invoked;
- deadlines/cancellation propagate where provider APIs expose them;
- page render errors do not leak internal details;
- the provider runtime contains no product business/domain routing;
- generated `lambda.rs` contains no credentials or mutable deployment config;
- `api-docs#176` compile fixture builds the generated bin under AWS and GCP features with both `PageFn` and `PageFinalizeFn`.

## Relationship to the existing template runtime

The current command-envelope runtime (`worker-aws`, `worker-http`, portable workers) remains the surface for bounded background/integration commands.

Page HTTP runtime is an additional narrow surface with browser HTTP semantics. The shared abstraction between the two is hardened provider hosting and deterministic generated artifacts, not a shared command envelope or RPC identity.
