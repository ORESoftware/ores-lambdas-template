//! HTTP listener binary for Cloud Run / Azure custom handler / Scintilla / local.
//! flags-2-env owns the argv boundary (.cli-flags.toml); PORT and FUNCTIONS_CUSTOMHANDLER_PORT are
//! the platforms' contracts and are honoured through that contract, not read ad hoc.
use flags2env::BundledFlags2Env;
use std::collections::HashMap;
use __CRATE__::adapters::http::{detect_provider, router, HttpConfig};

const CONTRACT: &str = ".cli-flags.toml";

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
    parser.audit_config(Some(CONTRACT))?;
    let argv: Vec<String> = std::env::args().collect();
    let parsed = parser.parse_structured(&argv, Some(CONTRACT))?;
    if !parsed.unknown_options.is_empty() || !parsed.errors.is_empty() {
        return Err(format!(
            "invalid arguments: unknown={:?} errors={:?}",
            parsed.unknown_options, parsed.errors
        )
        .into());
    }
    let mut values: HashMap<String, String> = std::env::vars().collect();
    values.extend(parsed.provided_flags);
    let config: Config = parser.coerce(&values, Some(CONTRACT))?;
    // Azure's custom-handler port takes precedence when present; Cloud Run sets PORT.
    let port = config
        .functions_customhandler_port
        .or(config.port)
        .unwrap_or(8080);
    let provider = detect_provider(|k| std::env::var(k).ok());
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
