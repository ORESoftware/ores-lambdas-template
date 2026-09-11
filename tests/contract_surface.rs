#[allow(dead_code)]
#[path = "../src/runtime.rs"]
mod runtime;

fn schema_enum(name: &str) -> Vec<String> {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema-authority/authored.schema.json"))
            .expect("authored lambda JSON Schema must parse");
    schema["$defs"][name]["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("missing enum contract for {name}"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("enum values must be strings")
                .to_owned()
        })
        .collect()
}

fn wire<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("runtime enum must serialize")
        .as_str()
        .expect("runtime enum wire value must be a string")
        .to_owned()
}

#[test]
fn rust_provider_values_match_peer_authority_contract() {
    let runtime_values = vec![
        wire(runtime::Provider::AwsLambda),
        wire(runtime::Provider::GcpCloudRun),
        wire(runtime::Provider::AzureFunctions),
        wire(runtime::Provider::Vercel),
        wire(runtime::Provider::CloudflareWorkers),
        wire(runtime::Provider::Scintilla),
        wire(runtime::Provider::Local),
    ];
    assert_eq!(runtime_values, schema_enum("Provider"));
}

#[test]
fn rust_operation_values_match_peer_authority_contract() {
    let runtime_values = vec![
        wire(runtime::Operation::Health),
        wire(runtime::Operation::Version),
        wire(runtime::Operation::Echo),
    ];
    assert_eq!(runtime_values, schema_enum("Operation"));
}
