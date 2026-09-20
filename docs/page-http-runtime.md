# Generated web-page HTTP runtime — draft

> Scope: provider hosting for generated `*-web-server.rs` / `*-admin-web-server.rs` page Lambdas. This is not the API/RPC operation-Lambda dispatcher.

## Coordinated ownership

The web-page Lambda stack is split deliberately:

| PR | Owns |
| --- | --- |
| `api-docs#177` | page/API boundary and page manifest/docs direction |
| `api-docs#176` | provider-neutral page module, page/state/finalization ABI |
| `ores-stack#69` | repository admission, deterministic web projections and source sync/check |
| `ores-stack#68` | provider-wrapper build/package orchestration |
| `ores-lambdas-template#35` | provider HTTP lifecycle, bounded admission and provider envelopes |

The server source tree is:

```text
src/pages/x/y/z/
  page.rs      # authored routing/behavior authority
  gen.rs       # optional authored build-time enumeration
  lambda.rs    # generated provider-neutral module; no main()
```

Provider-specific executables are generated outside server source, under an ignored build tree such as:

```text
*-lambdas/build/lambda/aws/funcs/web/<id>/main.rs
*-lambdas/build/lambda/gcp/funcs/web/<id>/main.rs
```

The same deterministic wrapper template is used by conformance and deployment builds so test and production mains cannot drift.

## Generated-module ABI

`api-docs#176` binds the private authored page and its admitted finalizer inside generated sibling `lambda.rs` and exports only:

```rust
pub async fn init_state() -> Result<
    ores_api_docs_client::PageState,
    ores_api_docs_client::PageLambdaStateError,
>;

pub async fn run(
    context: ores_api_docs_client::PageContext,
    hints: ores_api_docs_client::PageResponseRequestHints<'_>,
) -> ores_api_docs_client::FinalizedPageResponse;
```

The provider runtime does **not** receive or invoke `PageFn` or `PageFinalizeFn`. Those remain bound inside the generated server module so standalone/conformance/provider hosts execute the same page and response-finalization authority.

## Provider-runtime API

This crate owns normalized provider HTTP shapes:

```rust
pub struct PageHttpRequest {
    pub method: PageHttpMethod,
    pub raw_path: String,
    pub raw_query: Option<String>,
    pub headers: BTreeMap<String, Vec<String>>,
    pub cookies: Vec<String>,
    pub body: Vec<u8>,
    pub provenance: IngressProvenance,
}

pub struct PageInvocation {
    pub context: ores_api_docs_client::PageContext,
    pub wasm_have: Option<String>,
    pub head: bool,
}

pub fn admit_page_request(
    request: PageHttpRequest,
    state: ores_api_docs_client::PageState,
    axum_paths: &'static [&'static str],
) -> Result<PageInvocation, PageHttpResponse>;

pub fn finish_page_response(
    finalized: ores_api_docs_client::FinalizedPageResponse,
    head: bool,
) -> PageHttpResponse;
```

Provider wrappers therefore execute:

```text
provider event/http
  -> provider adapter normalization
  -> admit_page_request
  -> generated_lambda::run(invocation.context, invocation.hints())
  -> finish_page_response
  -> provider response envelope
```

AWS/GCP outer loops remain generic `run_page(state, handler)` hosts. They do not import product routing or page code directly.

## Fail-closed execution order

1. provider envelope shape/size admission;
2. trusted provider context -> `IngressProvenance`;
3. lowercase header normalization and count/byte limits;
4. cookie count/byte limits;
5. GET/HEAD admission and body rejection;
6. raw-path validation and exact-once segment percent decoding;
7. one generated page-pattern match and route-param extraction;
8. ambiguous sensitive-header rejection;
9. shared auth/session middleware when that ABI is available;
10. `PageContext` construction;
11. generated `lambda.rs::run` page invocation + shared finalization;
12. HEAD body suppression after finalization;
13. provider response-envelope conversion.

Malformed client paths return stable 400 responses. Malformed generated route patterns are server/build faults and return stable non-cacheable 500 responses rather than blaming the client.

## Raw path semantics

- Preserve the provider's raw path; never reconstruct it from decoded fields.
- Split on `/` before decoding.
- Percent-decode each segment exactly once.
- `%2F` inside a segment is data, not a separator.
- Reject invalid escapes, invalid UTF-8, NUL and decoded `.` / `..` segments.
- `%25` remains literal `%`; no later layer decodes again.
- Catch-all params rejoin already-decoded segments with `/`.
- `PageContext.request_path` retains the raw request path.
- Generated route patterns are bounded and malformed/duplicate capture patterns fail as server errors.

## Headers, cookies and bodies

Requests are bounded before page execution:

- body bytes are capped before/after AWS base64 decoding;
- header count, individual values and aggregate bytes are bounded;
- cookie count, individual values and aggregate bytes are bounded;
- CR/LF/NUL in normalized header/cookie inputs is rejected;
- `host`, `authorization` and `content-length` must be semantically unambiguous;
- repeated response `Set-Cookie` values remain separate and are never comma-folded.

AWS API Gateway v2 emits repeated cookies through `cookies[]`; GCP HTTP emits repeated `Set-Cookie` headers.

## Trusted provenance

`IngressProvenance` is provider-created capability data, never inferred from browser-controlled headers.

- AWS request IDs come from Lambda/provider event context.
- Generic GCP/Cloud Run forwarding/trace headers remain untrusted unless a future adapter validates authenticated platform metadata out of band.
- `x-forwarded-*`, `forwarded`, `x-amzn-*`, `x-goog-*` and trace headers do not become caller identity merely because a client supplied them.

Authenticated/admin page deployment remains blocked until the shared auth/session middleware ABI is wired and proven in this outer provider boundary.

## HEAD semantics

HEAD uses the same route admission and generated `lambda.rs::run` path as GET. Only after the page has been finalized does `finish_page_response` remove the body. Status and semantic headers therefore come from the same response authority as GET.

## AWS host

Initial AWS support targets API Gateway HTTP API payload v2 / Lambda Function URLs through the Rust Lambda runtime on the OS-only Lambda environment.

The generated build-only wrapper conceptually does:

```rust
#[path = "<exact-server-lambda.rs>"]
mod generated_lambda;

#[tokio::main]
async fn main() -> Result<(), RuntimeError> {
    let state = generated_lambda::init_state()
        .await
        .map_err(RuntimeError::state_init)?;

    page_runtime::aws::run_page(state, |state, request| async move {
        match page_runtime::admit_page_request(
            request,
            state,
            generated_lambda::ORES_PAGE_AXUM_PATHS,
        ) {
            Ok(invocation) => {
                let finalized = generated_lambda::run(
                    invocation.context,
                    invocation.hints(),
                )
                .await;
                Ok(page_runtime::finish_page_response(finalized, invocation.head))
            }
            Err(response) => Ok(response),
        }
    })
    .await
}
```

The provider wrapper is ignored/generated build material. No AWS runtime symbols live in server sibling `lambda.rs`.

## GCP host

The GCP wrapper uses the same generated server module and admission/finalization sequence. The GCP runtime adapter may own the required `PORT` listener; `$PORT` and listener lifecycle are provider infrastructure and do not belong in generated server source.

## Reproducibility

- `ores-api-docs-client` is pinned to the exact reviewed api-docs head.
- Cargo.lock is Cargo-generated and release/test builds use `--locked`.
- Provider wrapper/build receipts bind exact server source, generated module SHA-256, api-docs revision, provider runtime revision, provider/architecture and toolchain.
- No floating `main` revision is accepted as exact-source evidence.
- Ordinary source/build commands never deploy cloud resources implicitly.

## Required conformance

Before broad rollout, prove at least:

- standalone + AWS + GCP use the same generated `lambda.rs` bytes;
- route params agree for static/dynamic/catch-all/optional-catch-all cases;
- malformed request paths fail before generated-module execution;
- malformed generated patterns are stable server failures;
- invalid/oversized base64 bodies fail before execution;
- untrusted headers cannot forge provenance;
- duplicate sensitive headers fail closed;
- request/response cookies preserve multiplicity;
- HEAD runs the same generated page/finalizer then drops only the body;
- render errors do not expose internal details;
- auth/session pages remain deployment-blocked until middleware parity is proven;
- generated server `lambda.rs` contains no `main()`, provider SDK/runtime selection, credentials or mutable deployment config.

## Relationship to other runtimes

The existing command-envelope/background runtime remains separate. Web-page HTTP shares hardened provider-hosting primitives and reproducible build practices with that runtime, but it does not share RPC identity or command-envelope semantics.
