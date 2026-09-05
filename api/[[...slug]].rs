//! Vercel official Rust runtime entrypoint: every /api/* path forwards to the core envelope.
use __CRATE__::runtime::{handle, Provider};
use vercel_runtime::{run, Body, Error, Request, Response, StatusCode};

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(handler).await
}

pub async fn handler(req: Request) -> Result<Response<Body>, Error> {
    let request_id = req.headers().get("x-vercel-id").and_then(|v| v.to_str().ok()).unwrap_or("vercel").to_owned();
    let raw: &[u8] = match req.body() { Body::Binary(b) => b, Body::Text(t) => t.as_bytes(), Body::Empty => b"" };
    let receipt = handle(raw, Provider::Vercel, &request_id);
    let status = if receipt.ok { StatusCode::OK } else { StatusCode::BAD_REQUEST };
    Ok(Response::builder().status(status).header("content-type", "application/json").body(Body::Text(serde_json::to_string(&receipt)?))?)
}
