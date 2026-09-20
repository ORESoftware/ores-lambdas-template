//! HTTP listener binary for Cloud Run / Azure custom handler / Scintilla / local.
//! flags-2-env owns the argv boundary (.cli-flags.toml); PORT and FUNCTIONS_CUSTOMHANDLER_PORT are
//! the platforms' contracts and are honoured through that contract, not read ad hoc.
use flags2env::BundledFlags2Env;
use ores_middleware::{admit_server_stack_from_env, frameworks::axum::install_from_env};
use std::{
    collections::HashMap,
    fs,
    net::{IpAddr, SocketAddr},
};
use __CRATE__::adapters::http::{detect_provider, router, HttpConfig};

const CONTRACT: &str = ".cli-flags.toml";
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
    parser.audit_config(Some(CONTRACT))?;
    let argv: Vec<String> = std::env::args().collect();
    let parsed = parser.parse_structured(&argv, Some(CONTRACT))?;
    if !parsed.unknown_options.is_empty() || !parsed.errors.is_empty() {
        return Err(format!(
            "invalid arguments: {} unknown option(s), {} parse error(s)",
            parsed.unknown_options.len(),
            parsed.errors.len()
        )
        .into());
    }
    let mut values: HashMap<String, String> = std::env::vars().collect();
    values.extend(parsed.provided_flags);
    let config: Config = parser.coerce(&values, Some(CONTRACT))?;

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
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
