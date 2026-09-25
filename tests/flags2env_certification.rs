const WORKER: &str = include_str!("../src/bin/worker_http.rs");
const CONTRACT: &str = include_str!("../.cli-flags.toml");
const DOCKERFILE: &str = include_str!("../Dockerfile");
const CONTAINERFILE: &str = include_str!("../Containerfile");

#[test]
fn worker_uses_executable_owned_policy_and_absolute_override() {
    assert!(WORKER.contains("current_exe"));
    assert!(WORKER.contains("ORES_LAMBDAS_FLAGS_CONFIG"));
    assert!(WORKER.contains("must be an absolute path"));
    assert!(!WORKER.contains("const CONTRACT: &str = \".cli-flags.toml\""));
}

#[test]
fn diagnostics_are_value_safe_and_positionals_fail_closed() {
    assert!(CONTRACT.contains("allow_unknown = false"));
    assert!(WORKER.contains("parsed.extras.len()"));
    assert!(WORKER.contains("reviewed flags2env contract audit failed"));
    assert!(WORKER.contains("flags2env parsing failed"));
    assert!(WORKER.contains("flags2env typed coercion failed"));
    assert!(!WORKER.contains("{error}"));
}

#[test]
fn docker_and_oci_images_install_the_same_trusted_policy() {
    let path = "/usr/local/share/ores-lambdas/.cli-flags.toml";
    assert!(DOCKERFILE.contains(path));
    assert!(CONTAINERFILE.contains(path));
    assert_eq!(DOCKERFILE, CONTAINERFILE);
    assert!(!DOCKERFILE.contains("/app/.cli-flags.toml"));
}
