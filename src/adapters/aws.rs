//! AWS Lambda adapter: Lambda runtime context is authoritative for provider and request id.
//! Caller payloads may carry those fields for cross-provider portability, but they cannot spoof the
//! host provenance recorded in receipts. Errors are returned as receipts, never runtime panics.
use crate::runtime::{handle_bound, Provider, Receipt};
use lambda_runtime::LambdaEvent;
use serde_json::{Map, Value};

fn complete_envelope(payload: &Value, request_id: &str) -> Value {
    let Value::Object(map) = payload else {
        return payload.clone();
    };

    let provider = map
        .get("provider")
        .cloned()
        .unwrap_or_else(|| Value::String("aws-lambda".into()));
    let request_id = map
        .get("requestId")
        .cloned()
        .unwrap_or_else(|| Value::String(request_id.to_owned()));

    // This adapter is control-plane boundary code rather than a measured JSON
    // allocation hot path. Build a fresh object, including deep-cloned nested
    // values, so completion never mutates or aliases the caller-owned payload.
    Value::Object(Map::from_iter(
        map.iter()
            .filter(|(key, _)| *key != "provider" && *key != "requestId")
            .map(|(key, value)| (key.clone(), value.clone()))
            .chain([
                ("provider".to_owned(), provider),
                ("requestId".to_owned(), request_id),
            ]),
    ))
}

pub fn from_event(event: LambdaEvent<Value>) -> Receipt {
    // Build a complete, non-aliasing envelope from the caller payload, then let
    // the runtime stamp provider/requestId from Lambda's context: handle_bound
    // overrides both unconditionally, because the runtime context is stronger
    // than caller-controlled envelope metadata.
    let value = complete_envelope(&event.payload, &event.context.request_id);
    let raw = serde_json::to_vec(&value).unwrap_or_default();
    handle_bound(&raw, Provider::AwsLambda, &event.context.request_id)
}

#[cfg(test)]
mod tests {
    use super::complete_envelope;
    use serde_json::json;

    #[test]
    fn completion_returns_a_fresh_payload_and_preserves_the_source() {
        let source = json!({
            "command": {"operation": "echo", "payload": {"nested": [1, 2, 3]}}
        });
        let original = source.clone();

        let completed = complete_envelope(&source, "req-1");

        assert_eq!(source, original);
        assert_eq!(completed["provider"], "aws-lambda");
        assert_eq!(completed["requestId"], "req-1");
        assert_eq!(completed["command"], source["command"]);
    }

    #[test]
    fn caller_supplied_envelope_identity_is_preserved_in_the_new_value() {
        let source = json!({
            "provider": "custom",
            "requestId": "caller-request",
            "command": {"operation": "echo", "payload": {}}
        });

        let completed = complete_envelope(&source, "lambda-request");

        assert_eq!(completed["provider"], "custom");
        assert_eq!(completed["requestId"], "caller-request");
    }
}
