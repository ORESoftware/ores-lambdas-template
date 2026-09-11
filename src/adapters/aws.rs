//! AWS Lambda adapter: runtime context is authoritative for provider and request id.
//! Completion builds a fresh envelope rather than mutating or aliasing caller-owned payload state.
use crate::runtime::{handle, Provider, Receipt};
use lambda_runtime::LambdaEvent;
use serde_json::{Map, Value};

fn complete_envelope(payload: &Value, request_id: &str) -> Value {
    let Value::Object(map) = payload else {
        return payload.clone();
    };

    Value::Object(Map::from_iter(
        map.iter()
            .filter(|(key, _)| *key != "provider" && *key != "requestId")
            .map(|(key, value)| (key.clone(), value.clone()))
            .chain([
                ("provider".to_owned(), Value::String("aws-lambda".into())),
                ("requestId".to_owned(), Value::String(request_id.to_owned())),
            ]),
    ))
}

pub fn from_event(event: LambdaEvent<Value>) -> Receipt {
    let value = complete_envelope(&event.payload, &event.context.request_id);
    let raw = serde_json::to_vec(&value).unwrap_or_default();
    handle(&raw, Provider::AwsLambda, &event.context.request_id)
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
    fn trusted_lambda_identity_overrides_caller_metadata_without_mutation() {
        let source = json!({
            "provider": "custom",
            "requestId": "caller-request",
            "command": {"operation": "echo", "payload": {}}
        });
        let original = source.clone();

        let completed = complete_envelope(&source, "lambda-request");

        assert_eq!(source, original);
        assert_eq!(completed["provider"], "aws-lambda");
        assert_eq!(completed["requestId"], "lambda-request");
    }
}
