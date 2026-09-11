#![cfg(feature = "http")]
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use __CRATE__::adapters::http::{detect_provider, router, HttpConfig};
use __CRATE__::runtime::{Provider, SCHEMA_VERSION};

fn app() -> axum::Router {
    app_for(Provider::Local)
}

fn app_for(provider: Provider) -> axum::Router {
    router(HttpConfig { provider })
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn probes_and_invoke() {
    let res = app()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let env = format!(
        r#"{{"provider":"gcp-cloud-run","requestId":"r-9","command":{{"schemaVersion":"{SCHEMA_VERSION}","operation":"version"}}}}"#
    );
    let res = app()
        .oneshot(
            Request::post("/invoke")
                .header("content-type", "application/json")
                .body(Body::from(env))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["provider"], "local");
    assert_eq!(body["requestId"], "http");
    assert_eq!(body["result"]["schemaVersion"], SCHEMA_VERSION);
}

#[tokio::test]
async fn platform_metadata_overrides_spoofed_body_metadata() {
    let res = app_for(Provider::GcpCloudRun)
        .oneshot(
            Request::post("/invoke")
                .header("content-type", "application/json")
                .header("x-request-id", "generic-spoof")
                .header("x-cloud-trace-context", "trace-42/span-7;o=1")
                .body(Body::from(include_str!("fixtures/spoofed-invocation.json")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["provider"], "gcp-cloud-run");
    assert_eq!(body["requestId"], "trace-42");
    assert_ne!(body["requestId"], "untrusted-body");
}

#[tokio::test]
async fn azure_invocation_header_precedes_generic_request_id() {
    let res = app_for(Provider::AzureFunctions)
        .oneshot(
            Request::post("/api/invoke")
                .header("content-type", "application/json")
                .header("x-request-id", "generic-spoof")
                .header("x-azure-functions-invocationid", "azure-42")
                .body(Body::from(include_str!("fixtures/spoofed-invocation.json")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["provider"], "azure-functions");
    assert_eq!(body["requestId"], "azure-42");
}

#[tokio::test]
async fn unsafe_request_id_is_not_reflected() {
    let res = app()
        .oneshot(
            Request::post("/invoke")
                .header("content-type", "application/json")
                .header("x-request-id", "bad id with spaces")
                .body(Body::from(include_str!("fixtures/spoofed-invocation.json")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = json_body(res).await;
    assert_eq!(body["requestId"], "request");
    assert!(!body.to_string().contains("bad id with spaces"));
}

#[tokio::test]
async fn bad_envelope_is_400_and_request_id_comes_from_header() {
    let res = app()
        .oneshot(
            Request::post("/invoke")
                .header("x-request-id", "trace-42")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = json_body(res).await;
    assert_eq!(body["requestId"], "trace-42");
    assert_eq!(body["error"]["code"], "invalid_invocation");
}

#[tokio::test]
async fn oversized_body_is_413() {
    let big = vec![b' '; __CRATE__::runtime::MAX_INVOCATION_BYTES + 2];
    let res = app()
        .oneshot(Request::post("/invoke").body(Body::from(big)).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn provider_detection_follows_platform_env() {
    assert_eq!(
        detect_provider(|k| (k == "FUNCTIONS_CUSTOMHANDLER_PORT").then(|| "7071".into())),
        Provider::AzureFunctions
    );
    assert_eq!(
        detect_provider(|k| (k == "K_SERVICE").then(|| "svc".into())),
        Provider::GcpCloudRun
    );
    assert_eq!(detect_provider(|_| None), Provider::Local);
}
