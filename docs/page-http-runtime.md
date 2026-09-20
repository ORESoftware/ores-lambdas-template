# Generated web-page HTTP runtime

> Scope: provider hosting for generated `*-web-server.rs` / `*-admin-web-server.rs` page Lambdas. This is not the API/RPC operation-Lambda dispatcher.

## Ownership split

The server repository owns authored page behavior and the provider-neutral generated sibling:

```text
src/pages/x/y/z/
  page.rs      # authored route + behavior authority
  gen.rs       # optional authored build-time enumeration
  lambda.rs    # generated provider-neutral module; no provider main()
```

The `*-lambdas` repository owns provider lifecycle, provider HTTP/event normalization, build-only `main.rs`, compilation, packaging and deployment artifacts. Provider-specific executables live only below ignored build output such as:

```text
build/lambda/aws/funcs/web/<id>/main.rs
build/lambda/gcp/funcs/web/<id>/main.rs
```

Provider SDK/runtime identifiers, `$PORT`, credentials and mutable deployment configuration do not belong in server-owned `lambda.rs`.

## Generated page ABI

The current authority is merged `ORESoftware/api-docs#195`, revision:

```text
f58db5a020339879b6aa0b5404c59b4bd9cb2357
```

A generated page module exposes the following provider-neutral surface:

```rust
pub const ORES_PAGE_AUTH: &str;
pub const ORES_PAGE_AXUM_PATHS: &[&str];

pub async fn init_state() -> Result<
    ores_api_docs_client::PageState,
    ores_api_docs_client::PageLambdaStateError,
>;

pub fn admit(
    input: ores_api_docs_client::PageAdmissionInput,
) -> ores_api_docs_client::PageAdmissionFuture;

pub async fn run(
    context: ores_api_docs_client::PageContext,
    hints: ores_api_docs_client::PageResponseRequestHints<'_>,
) -> ores_api_docs_client::FinalizedPageResponse;
```

`ORES_PAGE_AUTH` is copied from authored `#[ores_page(auth = "...")]` metadata. Public pages bind `admit` to the built-in `admit_public_page`; that function rejects any non-public auth requirement. Session/admin pages bind `admit` to the product-owned `ores_page_admit_request` hook. The provider runtime transports request material but never interprets product roles or session semantics itself.

## Required execution order

Every AWS and GCP web-page host follows this order:

```text
provider HTTP/event
  -> bounded provider normalization
  -> method/body/path/header structural admission
  -> generated route match + route params
  -> PageAdmissionInput
  -> generated lambda::admit
  -> admitted PageContext
  -> generated lambda::run
  -> shared response finalization
  -> provider response envelope
```

The security invariant is simple: **generated page code is not callable for the request until `lambda::admit` succeeds.** A rejected admission becomes an HTTP response and never executes `lambda::run`.

The shared runtime entrypoint is `invoke_page`. Provider-specific outer loops normalize their envelopes and call it; they do not duplicate product auth policy.

Conceptually the generated AWS/GCP build-only wrapper is:

```rust
mod lambda;

#[tokio::main]
async fn main() -> Result<(), ores_lambda_runtime::RuntimeError> {
    let state = lambda::init_state()
        .await
        .map_err(ores_lambda_runtime::RuntimeError::state_init)?;

    ores_lambda_runtime::aws::run_page(state, |state, request| async move {
        ores_lambda_runtime::invoke_page(
            request,
            state,
            lambda::ORES_PAGE_AUTH,
            lambda::ORES_PAGE_AXUM_PATHS,
            lambda::admit,
            |context, wasm_have| async move {
                lambda::run(
                    context,
                    ores_api_docs_client::PageResponseRequestHints {
                        wasm_have: wasm_have.as_deref(),
                        dev_reload_script: None,
                    },
                )
                .await
            },
        )
        .await
    })
    .await
}
```

The GCP wrapper uses the same `invoke_page` sequence and generated `lambda.rs` bytes; only the outer provider host changes.

## Admission input

`PageAdmissionInput` receives:

- the authored auth requirement;
- validated route params;
- normalized GET/HEAD method;
- raw path and optional raw query;
- lowercase bounded headers;
- bounded cookies;
- the exact type-erased `PageState` returned by `lambda::init_state`.

Header/cookie values are redacted from the admission types' `Debug` implementations because they may contain bearer tokens, session cookies, CSRF data or other credentials.

## Structural request hardening

Requests are rejected before product admission or page execution when they violate the provider-neutral structural rules:

- only GET and HEAD are accepted for filesystem pages;
- request bodies are rejected and byte bounded;
- path and query lengths are bounded;
- header count, individual values and aggregate bytes are bounded;
- cookie count, individual values and aggregate bytes are bounded;
- CR/LF/NUL in normalized header/cookie material is rejected;
- `host`, `authorization` and `content-length` must be unambiguous;
- generated route-pattern count is bounded;
- malformed generated route patterns are server failures, not client 4xx guesses.

## Path semantics

- Preserve the provider's raw path; never reconstruct it from a decoded path.
- Split on `/` before percent decoding.
- Percent-decode each segment exactly once.
- `%2F` inside a segment remains route-parameter data, not a new separator.
- Reject invalid escapes, invalid UTF-8, NUL and decoded `.` / `..` segments.
- Catch-all params rejoin already-decoded segments with `/`.
- `PageContext.request_path` preserves the raw request path.

## Provider trust boundary

`IngressProvenance` is provider-created capability data. Browser-controlled forwarding and trace headers do not become caller identity.

- AWS request IDs come from Lambda/provider event context.
- Generic Cloud Run forwarding/trace headers remain untrusted unless a future adapter validates authenticated platform metadata out of band.
- `x-forwarded-*`, `forwarded`, `x-amzn-*`, `x-goog-*` and trace headers cannot assert product identity merely because they are present.

Product auth/session middleware may inspect the bounded normalized request supplied through `PageAdmissionInput`, but provider adapters must not replace that middleware with cloud-specific identity guesses.

## HEAD and response semantics

HEAD uses the same structural admission, product admission, generated page execution and finalization path as GET. Only after finalization is the response body removed. Status and semantic headers therefore come from the same authority as GET.

Repeated `Set-Cookie` values remain separate throughout admission/finalization/provider conversion and are never comma-folded.

Admission rejections preserve their status and headers. For HEAD rejections, only the body is suppressed.

## Provider hosts

### AWS

Initial AWS support targets API Gateway HTTP API payload v2 / Lambda Function URLs through the Rust Lambda runtime on the OS-only Lambda environment. Provider normalization owns base64 decoding, Lambda request context, bounded request metadata and response-envelope conversion.

The production artifact is a compiled Linux executable packaged as Lambda `bootstrap`/`bootstrap.zip` or OCI. Server `.rs` source is reproducibility evidence, not the deployed artifact.

### GCP

The GCP host is an HTTP/Cloud Run adapter. It owns the required `$PORT` listener and binds on `0.0.0.0`; those are provider lifecycle concerns and stay outside generated server source.

The production artifact is a compiled Linux executable suitable for the certified OS-only lane or an OCI image.

## Reproducibility

- `ores-api-docs-client` is pinned to exact merged revision `f58db5a020339879b6aa0b5404c59b4bd9cb2357`.
- `Cargo.lock` is Cargo-generated and CI/release builds use `--locked`.
- Provider wrapper/build receipts bind exact server source, generated-module SHA-256, runtime revision, provider, architecture and toolchain.
- No floating `main` revision is accepted as exact-source evidence.
- Ordinary source generation, build and package operations never mutate cloud resources; deployment remains explicit.

## Required conformance before fleet rollout

The rollout proof must show:

- standalone, AWS and GCP hosts consume byte-identical generated `lambda.rs` source;
- session-page admission executes before page code for AWS and GCP wrappers;
- a rejected admission never executes page code;
- the built-in public admission succeeds only for `auth = "public"` and fails closed for non-public auth;
- route params agree across static, dynamic and catch-all routes;
- malformed paths/patterns fail before page execution;
- untrusted headers cannot forge provenance;
- request/response cookies preserve multiplicity;
- HEAD follows the admitted GET execution path and suppresses only the final body;
- generated server `lambda.rs` contains no provider lifecycle, SDK selection, credentials or mutable deployment config;
- real Canonical/Sonus canaries and the required `*-test` consumers compile/package against the same reviewed generator/runtime revisions.
