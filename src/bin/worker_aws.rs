use std::convert::Infallible;

use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use ores_middleware::OperationOutcome;
use serde_json::Value;
use __CRATE__::adapters::aws::{from_event, invocation_boundary, trusted_invocation_metadata};

fn require_completed<T>(outcome: OperationOutcome<T>) -> Result<T, Error> {
    match outcome {
        OperationOutcome::Completed(value) => Ok(value),
        OperationOutcome::Failed(failure) => Err(std::io::Error::other(format!(
            "Lambda invocation middleware rejected callback: {}",
            failure.code
        ))
        .into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Fail cold start if repository-owned invocation policy is absent or cannot
    // be admitted. The boundary is immutable and safe to clone per callback.
    let boundary = invocation_boundary().map_err(|error| -> Error { Box::new(error) })?;
    run(service_fn(move |event: LambdaEvent<Value>| {
        let boundary = boundary.clone();
        async move {
            let metadata = trusted_invocation_metadata(&event, "worker");
            let outcome = boundary
                .run(metadata, async move { Ok::<_, Infallible>(from_event(event)) })
                .await
                .map_err(|error| -> Error { Box::new(error) })?;
            Ok::<_, Error>(serde_json::to_value(require_completed(outcome)?)?)
        }
    }))
    .await
}
