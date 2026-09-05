//! Minimal AWS health Lambda: returns a fixed receipt so uptime checks need no envelope.
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(service_fn(|event: LambdaEvent<Value>| async move {
        Ok::<_, Error>(json!({ "requestId": event.context.request_id, "provider": "aws-lambda", "ok": true, "operation": "health", "result": { "status": "ok" } }))
    }))
    .await
}
