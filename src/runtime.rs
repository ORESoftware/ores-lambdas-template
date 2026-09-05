//! Bounded command envelope + dispatch. Pure: no I/O, no listeners, no globals.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Wire schema this core accepts. Bump only with a reviewed contract change.
pub const SCHEMA_VERSION: &str = "__PREFIX__.worker-command.v1";
/// Invocations larger than this are rejected before JSON parsing.
pub const MAX_INVOCATION_BYTES: usize = 256 * 1024;
const MAX_ID_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    AwsLambda,
    GcpCloudRun,
    AzureFunctions,
    Vercel,
    CloudflareWorkers,
    Scintilla,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Health,
    Version,
    Echo,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Command {
    pub schema_version: String,
    pub operation: Operation,
    #[serde(default)]
    pub payload: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Invocation {
    pub provider: Provider,
    pub request_id: String,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorReceipt {
    pub code: &'static str,
    /// Fixed strings only — never reflects submitted payload.
    pub message: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub request_id: String,
    pub provider: Provider,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    TooLarge,
    InvalidJson,
    UnsupportedSchema,
    UnboundedIdentifier,
    UnknownOperation,
}

impl RejectReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::TooLarge => "invocation_too_large",
            Self::InvalidJson => "invalid_invocation",
            Self::UnsupportedSchema => "unsupported_schema_version",
            Self::UnboundedIdentifier => "unbounded_identifier",
            Self::UnknownOperation => "unknown_operation",
        }
    }
    pub const fn message(self) -> &'static str {
        match self {
            Self::TooLarge => "invocation exceeds the size limit",
            Self::InvalidJson => "invocation is not a valid command envelope",
            Self::UnsupportedSchema => "command schema version is not supported",
            Self::UnboundedIdentifier => "request identifier is empty or too long",
            Self::UnknownOperation => "operation is not supported",
        }
    }
}

/// Parse and validate raw bytes into an invocation. Size check happens before parsing.
pub fn parse(raw: &[u8]) -> Result<Invocation, RejectReason> {
    if raw.len() > MAX_INVOCATION_BYTES {
        return Err(RejectReason::TooLarge);
    }
    let inv: Invocation = serde_json::from_slice(raw).map_err(|_| RejectReason::InvalidJson)?;
    validate(&inv)?;
    Ok(inv)
}

pub fn validate(inv: &Invocation) -> Result<(), RejectReason> {
    if inv.command.schema_version != SCHEMA_VERSION {
        return Err(RejectReason::UnsupportedSchema);
    }
    if inv.request_id.is_empty() || inv.request_id.len() > MAX_ID_LEN || !inv.request_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':' || b == b'.') {
        return Err(RejectReason::UnboundedIdentifier);
    }
    Ok(())
}

/// Execute an accepted invocation. Only here may provider clients / ORM resources be touched,
/// and only for the accepted operation (cold-start boundary).
pub fn dispatch(inv: &Invocation) -> Receipt {
    let result = match inv.command.operation {
        Operation::Health => serde_json::json!({ "status": "ok" }),
        Operation::Version => serde_json::json!({
            "package": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "schemaVersion": SCHEMA_VERSION,
        }),
        Operation::Echo => Value::Object(inv.command.payload.clone()),
    };
    Receipt { request_id: inv.request_id.clone(), provider: inv.provider, ok: true, operation: Some(inv.command.operation), result: Some(result), error: None }
}

/// Parse + dispatch, producing a receipt either way. `provider` and `request_id` are the
/// adapter's best knowledge for rejected envelopes (they may not be parseable).
pub fn handle(raw: &[u8], provider: Provider, fallback_request_id: &str) -> Receipt {
    match parse(raw) {
        Ok(inv) => dispatch(&inv),
        Err(reason) => Receipt {
            request_id: fallback_request_id.chars().take(MAX_ID_LEN).collect(),
            provider,
            ok: false,
            operation: None,
            result: None,
            error: Some(ErrorReceipt { code: reason.code(), message: reason.message() }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(op: &str) -> String {
        format!(r#"{{"provider":"local","requestId":"t-1","command":{{"schemaVersion":"{SCHEMA_VERSION}","operation":"{op}","payload":{{"a":1}}}}}}"#)
    }

    #[test]
    fn health_and_echo_dispatch() {
        let r = handle(envelope("health").as_bytes(), Provider::Local, "x");
        assert!(r.ok);
        assert_eq!(r.result.unwrap()["status"], "ok");
        let r = handle(envelope("echo").as_bytes(), Provider::Local, "x");
        assert_eq!(r.result.unwrap()["a"], 1);
        assert_eq!(r.request_id, "t-1");
    }

    #[test]
    fn rejects_fail_closed_with_fixed_messages() {
        let big = vec![b' '; MAX_INVOCATION_BYTES + 1];
        assert_eq!(handle(&big, Provider::AwsLambda, "req").error.unwrap().code, "invocation_too_large");
        assert_eq!(handle(b"{", Provider::AwsLambda, "req").error.unwrap().code, "invalid_invocation");
        let unknown_field = envelope("health").replace(r#""payload""#, r#""extra":1,"payload""#);
        assert_eq!(handle(unknown_field.as_bytes(), Provider::Local, "req").error.unwrap().code, "invalid_invocation");
        let bad_schema = envelope("health").replace(SCHEMA_VERSION, "v0");
        assert_eq!(handle(bad_schema.as_bytes(), Provider::Local, "req").error.unwrap().code, "unsupported_schema_version");
        let unknown_op = envelope("reboot");
        assert_eq!(handle(unknown_op.as_bytes(), Provider::Local, "req").error.unwrap().code, "invalid_invocation");
        let long_id = envelope("health").replace(r#""t-1""#, &format!("\"{}\"", "x".repeat(200)));
        assert_eq!(handle(long_id.as_bytes(), Provider::Local, "req").error.unwrap().code, "unbounded_identifier");
        let scalar_payload = envelope("health").replace(r#"{"a":1}"#, "1");
        assert_eq!(handle(scalar_payload.as_bytes(), Provider::Local, "req").error.unwrap().code, "invalid_invocation");
    }

    #[test]
    fn receipt_never_reflects_payload_on_error() {
        let r = handle(b"{\"provider\":\"local\",\"requestId\":\"\",\"command\":{\"schemaVersion\":\"x\",\"operation\":\"health\"}}", Provider::Local, "fallback-id");
        let text = serde_json::to_string(&r).unwrap();
        assert!(!text.contains("\"x\""));
        assert_eq!(r.request_id, "fallback-id");
    }
}
