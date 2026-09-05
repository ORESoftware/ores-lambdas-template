# Azure Functions (custom handler)

Azure runs the compiled `worker-http` binary as a **custom handler**: the Functions host
starts it, sets `FUNCTIONS_CUSTOMHANDLER_PORT`, and (with `enableForwardingHttpRequest`)
forwards the raw HTTP request to it — so the same binary that serves Cloud Run serves Azure.

```sh
cargo build --release --features http --target x86_64-unknown-linux-musl   # Linux consumption/flex plans
cp target/x86_64-unknown-linux-musl/release/worker-http deploy/azure/worker-http
cd deploy/azure && func azure functionapp publish <app-name>   # or: az functionapp deployment source config-zip
```

Container alternative (Premium / Container Apps): the shared image + `deploy/azure/host.json`
copied into `/home/site/wwwroot`. The Azure SDK for Rust is not needed by this adapter.
