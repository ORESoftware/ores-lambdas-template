use super::{
    normalize_header, IngressProvenance, PageHttpMethod, PageHttpRequest, PageHttpResponse,
    RuntimeError, ERROR_CONTENT_TYPE, MAX_PAGE_BODY_BYTES,
};
use axum::{
    body::{to_bytes, Body},
    http::{HeaderName, HeaderValue, Request, Response, StatusCode},
    routing::any,
    Router,
};
use ores_api_docs_client::PageState;
use std::{collections::BTreeMap, future::Future};

const MAX_COOKIE_COUNT: usize = 64;
const MAX_COOKIE_BYTES: usize = 4096;

pub async fn run_page<H, Fut>(state: PageState, handler: H) -> Result<(), RuntimeError>
where
    H: Fn(PageState, PageHttpRequest) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<PageHttpResponse, RuntimeError>> + Send + 'static,
{
    let app = Router::new().fallback(any(move |request: Request<Body>| {
        let state = state.clone();
        let handler = handler.clone();
        async move {
            match from_request(request).await {
                Ok(request) => match handler(state, request).await {
                    Ok(response) => to_response(response),
                    Err(_) => stable_internal_error(),
                },
                Err(response) => response,
            }
        }
    }));
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_owned());
    let address = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|error| RuntimeError::provider("gcp", error))?;
    axum::serve(listener, app)
        .await
        .map_err(|error| RuntimeError::provider("gcp", error))
}

async fn from_request(request: Request<Body>) -> Result<PageHttpRequest, Response<Body>> {
    let (parts, body) = request.into_parts();
    let method = match parts.method.as_str() {
        "GET" => PageHttpMethod::Get,
        "HEAD" => PageHttpMethod::Head,
        _ => PageHttpMethod::Unsupported,
    };
    let mut headers = BTreeMap::new();
    for (name, value) in &parts.headers {
        let Ok(value) = value.to_str() else {
            return Err(text_response(400, "invalid request headers"));
        };
        if normalize_header(&mut headers, name.as_str(), value).is_err() {
            return Err(text_response(400, "invalid request headers"));
        }
    }
    let bytes = to_bytes(body, MAX_PAGE_BODY_BYTES)
        .await
        .map_err(|_| text_response(413, "request body too large"))?;
    let cookies = parse_cookie_headers(headers.remove("cookie"))?;

    // Generic Cloud Run HTTP headers are client-controllable. Until the hosting
    // layer passes authenticated platform metadata out-of-band, do not promote
    // x-cloud-trace-context/x-forwarded-* into trusted provenance.
    let request_id = "request";
    Ok(PageHttpRequest {
        method,
        raw_path: parts.uri.path().to_owned(),
        raw_query: parts.uri.query().map(ToOwned::to_owned),
        headers,
        cookies,
        body: bytes.to_vec(),
        provenance: IngressProvenance::provider("gcp", request_id),
    })
}

fn parse_cookie_headers(values: Option<Vec<String>>) -> Result<Vec<String>, Response<Body>> {
    let Some(values) = values else {
        return Ok(Vec::new());
    };
    let mut cookies = Vec::new();
    for value in values {
        if value.len() > MAX_COOKIE_BYTES
            || value
                .bytes()
                .any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err(text_response(400, "invalid request cookies"));
        }
        for cookie in value.split(';').map(str::trim).filter(|value| !value.is_empty()) {
            cookies.push(cookie.to_owned());
            if cookies.len() > MAX_COOKIE_COUNT {
                return Err(text_response(400, "too many request cookies"));
            }
        }
    }
    Ok(cookies)
}

fn to_response(response: PageHttpResponse) -> Response<Body> {
    let mut builder = Response::builder().status(response.status);
    if let Some(headers) = builder.headers_mut() {
        for (name, value) in response.headers {
            let Ok(name) = HeaderName::try_from(name) else {
                return stable_internal_error();
            };
            let Ok(value) = HeaderValue::try_from(value) else {
                return stable_internal_error();
            };
            headers.append(name, value);
        }
        for cookie in response.set_cookies {
            let Ok(value) = HeaderValue::try_from(cookie) else {
                return stable_internal_error();
            };
            headers.append(axum::http::header::SET_COOKIE, value);
        }
    }
    builder
        .body(Body::from(response.body))
        .unwrap_or_else(|_| stable_internal_error())
}

fn text_response(status: u16, message: &'static str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        .header("content-type", ERROR_CONTENT_TYPE)
        .header("cache-control", "no-store")
        .body(Body::from(message))
        .unwrap_or_else(|_| stable_internal_error())
}

fn stable_internal_error() -> Response<Body> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header("content-type", ERROR_CONTENT_TYPE)
        .header("cache-control", "no-store")
        .body(Body::from("page host failed"))
        .expect("static error response is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    #[tokio::test]
    async fn client_trace_header_never_becomes_trusted_provenance() {
        let request = Request::builder()
            .method(Method::GET)
            .uri("/users/42")
            .header("x-cloud-trace-context", "attacker-controlled")
            .body(Body::empty())
            .unwrap();
        let normalized = from_request(request).await.unwrap();
        assert_eq!(normalized.provenance.request_id(), "request");
        assert_eq!(
            normalized.headers["x-cloud-trace-context"],
            vec!["attacker-controlled".to_owned()]
        );
    }

    #[tokio::test]
    async fn unsupported_method_is_normalized_for_shared_405_admission() {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let normalized = from_request(request).await.unwrap();
        assert_eq!(normalized.method, PageHttpMethod::Unsupported);
    }

    #[tokio::test]
    async fn cookie_header_is_removed_from_generic_headers_and_split() {
        let request = Request::builder()
            .method(Method::GET)
            .uri("/")
            .header("cookie", "a=1; b=2")
            .body(Body::empty())
            .unwrap();
        let normalized = from_request(request).await.unwrap();
        assert!(!normalized.headers.contains_key("cookie"));
        assert_eq!(normalized.cookies, vec!["a=1", "b=2"]);
    }

    #[test]
    fn repeated_set_cookie_headers_stay_repeated() {
        let response = to_response(PageHttpResponse {
            status: 200,
            headers: vec![],
            set_cookies: vec![
                "a=1; Path=/".to_owned(),
                "b=2; Expires=Wed, 21 Oct 2030 07:28:00 GMT".to_owned(),
            ],
            body: Vec::new(),
        });
        assert_eq!(
            response
                .headers()
                .get_all(axum::http::header::SET_COOKIE)
                .iter()
                .count(),
            2
        );
    }
}
