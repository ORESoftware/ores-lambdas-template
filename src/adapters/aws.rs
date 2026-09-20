//! AWS Lambda adapter: runtime context is authoritative for provider and request id.
//! Completion builds a fresh envelope rather than mutating or aliasing caller-owned payload state.
use crate::runtime::{handle_bound, Provider, Receipt};
use lambda_runtime::LambdaEvent;
use ores_middleware::{
    LambdaInvocationBoundary, LambdaInvocationError, LambdaInvocationMetadata,
    LambdaInvocationTrigger,
};
use serde_json::{Map, Value};

pub const MIDDLEWARE_STACK_CONFIG: &str = "config/ores-middleware.stack.json";
pub const MIDDLEWARE_TARGET: &str = "portable-adapters";

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

/// Build the process-wide callback boundary before the AWS runtime begins
/// polling invocations. Missing/malformed repository policy therefore fails
/// cold start instead of allowing an unguarded callback.
pub fn invocation_boundary() -> Result<LambdaInvocationBoundary, LambdaInvocationError> {
    LambdaInvocationBoundary::from_env(
        "__PREFIX__-lambdas",
        Some(MIDDLEWARE_TARGET),
        MIDDLEWARE_STACK_CONFIG,
    )
}

/// Derive callback metadata only from the trusted Lambda runtime context and
/// the already-decoded payload size. AWS X-Ray trace IDs are intentionally not
/// projected into the W3C 32-hex trace-id slot; the middleware boundary creates
/// a standards-shaped trace ID when no trusted W3C value exists.
pub fn trusted_invocation_metadata(
    event: &LambdaEvent<Value>,
    function_name: &str,
) -> LambdaInvocationMetadata {
    let payload_bytes = serde_json::to_vec(&event.payload)
        .ok()
        .and_then(|bytes| u64::try_from(bytes.len()).ok())
        .unwrap_or(u64::MAX);
    LambdaInvocationMetadata {
        invocation_id: event.context.request_id.clone(),
        trace_id: None,
        function_name: function_name.to_owned(),
        trigger: LambdaInvocationTrigger::Direct,
        payload_bytes,
        deadline_unix_ms: Some(event.context.deadline),
    }
}

pub fn from_event(event: LambdaEvent<Value>) -> Receipt {
    // Preserve the functional fresh-value construction from the adapter boundary, then bind the
    // host context again in the common runtime path so request-id validation/normalization is shared.
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
