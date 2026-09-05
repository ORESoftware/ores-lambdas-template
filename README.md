# __PREFIX__-lambdas

Provider-neutral Rust functions for **__ORG__**: one bounded command core, thin adapters for
AWS Lambda, Google Cloud Run, Azure Functions (custom handler), Vercel, Scintilla/Kubernetes and
local/stdin hosts. See `docs/architecture.md`.

```sh
cargo test --features http,portable,aws --all-targets
cargo run --features portable --bin worker-portable <<'JSON'
{"provider":"local","requestId":"demo-1","command":{"schemaVersion":"__PREFIX__.worker-command.v1","operation":"health","payload":{}}}
JSON
cargo run --features http --bin worker-http      # then: curl -XPOST localhost:8080/invoke -d '…envelope…'
```

Deploy manifests live under `deploy/<platform>/`; images come from the shared
[ores-gha-workflows](https://github.com/ORESoftware/ores-gha-workflows) container workflow.
