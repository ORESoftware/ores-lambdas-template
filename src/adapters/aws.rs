//! AWS Lambda adapter: the Lambda event *is* the command envelope; request id comes from the
//! Lambda context. Errors are returned as receipts (HTTP 200 for direct invokes), never as
//! runtime panics, so retries and destinations behave predictably.
use crate::runtime::{handle, Provider, Receipt};
use lambda_runtime::LambdaEvent;
use serde_json::Value;

pub fn from_event(event: LambdaEvent<Value>) -> Receipt {
    let raw = serde_json::to_vec(&event.payload).unwrap_or_default();
    // A caller may omit provider/requestId in the raw event; fill them from Lambda's context so
    // the envelope is complete before validation.
    let mut value: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.entry("provider")
            .or_insert_with(|| Value::String("aws-lambda".into()));
        map.entry("requestId")
            .or_insert_with(|| Value::String(event.context.request_id.clone()));
    }
    let raw = serde_json::to_vec(&value).unwrap_or_default();
    handle(&raw, Provider::AwsLambda, &event.context.request_id)
}
