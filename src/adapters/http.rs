//! HTTP adapter shared by Google Cloud Run (`PORT`), Azure Functions custom handlers
//! (`FUNCTIONS_CUSTOMHANDLER_PORT`, `host.json` forwards the raw request), Scintilla/Kubernetes,
//! and local runs. `POST /invoke` carries the command envelope; `GET /healthz` and `/readyz` are
//! the platform probes. The listener is started by the binary, never by the library.
use crate::runtime::{handle_bound, Provider, Receipt, MAX_INVOCATION_BYTES};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};

#[derive(Debug, Clone, Copy)]
pub struct HttpConfig {
    pub provider: Provider,
}

/// Provider from the environment the platform gives us; explicit override wins.
pub fn detect_provider(env: impl Fn(&str) -> Option<String>) -> Provider {
    if env("FUNCTIONS_CUSTOMHANDLER_PORT").is_some() || env("FUNCTIONS_WORKER_RUNTIME").is_some() {
        Provider::AzureFunctions
    } else if env("K_SERVICE").is_some() || env("CLOUD_RUN_JOB").is_some() {
        Provider::GcpCloudRun
    } else if env("SCINTILLA_FUNCTION").is_some() {
        Provider::Scintilla
    } else {
        Provider::Local
    }
}

pub fn router(config: HttpConfig) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ok" }))
        .route("/invoke", post(invoke))
        // Azure forwards the original path when enableForwardingHttpRequest=true; accept the function route too.
        .route("/api/invoke", post(invoke))
        .layer(DefaultBodyLimit::max(MAX_INVOCATION_BYTES + 1))
        .with_state(config)
}

fn request_id(provider: Provider, headers: &HeaderMap) -> String {
    let provider_header = match provider {
        Provider::GcpCloudRun => headers.get("x-cloud-trace-context"),
        Provider::AzureFunctions => headers.get("x-azure-functions-invocationid"),
        _ => None,
    };

    provider_header
        .or_else(|| headers.get("x-request-id"))
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            if provider == Provider::GcpCloudRun {
                value.split('/').next().unwrap_or(value)
            } else {
                value
            }
        })
        .unwrap_or("http")
        .to_owned()
}

async fn invoke(
    State(config): State<HttpConfig>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Receipt>) {
    let request_id = request_id(config.provider, &headers);
    let receipt = handle_bound(&body, config.provider, &request_id);
    let status = if receipt.ok {
        StatusCode::OK
    } else if receipt
        .error
        .as_ref()
        .is_some_and(|e| e.code == "invocation_too_large")
    {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(receipt))
}
