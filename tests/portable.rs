#![cfg(feature = "portable")]
use __CRATE__::adapters::portable::run;
use __CRATE__::runtime::SCHEMA_VERSION;

#[test]
fn stdin_json_in_json_out() {
    let input = format!(r#"{{"provider":"local","requestId":"p-1","command":{{"schemaVersion":"{SCHEMA_VERSION}","operation":"echo","payload":{{"k":"v"}}}}}}"#);
    let mut out = Vec::new();
    let receipt = run(input.as_bytes(), &mut out).unwrap();
    assert!(receipt.ok);
    let text = String::from_utf8(out).unwrap();
    assert!(text.ends_with('\n'));
    assert!(text.contains(r#""k":"v""#));
}
