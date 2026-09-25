//! HTTP listener binary for Cloud Run / Azure custom handler / Scintilla / local.
//! flags-2-env owns the argv boundary (.cli-flags.toml); PORT and FUNCTIONS_CUSTOMHANDLER_PORT are
//! the platforms' contracts and are honoured through that contract, not read ad hoc.
use flags2env::BundledFlags2Env;
use ores_middleware::{admit_server_stack_from_env, frameworks::axum::install_from_env};
use std::{
    collections::HashMap,
    fs, io,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use __CRATE__::adapters::http::{detect_provider, router, HttpConfig};

const CONTRACT_FILE: &str = ".cli-flags.toml";
const CONTRACT_OVERRIDE_ENV: &str = "ORES_LAMBDAS_FLAGS_CONFIG";
const POSITIONALS_ENV: &str = "ORES_LAMBDAS_POSITIONALS";
const UNKNOWN_OPTIONS_ENV: &str = "ORES_LAMBDAS_UNKNOWN_OPTIONS";
const PARSE_ERRORS_ENV: &str = "ORES_LAMBDAS_PARSE_ERRORS";
const INSTALL_SHARE_DIR: &str = "ores-lambdas";
const MIDDLEWARE_STACK_CONFIG: &str = "config/ores-middleware.stack.json";
const MIDDLEWARE_TARGET: &str = "portable-adapters";

#[derive(Debug, serde::Deserialize)]
struct Config {
    #[serde(rename = "PORT")]
    port: Option<u16>,
    #[serde(rename = "FUNCTIONS_CUSTOMHANDLER_PORT")]
    functions_customhandler_port: Option<u16>,
    #[serde(rename = "LAMBDAS_BIND")]
    bind: String,
}

fn admit_middleware_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let middleware_manifest = ".ores-mw.toml";
    let manifest_metadata = fs::symlink_metadata(middleware_manifest)?;
    if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
        return Err(format!(
            "middleware manifest must be a regular non-symlink file: {middleware_manifest}"
        )
        .into());
    }
    let manifest_path =
        admit_server_stack_from_env(Some(MIDDLEWARE_TARGET), MIDDLEWARE_STACK_CONFIG)?;
    let metadata = fs::symlink_metadata(MIDDLEWARE_STACK_CONFIG)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "middleware stack config must be a regular non-symlink file: {MIDDLEWARE_STACK_CONFIG}"
        )
        .into());
    }
    eprintln!(
        "__PREFIX__-lambdas admitted middleware target {MIDDLEWARE_TARGET} from {}",
        manifest_path.display()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parser = BundledFlags2Env::new();
    let contract = resolve_contract_path()?;
    let contract = contract
        .to_str()
        .ok_or_else(|| invalid_input("reviewed flags2env contract path is not valid UTF-8"))?;
    parser
        .audit_config(Some(contract))
        .map_err(|_| invalid_input("reviewed flags2env contract audit failed"))?;

    // Let flags2env read the process command line itself. Once this repository
    // adopts flags2env, application code must not retain a second argv parser.
    // The contract exposes only bounded JSON diagnostic channels; inspect the
    // parser-produced map before ambient environment values are merged so a
    // pre-existing environment variable cannot spoof an empty diagnostic.
    let parsed = parser
        .parse_process(Some(contract))
        .map_err(|_| invalid_input("flags2env process parsing failed"))?;
    let unknown_count = diagnostic_count(&parsed, UNKNOWN_OPTIONS_ENV)?;
    let parse_error_count = diagnostic_count(&parsed, PARSE_ERRORS_ENV)?;
    let positional_count = diagnostic_count(&parsed, POSITIONALS_ENV)?;
    if unknown_count != 0 || parse_error_count != 0 || positional_count != 0 {
        return Err(invalid_input(format!(
            "invalid arguments: {unknown_count} unknown option(s), {parse_error_count} parse error(s), {positional_count} positional extra(s)"
        ))
        .into());
    }

    // Keep provider discovery over the complete process environment while the
    // declared CLI/env keys are resolved by flags2env and override that ambient
    // snapshot according to the reviewed contract's precedence rules.
    let mut values: HashMap<String, String> = std::env::vars().collect();
    values.extend(parsed);
    let config: Config = parser
        .coerce(&values, Some(contract))
        .map_err(|_| invalid_input("flags2env typed coercion failed"))?;

    // The Lambda fleet contract requires middleware at the invocation boundary.
    // Admit the repository-owned .ores-mw.toml and its stack path before the
    // listener is bound so malformed/missing policy fails closed.
    admit_middleware_boundary()?;

    // Azure's custom-handler port takes precedence when present; Cloud Run sets PORT.
    let port = config
        .functions_customhandler_port
        .or(config.port)
        .unwrap_or(8080);
    // ores-compose host_bind supplies one full socket value per replica. Cloud providers commonly
    // supply host and port separately, so accept either representation without a second parser.
    let addr = if let Ok(socket) = config.bind.parse::<SocketAddr>() {
        socket
    } else {
        SocketAddr::new(config.bind.parse::<IpAddr>()?, port)
    };
    // Provider selection consumes the same final immutable environment map used for typed
    // flags2env coercion. Do not re-read ambient process state after CLI overrides are applied.
    let provider = detect_provider(|key| values.get(key).cloned());
    let app = install_from_env(router(HttpConfig { provider }), "__PREFIX__-lambdas")?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("__PREFIX__-lambdas worker-http listening on {addr} as {provider:?}");
    // The middleware's application rate-limit policy can use the peer IP as a
    // principal signal. Serve through Axum's connect-info make-service so every
    // request, including readiness probes, carries the actual accepted socket
    // address instead of failing closed with `rate_limit_principal_unavailable`.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

fn diagnostic_count(values: &HashMap<String, String>, key: &str) -> Result<usize, io::Error> {
    let Some(raw) = values.get(key) else {
        return Ok(0);
    };
    let entries: Vec<serde_json::Value> = serde_json::from_str(raw)
        .map_err(|_| invalid_input(format!("flags2env diagnostic channel {key} is invalid")))?;
    Ok(entries.len())
}

fn resolve_contract_path() -> Result<PathBuf, io::Error> {
    if let Some(explicit) =
        std::env::var_os(CONTRACT_OVERRIDE_ENV).filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(explicit);
        if !path.is_absolute() {
            return Err(invalid_input(format!(
                "{CONTRACT_OVERRIDE_ENV} must be an absolute path"
            )));
        }
        if !path.is_file() {
            return Err(invalid_input(format!(
                "{CONTRACT_OVERRIDE_ENV} does not name a readable regular file"
            )));
        }
        return path
            .canonicalize()
            .map_err(|_| invalid_input(format!("cannot resolve {CONTRACT_OVERRIDE_ENV}")));
    }

    if let Ok(executable) = std::env::current_exe() {
        if let Some(bin_dir) = executable.parent() {
            for candidate in [
                bin_dir
                    .join("..")
                    .join("share")
                    .join(INSTALL_SHARE_DIR)
                    .join(CONTRACT_FILE),
                bin_dir.join(CONTRACT_FILE),
            ] {
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CONTRACT_FILE);
    if source.is_file() {
        return Ok(source);
    }

    Err(invalid_input("reviewed flags2env contract is unavailable"))
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
