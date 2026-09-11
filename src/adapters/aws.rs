//! AWS Lambda adapter: Lambda runtime context is authoritative for provider and request id.
//! Caller payloads may carry those fields for cross-provider portability, but they cannot spoof the
//! host provenance recorded in receipts. Errors are returned as receipts, never runtime panics.
use crate::runtime::{handle_bound, Provider, Receipt};
use lambda_runtime::LambdaEvent;
use serde_json::Value;

pub fn from_event(event: LambdaEvent<Value>) -> Receipt {
    let raw = serde_json::to_vec(&event.payload).unwrap_or_default();
    handle_bound(&raw, Provider::AwsLambda, &event.context.request_id)
}
