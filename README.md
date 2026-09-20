# __PREFIX__-lambdas

Provider-neutral Rust functions for **__ORG__**: one bounded command core, thin adapters for AWS Lambda, Google Cloud Run, Azure Functions (custom handler), Vercel, Scintilla/Kubernetes and local/stdin hosts. See `docs/architecture.md`.

Every generated repository owns `.ores-lambda.toml`. It declares function provenance (`api_server`, `web_server`, or `custom`), mandatory ORES middleware policy, and the local replica/ingress topology consumed by `ORESoftware/ores-lambda`.

For local development the manifest is compiled into the provider-neutral `ores-lambda-ingress` config, while the same replica counts and port ranges are projected into `ores-compose`. `LAMBDAS_BIND` accepts either a host IP (cloud-style `PORT` remains separate) or the full `host:port` socket supplied by `ores-compose host_bind` for each replica.

The local ingress convention gives each function a prefix such as `/__ores/lambda/worker`; the ingress strips the prefix before forwarding, so the portable worker's `/invoke` endpoint is reached at `/__ores/lambda/worker/invoke`.

```sh
cargo test --features http,portable,aws --all-targets
cargo run --features portable --bin worker-portable <<'JSON'
{"provider":"local","requestId":"demo-1","command":{"schemaVersion":"__PREFIX__.worker-command.v1","operation":"health","payload":{}}}
JSON
cargo run --features http --bin worker-http
```

Before an HTTP Lambda may start, materialize the reviewed middleware concern (`.ores-mw.toml` plus `config/ores-middleware.stack.json`) and install the canonical `ORESoftware/ores-middleware` runtime. Middleware belongs at the invocation boundary rather than only at ingress so AWS/direct/queue/scheduled invocations cannot bypass it. `docs/runtime-integrations.md` owns the concern materialization rules.

Deploy manifests live under `deploy/<platform>/`; images come from the shared [ores-gha-workflows](https://github.com/ORESoftware/ores-gha-workflows) container workflow.
