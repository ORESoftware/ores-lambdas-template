#![cfg(feature = "aws")]
use lambda_runtime::{Context, LambdaEvent};
use serde_json::{json, Value};
use __CRATE__::adapters::aws::from_event;
use __CRATE__::runtime::Provider;

fn event(payload: Value, request_id: &str) -> LambdaEvent<Value> {
    let mut context = Context::default();
    context.request_id = request_id.to_owned();
    LambdaEvent::new(payload, context)
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/spoofed-invocation.json")).unwrap()
}

#[test]
fn lambda_context_overrides_spoofed_body_metadata() {
    let receipt = from_event(event(fixture(), "lambda-42"));
    assert!(receipt.ok);
    assert_eq!(receipt.provider, Provider::AwsLambda);
    assert_eq!(receipt.request_id, "lambda-42");
    assert_eq!(receipt.result.unwrap()["status"], "ok");
}

#[test]
fn lambda_context_completes_missing_provenance_fields() {
    let payload = json!({
        "command": {
            "schemaVersion": "__PREFIX__.worker-command.v1",
            "operation": "health",
            "payload": {}
        }
    });
    let receipt = from_event(event(payload, "lambda-43"));
    assert!(receipt.ok);
    assert_eq!(receipt.provider, Provider::AwsLambda);
    assert_eq!(receipt.request_id, "lambda-43");
}

#[test]
fn malformed_event_fails_closed_with_trusted_context() {
    let receipt = from_event(event(json!("not-an-envelope"), "lambda-44"));
    assert!(!receipt.ok);
    assert_eq!(receipt.provider, Provider::AwsLambda);
    assert_eq!(receipt.request_id, "lambda-44");
    assert_eq!(receipt.error.unwrap().code, "invalid_invocation");
}

#[test]
fn unsafe_lambda_request_id_is_normalized_and_not_reflected() {
    let receipt = from_event(event(fixture(), "bad id with spaces"));
    assert!(receipt.ok);
    assert_eq!(receipt.provider, Provider::AwsLambda);
    assert_eq!(receipt.request_id, "request");
    assert!(!serde_json::to_string(&receipt)
        .unwrap()
        .contains("bad id with spaces"));
}
