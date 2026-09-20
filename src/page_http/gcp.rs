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

const MAX_HEADER_COUNT: usize = 128;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_HEADER_VALUE_BYTES: usize = 16 * 1024;
const MAX_COOKIE_COUNT: usize = 64;
const MAX_COOKIE_BYTES: usize = 4096;
const MAX_COOKIE_TOTAL_BYTES: usize = 32 * 1024;
const MAX_RAW_PATH_BYTES: usize = 8192;
const MAX_RAW_QUERY_BYTES: usize = 16 * 1024;

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
    if parts.uri.path().len() > MAX_RAW_PATH_BYTES
        || parts
            .uri
            .query()
            .is_some_and(|query| query.len() > MAX_RAW_QUERY_BYTES)
    {
        return Err(text_response(414, "request target too long"));
    }
    if parts.headers.len() > MAX_HEADER_COUNT {
        return Err(text_response(431, "request headers too large"));
    }

    let mut total_header_bytes = 0usize;
    let mut headers = BTreeMap::new();
    for (name, value) in &parts.headers {
        let Ok(value) = value.to_str() else {
            return Err(text_response(400, "invalid request headers"));
        };
        if value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(text_response(431, "request headers too large"));
        }
        total_header_bytes = total_header_bytes
            .checked_add(name.as_str().len())
            .and_then(|total| total.checked_add(value.len()))
            .ok_or_else(|| text_response(431, "request headers too large"))?;
        if total_header_bytes > MAX_HEADER_BYTES {
            return Err(text_response(431, "request headers too large"));
        }
        if normalize_header(&mut headers, name.as_str(), value).is_err() {
            return Err(text_response(400, "invalid request headers"));
        }
    }
    let bytes = to_bytes(body, MAX_PAGE_BODY_BYTES)
        .await
        .map_err(|_| text_response(413, "request body too large"))?;
    let cookies = parse_cookie_headers(headers.remove("cookie"))
        .map_err(|error| text_response(400, error.message()))?;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CookieHeaderError {
    Invalid,
    TooMany,
}

impl CookieHeaderError {
    const fn message(self) -> &'static str {
        match self {
            Self::Invalid => "invalid request cookies",
            Self::TooMany => "too many request cookies",
        }
    }
}

fn parse_cookie_headers(values: Option<Vec<String>>) -> Result<Vec<String>, CookieHeaderError> {
    let Some(values) = values else {
        return Ok(Vec::new());
    };
    let mut total_cookie_bytes = 0usize;
    let mut cookies = Vec::new();
    for value in values {
        total_cookie_bytes = total_cookie_bytes
            .checked_add(value.len())
            .ok_or(CookieHeaderError::Invalid)?;
        if value.len() > MAX_COOKIE_BYTES
            || total_cookie_bytes > MAX_COOKIE_TOTAL_BYTES
            || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err(CookieHeaderError::Invalid);
        }
        for cookie in value
            .split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            cookies.push(cookie.to_owned());
            if cookies.len() > MAX_COOKIE_COUNT {
                return Err(CookieHeaderError::TooMany);
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

    #[tokio::test]
    async fn oversized_request_metadata_fails_before_page_handler() {
        let large_header = Request::builder()
            .method(Method::GET)
            .uri("/")
            .header("x-large", "x".repeat(MAX_HEADER_VALUE_BYTES + 1))
            .body(Body::empty())
            .unwrap();
        assert_eq!(from_request(large_header).await.unwrap_err().status(), 431);

        let long_path = Request::builder()
            .method(Method::GET)
            .uri(format!("/{}", "x".repeat(MAX_RAW_PATH_BYTES)))
            .body(Body::empty())
            .unwrap();
        assert_eq!(from_request(long_path).await.unwrap_err().status(), 414);

        let long_query = Request::builder()
            .method(Method::GET)
            .uri(format!("/?q={}", "x".repeat(MAX_RAW_QUERY_BYTES + 1)))
            .body(Body::empty())
            .unwrap();
        assert_eq!(from_request(long_query).await.unwrap_err().status(), 414);
    }

    #[test]
    fn cookie_parser_returns_small_typed_errors() {
        assert_eq!(
            parse_cookie_headers(Some(vec!["x".repeat(MAX_COOKIE_BYTES + 1)])),
            Err(CookieHeaderError::Invalid)
        );
        assert_eq!(
            parse_cookie_headers(Some(vec!["x=1;".repeat(MAX_COOKIE_COUNT + 1)])),
            Err(CookieHeaderError::TooMany)
        );
        assert_eq!(
            parse_cookie_headers(Some(vec!["x".repeat(MAX_COOKIE_TOTAL_BYTES + 1)])),
            Err(CookieHeaderError::Invalid)
        );
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
