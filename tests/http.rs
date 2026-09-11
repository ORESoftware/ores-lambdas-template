#![cfg(feature = "http")]
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use __CRATE__::adapters::http::{detect_provider, router, HttpConfig};
use __CRATE__::runtime::{Provider, SCHEMA_VERSION};

fn app() -> axum::Router {
    router(HttpConfig {
        provider: Provider::Local,
    })
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
    let body: serde_json::Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["ok"], true);
    assert_eq!(body["result"]["schemaVersion"], SCHEMA_VERSION);
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
    let body: serde_json::Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
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
