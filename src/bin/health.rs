//! Minimal AWS health Lambda: returns a fixed receipt so uptime checks need no envelope.
use std::convert::Infallible;

use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use ores_middleware::OperationOutcome;
use serde_json::{json, Value};
use __CRATE__::adapters::aws::{invocation_boundary, trusted_invocation_metadata};

fn require_completed<T>(outcome: OperationOutcome<T>) -> Result<T, Error> {
    match outcome {
        OperationOutcome::Completed(value) => Ok(value),
        OperationOutcome::Failed(failure) => Err(std::io::Error::other(format!(
            "Lambda invocation middleware rejected health callback: {}",
            failure.code
        ))
        .into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let boundary = invocation_boundary().map_err(|error| -> Error { Box::new(error) })?;
    run(service_fn(move |event: LambdaEvent<Value>| {
        let boundary = boundary.clone();
        async move {
            let metadata = trusted_invocation_metadata(&event, "health");
            let request_id = event.context.request_id.clone();
            let outcome = boundary
                .run(metadata, async move {
                    Ok::<_, Infallible>(json!({
                        "requestId": request_id,
                        "provider": "aws-lambda",
                        "ok": true,
                        "operation": "health",
                        "result": { "status": "ok" }
                    }))
                })
                .await
                .map_err(|error| -> Error { Box::new(error) })?;
            Ok::<_, Error>(require_completed(outcome)?)
        }
    }))
    .await
}
