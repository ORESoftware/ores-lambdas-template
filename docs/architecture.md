# __PREFIX__-lambdas — architecture

Follows the fleet lambda contract established by `zed-pkg/zed-lambdas`: **one provider-neutral
core, thin adapters, no listener started by the library, bounded fail-closed envelopes, fixed
error messages, architecture-specific artifacts with a release manifest.** This repository adds
the adapters the first generation lacked for platforms whose *function process must serve HTTP*.

## Ownership rule

A function lives here when it is deployed independently and reused by more than one server or
app. A handler coupled to one server's private route stays under that server's `src/lambdas`.

## Core (`src/runtime.rs`)

Envelope `__PREFIX__.worker-command.v1`: `{ provider, requestId, command: { schemaVersion,
operation, payload } }`. Size is checked before parsing (256 KiB). Unknown fields, unsupported
schema versions, unbounded identifiers, scalar payloads and unknown operations are rejected with
fixed messages that never echo input. `dispatch` initializes provider clients only for an accepted
operation and reuses them on warm invocations (cold-start boundary). Retries, cancellations and
authentication disagreement must not create duplicate durable side effects — operations are
idempotent by `requestId`.

## Adapters — how each platform hosts the same core

| platform | adapter | how it runs | artifact | notes |
|---|---|---|---|---|
| AWS Lambda | `worker-aws`, `health` (`--features aws`) | `lambda_runtime` on `provided.al2023`, arm64 + x86_64 | ZIP with exactly one `bootstrap` (cargo-lambda) | direct/async invoke; retries + SQS destination in `deploy/aws/template.yaml` |
| Google Cloud Run | `worker-http` (`--features http`) | listens on `PORT`; Cloud Run functions do not support Rust, the container is the function | OCI image (shared images workflow) | `deploy/gcp/service.yaml`: scale-to-zero, startup CPU boost, probes |
| Azure Functions | `worker-http` | **custom handler**: host launches the binary, sets `FUNCTIONS_CUSTOMHANDLER_PORT`, forwards raw HTTP (`host.json`) | musl binary in the function app, or container | `deploy/azure/` |
| Vercel | `api/[[...slug]].rs` (`--features vercel`) | official Rust runtime, one catch-all function | Vercel build | `deploy/vercel/vercel.json` |
| Scintilla (`scintilla-run`) / k8s | `worker-http` | OCI runtime with `/healthz` `/readyz`, HTTP binding | same image | `deploy/scintilla/function.json` |
| Cloudflare Workers / OCI jobs / cron / local | `worker-portable` (`--features portable`) | strict JSON on stdin → one JSON receipt on stdout | binary | host owns the event loop |

`worker-http` reads `PORT` / `FUNCTIONS_CUSTOMHANDLER_PORT` through `.cli-flags.toml` (flags-2-env
at the argv boundary), so the platform contract is declared, audited and typed rather than read
ad hoc. Provider detection uses the platforms' own environment (`K_SERVICE`, `FUNCTIONS_*`,
`SCINTILLA_FUNCTION`).

## What the reference platforms taught this layout

- **AWS** — immutable versions + aliases, async retry/destination semantics, one-binary ZIP with
  `bootstrap`, arm64 first (Graviton price/perf); we return receipts instead of panicking so
  retries stay idempotent. ([aws-lambda-rust-runtime](https://github.com/aws/aws-lambda-rust-runtime))
- **GCP** — no Rust in Cloud Run functions; use Cloud Run containers with `PORT`, request-scoped
  CPU, min/max instances, startup CPU boost. ([Rust serverless on the big three](https://shinglyu.com/web/2025/09/16/rust-serverless-on-the-big-three-clouds-aws-azure-and-gcp-compared.html))
- **Azure** — custom handlers are a plain HTTP server behind the Functions host; forward the raw
  request and keep the binary self-contained (musl). (same source)
- **Vercel** — per-file functions; a catch-all keeps one binary and one routing authority.
  ([vercel-community/rust → official runtime](https://github.com/vercel-community/rust))
- **Scintilla** — the in-house runner already covers revisions/aliases, durable async, CloudEvents,
  cron; this repo only needs to be a well-behaved OCI HTTP function for it.

## Artifact boundary

Build output is architecture-specific. AWS ZIPs contain exactly one executable `bootstrap`. OCI
final stages keep only the runtime executable(s) and `.cli-flags.toml`. CI writes a manifest with
source SHA, target architecture, adapter, runtime, checksum and toolchain for every artifact.

## Shared code

Domain source candidate: `__ORG__/__PREFIX__-lib-core` / `__PREFIX__-orm-core` — resolve and pin a
reviewed release or exact commit via zed-pkg before importing; never copy entities or reach through a
monorepo checkout.
