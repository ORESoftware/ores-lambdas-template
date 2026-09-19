//! Browser-page HTTP hosting for generated `*-web-server.rs` page Lambdas.
//!
//! This module is intentionally independent of the command-envelope worker
//! runtime. It normalizes provider HTTP, matches exactly one admitted page
//! route, builds `PageContext`, invokes the typed page trampoline, and calls the
//! shared api-docs finalizer exactly once.

use ores_api_docs_client::{
    PageContext, PageFinalizeFn, PageFn, PageLambdaStateError, PageResponseRequestHints, PageState,
};
use std::{collections::BTreeMap, fmt};

pub const MAX_PAGE_BODY_BYTES: usize = 64 * 1024;
const ERROR_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageHttpMethod {
    Get,
    Head,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct IngressProvenance {
    provider: &'static str,
    request_id: String,
}

impl IngressProvenance {
    pub(crate) fn provider(provider: &'static str, request_id: impl Into<String>) -> Self {
        Self {
            provider,
            request_id: request_id.into(),
        }
    }

    #[must_use]
    pub fn provider_name(&self) -> &'static str {
        self.provider
    }

    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
}

#[derive(Debug, Clone)]
pub struct PageHttpRequest {
    pub method: PageHttpMethod,
    pub raw_path: String,
    pub raw_query: Option<String>,
    pub headers: BTreeMap<String, Vec<String>>,
    pub cookies: Vec<String>,
    pub body: Vec<u8>,
    pub provenance: IngressProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub set_cookies: Vec<String>,
    pub body: Vec<u8>,
}

impl PageHttpResponse {
    pub(crate) fn text(status: u16, message: &'static str) -> Self {
        Self {
            status,
            headers: vec![
                ("content-type".to_owned(), ERROR_CONTENT_TYPE.to_owned()),
                ("cache-control".to_owned(), "no-store".to_owned()),
            ],
            set_cookies: Vec::new(),
            body: message.as_bytes().to_vec(),
        }
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    StateInit(PageLambdaStateError),
    Provider {
        provider: &'static str,
        _message: String,
    },
}

impl RuntimeError {
    #[must_use]
    pub fn state_init(error: PageLambdaStateError) -> Self {
        Self::StateInit(error)
    }

    pub(crate) fn provider(provider: &'static str, error: impl fmt::Display) -> Self {
        Self::Provider {
            provider,
            _message: error.to_string(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StateInit(_) => formatter.write_str("page lambda state initialization failed"),
            Self::Provider { provider, .. } => write!(formatter, "{provider} page host failed"),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StateInit(error) => Some(error.as_ref()),
            Self::Provider { .. } => None,
        }
    }
}

/// Execute one already-admitted browser page.
///
/// Request rejection returns a stable HTTP response and never calls `page`.
/// Infrastructure/provider failures use [`RuntimeError`].
pub async fn invoke_page(
    request: PageHttpRequest,
    state: PageState,
    _canonical_route: &'static str,
    axum_paths: &'static [&'static str],
    page: PageFn,
    finalize: PageFinalizeFn,
) -> Result<PageHttpResponse, RuntimeError> {
    if request.method == PageHttpMethod::Unsupported {
        return Ok(PageHttpResponse::text(405, "method not allowed"));
    }
    if request.body.len() > MAX_PAGE_BODY_BYTES {
        return Ok(PageHttpResponse::text(413, "request body too large"));
    }
    if !request.body.is_empty() {
        return Ok(PageHttpResponse::text(
            400,
            "GET/HEAD request body is not allowed",
        ));
    }

    let params = match match_any_path(&request.raw_path, axum_paths) {
        Ok(Some(params)) => params,
        Ok(None) => return Ok(PageHttpResponse::text(404, "page not found")),
        Err(()) => return Ok(PageHttpResponse::text(400, "invalid request path")),
    };

    if ambiguous_sensitive_header(&request.headers) {
        return Ok(PageHttpResponse::text(400, "ambiguous request headers"));
    }

    let wasm_have = single_header(&request.headers, "x-ores-wasm-have");
    let mut context = PageContext::new(params, request.raw_path.clone());
    context.state = state;
    let rendered = page(context).await;
    let finalized = finalize(
        rendered,
        PageResponseRequestHints {
            wasm_have,
            dev_reload_script: None,
        },
    );

    let mut headers = Vec::with_capacity(finalized.headers.len());
    let mut set_cookies = Vec::new();
    for (name, value) in finalized.headers {
        if name.eq_ignore_ascii_case("set-cookie") {
            set_cookies.push(value);
        } else {
            headers.push((name.to_ascii_lowercase(), value));
        }
    }
    let body = if request.method == PageHttpMethod::Head {
        Vec::new()
    } else {
        finalized.body
    };
    Ok(PageHttpResponse {
        status: finalized.status,
        headers,
        set_cookies,
        body,
    })
}

fn ambiguous_sensitive_header(headers: &BTreeMap<String, Vec<String>>) -> bool {
    ["host", "authorization", "content-length"]
        .iter()
        .any(|name| {
            headers.get(*name).is_some_and(|values| {
                values.len() != 1 || values.first().is_some_and(|value| value.contains(','))
            })
        })
}

fn single_header<'a>(headers: &'a BTreeMap<String, Vec<String>>, name: &str) -> Option<&'a str> {
    let values = headers.get(name)?;
    (values.len() == 1).then(|| values[0].as_str())
}

pub(crate) fn normalize_header(
    headers: &mut BTreeMap<String, Vec<String>>,
    name: &str,
    value: &str,
) -> Result<(), ()> {
    let name = name.to_ascii_lowercase();
    if name.is_empty()
        || !name.bytes().all(http_token_byte)
        || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
    {
        return Err(());
    }
    headers.entry(name).or_default().push(value.to_owned());
    Ok(())
}

fn http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn match_any_path(
    raw_path: &str,
    patterns: &[&str],
) -> Result<Option<BTreeMap<String, String>>, ()> {
    let request = decode_path_segments(raw_path)?;
    for pattern in patterns {
        if let Some(params) = match_pattern(&request, pattern)? {
            return Ok(Some(params));
        }
    }
    Ok(None)
}

fn decode_path_segments(raw_path: &str) -> Result<Vec<String>, ()> {
    if !raw_path.starts_with('/') || raw_path.contains('?') || raw_path.contains('#') {
        return Err(());
    }
    if raw_path == "/" {
        return Ok(Vec::new());
    }
    raw_path[1..]
        .split('/')
        .map(|segment| {
            if segment.is_empty() {
                return Err(());
            }
            let decoded = percent_decode_once(segment)?;
            if decoded.is_empty() || decoded == "." || decoded == ".." || decoded.contains('\0') {
                return Err(());
            }
            Ok(decoded)
        })
        .collect()
}

fn percent_decode_once(value: &str) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(());
        }
        let high = hex(bytes[index + 1]).ok_or(())?;
        let low = hex(bytes[index + 2]).ok_or(())?;
        out.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(out).map_err(|_| ())
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn match_pattern(
    request: &[String],
    pattern: &str,
) -> Result<Option<BTreeMap<String, String>>, ()> {
    if !pattern.starts_with('/') {
        return Err(());
    }
    let parts = if pattern == "/" {
        Vec::new()
    } else {
        pattern[1..].split('/').collect::<Vec<_>>()
    };
    let mut params = BTreeMap::new();
    let mut request_index = 0;
    for (index, part) in parts.iter().enumerate() {
        if let Some(name) = part.strip_prefix("{*").and_then(|value| value.strip_suffix('}')) {
            if name.is_empty() || index + 1 != parts.len() || request_index >= request.len() {
                return Ok(None);
            }
            params.insert(name.to_owned(), request[request_index..].join("/"));
            request_index = request.len();
            break;
        }
        let Some(actual) = request.get(request_index) else {
            return Ok(None);
        };
        if let Some(name) = part.strip_prefix('{').and_then(|value| value.strip_suffix('}')) {
            if name.is_empty() || name.starts_with('*') {
                return Err(());
            }
            params.insert(name.to_owned(), actual.clone());
        } else if *part != actual {
            return Ok(None);
        }
        request_index += 1;
    }
    if request_index == request.len() {
        Ok(Some(params))
    } else {
        Ok(None)
    }
}

#[cfg(feature = "page-aws")]
pub mod aws;
#[cfg(feature = "page-gcp")]
pub mod gcp;

#[cfg(test)]
mod tests {
    use super::*;
    use ores_api_docs_client::{
        FinalizedPageResponse, PageDocument, PageFuture, PageResponseRequestHints,
    };

    fn page(context: PageContext) -> PageFuture {
        Box::pin(async move {
            let id = context.route_params.get("id").cloned().unwrap_or_default();
            Ok(PageDocument::html(format!("<p>{id}</p>")))
        })
    }

    fn finalize(
        result: ores_api_docs_client::PageResult,
        _hints: PageResponseRequestHints<'_>,
    ) -> FinalizedPageResponse {
        let document = result.expect("page result");
        FinalizedPageResponse {
            status: document.status,
            headers: vec![("content-type".to_owned(), "text/html".to_owned())],
            body: document.html.into_bytes(),
        }
    }

    fn request(method: PageHttpMethod, path: &str) -> PageHttpRequest {
        PageHttpRequest {
            method,
            raw_path: path.to_owned(),
            raw_query: None,
            headers: BTreeMap::new(),
            cookies: Vec::new(),
            body: Vec::new(),
            provenance: IngressProvenance::provider("test", "r1"),
        }
    }

    #[tokio::test]
    async fn dynamic_path_decodes_once_and_preserves_encoded_slash_as_data() {
        let response = invoke_page(
            request(PageHttpMethod::Get, "/users/a%2Fb%252Fc"),
            PageState::default(),
            "/users/{id}",
            &["/users/{id}"],
            page,
            finalize,
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"<p>a/b%2Fc</p>");
    }

    #[tokio::test]
    async fn malformed_or_dot_segments_fail_before_page_invocation() {
        for path in ["/users/%", "/users/%2e%2e", "/users/%00"] {
            let response = invoke_page(
                request(PageHttpMethod::Get, path),
                PageState::default(),
                "/users/{id}",
                &["/users/{id}"],
                page,
                finalize,
            )
            .await
            .unwrap();
            assert_eq!(response.status, 400, "{path}");
        }
    }

    #[tokio::test]
    async fn unsupported_method_fails_before_page_invocation() {
        let response = invoke_page(
            request(PageHttpMethod::Unsupported, "/users/42"),
            PageState::default(),
            "/users/{id}",
            &["/users/{id}"],
            page,
            finalize,
        )
        .await
        .unwrap();
        assert_eq!(response.status, 405);
    }

    #[tokio::test]
    async fn head_runs_same_page_and_finalizer_then_drops_body() {
        let response = invoke_page(
            request(PageHttpMethod::Head, "/users/42"),
            PageState::default(),
            "/users/{id}",
            &["/users/{id}"],
            page,
            finalize,
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.is_empty());
        assert_eq!(
            response.headers,
            vec![("content-type".to_owned(), "text/html".to_owned())]
        );
    }

    #[tokio::test]
    async fn coalesced_sensitive_header_is_rejected() {
        let mut request = request(PageHttpMethod::Get, "/users/42");
        request
            .headers
            .insert("authorization".to_owned(), vec!["a,b".to_owned()]);
        let response = invoke_page(
            request,
            PageState::default(),
            "/users/{id}",
            &["/users/{id}"],
            page,
            finalize,
        )
        .await
        .unwrap();
        assert_eq!(response.status, 400);
    }

    #[test]
    fn provenance_has_no_public_constructor_and_exposes_only_validated_fields() {
        let provenance = IngressProvenance::provider("test", "request-1");
        assert_eq!(provenance.provider_name(), "test");
        assert_eq!(provenance.request_id(), "request-1");
    }
}
