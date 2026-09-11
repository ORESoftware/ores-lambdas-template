use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

const MAX_ROOT_TOML_BYTES: u64 = 2 * 1024 * 1024;
const GITHUB_OWNER_PREFIX: &str = "https://github.com/";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parse_toml(path: &Path) -> Value {
    let metadata = fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("{} metadata failed: {error}", path.display()));
    assert!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "{} must be a regular non-symlink file",
        path.display()
    );
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} must be UTF-8 TOML: {error}", path.display()));
    text.parse::<Value>()
        .unwrap_or_else(|error| panic!("{} must parse as TOML: {error}", path.display()))
}

fn is_lower_hex_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn concern_by_id<'a>(concerns: &'a [Value], id: &str) -> &'a Value {
    concerns
        .iter()
        .find(|concern| concern.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("missing concern {id}"))
}

#[test]
fn root_toml_contracts_are_regular_bounded_and_parseable() {
    let root = repository_root();
    let mut parsed = BTreeSet::new();

    for entry in fs::read_dir(&root).expect("read repository root") {
        let entry = entry.expect("read repository-root entry");
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).expect("root TOML metadata");
        assert!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "root TOML must be a regular non-symlink file: {}",
            path.display()
        );
        assert!(
            metadata.len() <= MAX_ROOT_TOML_BYTES,
            "root TOML exceeds 2 MiB: {}",
            path.display()
        );
        parse_toml(&path);
        parsed.insert(
            path.file_name()
                .and_then(|name| name.to_str())
                .expect("UTF-8 root TOML filename")
                .to_owned(),
        );
    }

    assert!(parsed.contains(".cli-flags.toml"));
    assert!(parsed.contains(".ores-otel.toml"));
    assert!(
        !(parsed.contains(".shared-auth.toml") && parsed.contains(".auth-shared.toml")),
        "Shared Auth canonical and compatibility filenames must never coexist"
    );
}

#[test]
fn otel_baseline_remains_strict_secret_safe_and_flags2env_bound() {
    let root = repository_root();
    let otel = parse_toml(&root.join(".ores-otel.toml"));
    assert_eq!(otel.get("version").and_then(Value::as_integer), Some(1));
    assert_eq!(otel.get("mode").and_then(Value::as_str), Some("server"));
    assert_eq!(otel.get("strict").and_then(Value::as_bool), Some(true));

    let flags = otel
        .get("flags2env")
        .and_then(Value::as_table)
        .expect("OTEL flags2env table");
    assert_eq!(
        flags.get("contract").and_then(Value::as_str),
        Some(".cli-flags.toml")
    );
    assert_eq!(
        flags.get("require_audit").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        flags.get("precedence").and_then(Value::as_str),
        Some("argv-over-env")
    );

    let bindings = otel
        .get("env")
        .and_then(Value::as_array)
        .expect("OTEL env array");
    let mut names = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut headers_are_secret = false;
    for binding in bindings {
        let table = binding.as_table().expect("OTEL env binding table");
        let name = table
            .get("name")
            .and_then(Value::as_str)
            .expect("OTEL env binding name");
        let key = table
            .get("key")
            .and_then(Value::as_str)
            .expect("OTEL env binding key");
        assert!(
            names.insert(name),
            "duplicate OTEL env binding name: {name}"
        );
        assert!(keys.insert(key), "duplicate OTEL env binding key: {key}");

        if table.get("secret").and_then(Value::as_bool) == Some(true) {
            assert!(
                !table.contains_key("default") && !table.contains_key("default_value"),
                "secret OTEL binding may not carry a plaintext default: {key}"
            );
        }
        if key == "OTEL_EXPORTER_OTLP_HEADERS" {
            headers_are_secret = table.get("secret").and_then(Value::as_bool) == Some(true);
        }
    }
    assert!(headers_are_secret, "OTLP headers must remain secret-bound");
}

#[test]
fn concern_catalog_is_unique_pinned_and_fail_closed() {
    let root = repository_root();
    let catalog_path = root.join("config/concerns/catalog.toml");
    let catalog = parse_toml(&catalog_path);
    assert_eq!(
        catalog.get("schema_version").and_then(Value::as_integer),
        Some(1)
    );
    let concerns = catalog
        .get("concerns")
        .and_then(Value::as_array)
        .expect("concern catalog array");
    assert!(!concerns.is_empty(), "concern catalog must not be empty");

    let mut ids = BTreeSet::new();
    let mut filenames = BTreeSet::new();
    for concern in concerns {
        let table = concern.as_table().expect("concern table");
        let id = table.get("id").and_then(Value::as_str).expect("concern id");
        let canonical = table
            .get("canonical_file")
            .and_then(Value::as_str)
            .expect("canonical concern filename");
        let owner = table
            .get("owner")
            .and_then(Value::as_str)
            .expect("concern owner");
        let mode = table
            .get("mode")
            .and_then(Value::as_str)
            .expect("concern mode");
        let template = table
            .get("template")
            .and_then(Value::as_str)
            .expect("concern template field");

        assert!(!id.is_empty());
        assert!(ids.insert(id), "duplicate concern id: {id}");
        assert!(canonical.starts_with('.') && canonical.ends_with(".toml"));
        assert!(
            filenames.insert(canonical),
            "duplicate canonical concern filename: {canonical}"
        );
        assert!(
            owner.starts_with(GITHUB_OWNER_PREFIX),
            "invalid owner for {id}"
        );
        assert!(
            matches!(mode, "required" | "optional" | "owner-schema-required"),
            "unsupported concern mode {mode} for {id}"
        );

        if let Some(revision) = table.get("source_revision").and_then(Value::as_str) {
            assert!(
                is_lower_hex_revision(revision),
                "{id}: source_revision must be immutable lowercase 40-hex"
            );
        }

        if let Some(aliases) = table.get("aliases").and_then(Value::as_array) {
            for alias in aliases {
                let alias = alias.as_str().expect("concern alias string");
                assert!(alias.starts_with('.') && alias.ends_with(".toml"));
                assert!(filenames.insert(alias), "duplicate concern alias: {alias}");
            }
        }

        match mode {
            "optional" => {
                assert!(
                    !template.is_empty(),
                    "optional concern {id} needs a template"
                );
                parse_toml(&root.join(template));
            }
            "owner-schema-required" => assert!(
                template.is_empty(),
                "unadmitted concern {id} must not fabricate a template"
            ),
            "required" => {
                if !template.is_empty() {
                    parse_toml(&root.join(template));
                } else {
                    parse_toml(&root.join(canonical));
                }
            }
            _ => unreachable!("validated concern mode"),
        }

        if table.get("tjsv_required").and_then(Value::as_bool) == Some(true) {
            let revision = table
                .get("source_revision")
                .and_then(Value::as_str)
                .expect("TJSV concern source revision");
            assert!(is_lower_hex_revision(revision));
            let typespec = table
                .get("typespec")
                .and_then(Value::as_str)
                .expect("TJSV TypeSpec authority path");
            let schema = table
                .get("json_schema")
                .and_then(Value::as_str)
                .expect("TJSV JSON Schema authority path");
            assert!(typespec.starts_with("contracts/") && typespec.ends_with(".tsp"));
            assert!(schema.starts_with("contracts/") && schema.ends_with(".json"));
        }
    }

    let shared = concern_by_id(concerns, "shared-auth");
    assert_eq!(
        shared.get("canonical_file").and_then(Value::as_str),
        Some(".shared-auth.toml")
    );
    let aliases = shared
        .get("aliases")
        .and_then(Value::as_array)
        .expect("Shared Auth compatibility alias");
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0].as_str(), Some(".auth-shared.toml"));

    for id in ["rate-limit", "chat"] {
        assert_eq!(
            concern_by_id(concerns, id)
                .get("tjsv_required")
                .and_then(Value::as_bool),
            Some(true),
            "{id} must remain TJSV gated"
        );
    }
}

#[test]
fn reviewed_concern_templates_keep_security_defaults() {
    let root = repository_root();

    let shared = parse_toml(&root.join("config/concerns/templates/shared-auth.toml"));
    assert_eq!(
        shared
            .get("compatibility")
            .and_then(Value::as_table)
            .and_then(|table| table.get("repository"))
            .and_then(Value::as_str),
        Some("https://github.com/shared-auth/shared-auth-interfaces")
    );
    assert_eq!(
        shared
            .get("factors")
            .and_then(Value::as_table)
            .and_then(|table| table.get("two_factor"))
            .and_then(Value::as_table)
            .and_then(|table| table.get("required"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let chat = parse_toml(&root.join("config/concerns/templates/ores-chat.toml"));
    assert_eq!(chat.get("strict").and_then(Value::as_bool), Some(true));
    assert_eq!(
        chat.get("flags2env")
            .and_then(Value::as_table)
            .and_then(|table| table.get("contract"))
            .and_then(Value::as_str),
        Some(".cli-flags.toml")
    );

    let rate = parse_toml(&root.join("config/concerns/templates/ores-rl.toml"));
    let policies = rate
        .get("policies")
        .and_then(Value::as_array)
        .expect("rate-limit policies");
    assert!(!policies.is_empty());
    for policy in policies {
        assert_eq!(
            policy
                .as_table()
                .and_then(|table| table.get("backendFailureMode"))
                .and_then(Value::as_str),
            Some("fail-closed")
        );
    }
}
