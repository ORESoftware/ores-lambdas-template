//! HTTP listener binary for Cloud Run / Azure custom handler / Scintilla / local.
//! flags-2-env owns the argv boundary (.cli-flags.toml); PORT and FUNCTIONS_CUSTOMHANDLER_PORT are
//! the platforms' contracts and are honoured through that contract, not read ad hoc.
use flags2env::BundledFlags2Env;
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
};
use __CRATE__::adapters::http::{detect_provider, router, HttpConfig};

const CONTRACT_FILE: &str = ".cli-flags.toml";
const CONTRACT_OVERRIDE_ENV: &str = "ORES_LAMBDAS_FLAGS_CONFIG";
const INSTALL_SHARE_DIR: &str = "ores-lambdas";

#[derive(Debug, serde::Deserialize)]
struct Config {
    #[serde(rename = "PORT")]
    port: Option<u16>,
    #[serde(rename = "FUNCTIONS_CUSTOMHANDLER_PORT")]
    functions_customhandler_port: Option<u16>,
    #[serde(rename = "LAMBDAS_BIND")]
    bind: String,
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
    let argv: Vec<String> = std::env::args().collect();
    let parsed = parser
        .parse_structured(&argv, Some(contract))
        .map_err(|_| invalid_input("flags2env parsing failed"))?;
    if !parsed.unknown_options.is_empty() || !parsed.errors.is_empty() || !parsed.extras.is_empty() {
        return Err(invalid_input(format!(
            "invalid arguments: {} unknown option(s), {} parse error(s), {} positional extra(s)",
            parsed.unknown_options.len(),
            parsed.errors.len(),
            parsed.extras.len()
        ))
        .into());
    }
    let mut values: HashMap<String, String> = std::env::vars().collect();
    values.extend(parsed.provided_flags);
    let config: Config = parser
        .coerce(&values, Some(contract))
        .map_err(|_| invalid_input("flags2env typed coercion failed"))?;
    // Azure's custom-handler port takes precedence when present; Cloud Run sets PORT.
    let port = config
        .functions_customhandler_port
        .or(config.port)
        .unwrap_or(8080);
    // Provider selection consumes the same final immutable environment map used for typed
    // flags2env coercion. Do not re-read ambient process state after CLI overrides are applied.
    let provider = detect_provider(|key| values.get(key).cloned());
    let app = router(HttpConfig { provider });
    let addr = format!("{}:{port}", config.bind);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("__PREFIX__-lambdas worker-http listening on {addr} as {provider:?}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn resolve_contract_path() -> Result<PathBuf, io::Error> {
    if let Some(explicit) = std::env::var_os(CONTRACT_OVERRIDE_ENV).filter(|value| !value.is_empty()) {
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
