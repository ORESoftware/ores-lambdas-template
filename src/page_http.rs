//! Browser-page HTTP hosting for generated `*-web-server.rs` page Lambdas.
//!
//! The provider runtime owns bounded HTTP admission, route matching and provider
//! envelopes. The generated provider-neutral sibling `lambda.rs` owns page
//! invocation and shared response finalization through its `init_state()` / `run()`
//! ABI. Keeping those responsibilities separate prevents AWS/GCP wrappers from
//! re-implementing page behavior or asset finalization.

use ores_api_docs_client::{
    FinalizedPageResponse, PageContext, PageLambdaStateError, PageResponseRequestHints, PageState,
};
use std::{collections::BTreeMap, fmt};

pub const MAX_PAGE_BODY_BYTES: usize = 64 * 1024;
const ERROR_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
const MAX_PAGE_PATH_PATTERNS: usize = 4;

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

/// Provider-neutral input to the generated page module after HTTP admission.
///
/// Provider wrappers build request envelopes and call [`admit_page_request`].
/// If admitted, they pass `context` and `hints()` to generated `lambda.rs::run`,
/// then pass that finalized response to [`finish_page_response`].
#[derive(Debug, Clone)]
pub struct PageInvocation {
    pub context: PageContext,
    pub wasm_have: Option<String>,
    pub head: bool,
}

impl PageInvocation {
    #[must_use]
    pub fn hints(&self) -> PageResponseRequestHints<'_> {
        PageResponseRequestHints {
            wasm_have: self.wasm_have.as_deref(),
            dev_reload_script: None,
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

/// Admit one provider-normalized request for exactly one generated page module.
///
/// This function deliberately does **not** invoke `PageFn` or `PageFinalizeFn`.
/// Those are bound into generated server `lambda.rs::run`, so test and provider
/// builds execute the same page/finalizer authority.
pub fn admit_page_request(
    request: PageHttpRequest,
    state: PageState,
    axum_paths: &'static [&'static str],
) -> Result<PageInvocation, PageHttpResponse> {
    if request.method == PageHttpMethod::Unsupported {
        return Err(PageHttpResponse::text(405, "method not allowed"));
    }
    if request.body.len() > MAX_PAGE_BODY_BYTES {
        return Err(PageHttpResponse::text(413, "request body too large"));
    }
    if !request.body.is_empty() {
        return Err(PageHttpResponse::text(
            400,
            "GET/HEAD request body is not allowed",
        ));
    }
    if axum_paths.is_empty() || axum_paths.len() > MAX_PAGE_PATH_PATTERNS {
        return Err(PageHttpResponse::text(500, "page host failed"));
    }

    let params = match match_any_path(&request.raw_path, axum_paths) {
        Ok(Some(params)) => params,
        Ok(None) => return Err(PageHttpResponse::text(404, "page not found")),
        Err(PathMatchError::InvalidRequest) => {
            return Err(PageHttpResponse::text(400, "invalid request path"));
        }
        Err(PathMatchError::InvalidPattern) => {
            return Err(PageHttpResponse::text(500, "page host failed"));
        }
    };

    if ambiguous_sensitive_header(&request.headers) {
        return Err(PageHttpResponse::text(400, "ambiguous request headers"));
    }

    let wasm_have = single_header(&request.headers, "x-ores-wasm-have").map(ToOwned::to_owned);
    let mut context = PageContext::new(params, request.raw_path);
    context.state = state;
    Ok(PageInvocation {
        context,
        wasm_have,
        head: request.method == PageHttpMethod::Head,
    })
}

/// Convert the generated module's already-finalized page response into the
/// provider-neutral HTTP shape. Provider adapters perform only the final cloud
/// envelope conversion after this step.
#[must_use]
pub fn finish_page_response(
    finalized: FinalizedPageResponse,
    head: bool,
) -> PageHttpResponse {
    let mut headers = Vec::with_capacity(finalized.headers.len());
    let mut set_cookies = Vec::new();
    for (name, value) in finalized.headers {
        if name.eq_ignore_ascii_case("set-cookie") {
            set_cookies.push(value);
        } else {
            headers.push((name.to_ascii_lowercase(), value));
        }
    }
    PageHttpResponse {
        status: finalized.status,
        headers,
        set_cookies,
        body: if head { Vec::new() } else { finalized.body },
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathMatchError {
    InvalidRequest,
    InvalidPattern,
}

fn match_any_path(
    raw_path: &str,
    patterns: &[&str],
) -> Result<Option<BTreeMap<String, String>>, PathMatchError> {
    let request = decode_path_segments(raw_path)?;
    for pattern in patterns {
        if let Some(params) = match_pattern(&request, pattern)? {
            return Ok(Some(params));
        }
    }
    Ok(None)
}

fn decode_path_segments(raw_path: &str) -> Result<Vec<String>, PathMatchError> {
    if !raw_path.starts_with('/') || raw_path.contains('?') || raw_path.contains('#') {
        return Err(PathMatchError::InvalidRequest);
    }
    if raw_path == "/" {
        return Ok(Vec::new());
    }
    raw_path[1..]
        .split('/')
        .map(|segment| {
            if segment.is_empty() {
                return Err(PathMatchError::InvalidRequest);
            }
            let decoded = percent_decode_once(segment)?;
            if decoded.is_empty() || decoded == "." || decoded == ".." || decoded.contains('\0') {
                return Err(PathMatchError::InvalidRequest);
            }
            Ok(decoded)
        })
        .collect()
}

fn percent_decode_once(value: &str) -> Result<String, PathMatchError> {
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
            return Err(PathMatchError::InvalidRequest);
        }
        let high = hex(bytes[index + 1]).ok_or(PathMatchError::InvalidRequest)?;
        let low = hex(bytes[index + 2]).ok_or(PathMatchError::InvalidRequest)?;
        out.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(out).map_err(|_| PathMatchError::InvalidRequest)
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
) -> Result<Option<BTreeMap<String, String>>, PathMatchError> {
    if !pattern.starts_with('/') || pattern.contains('?') || pattern.contains('#') {
        return Err(PathMatchError::InvalidPattern);
    }
    let parts = if pattern == "/" {
        Vec::new()
    } else {
        pattern[1..].split('/').collect::<Vec<_>>()
    };
    let mut params = BTreeMap::new();
    let mut request_index = 0;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err(PathMatchError::InvalidPattern);
        }
        if let Some(name) = part
            .strip_prefix("{*")
            .and_then(|value| value.strip_suffix('}'))
        {
            if name.is_empty() || index + 1 != parts.len() {
                return Err(PathMatchError::InvalidPattern);
            }
            if request_index >= request.len() {
                return Ok(None);
            }
            params.insert(name.to_owned(), request[request_index..].join("/"));
            request_index = request.len();
            break;
        }
        let Some(actual) = request.get(request_index) else {
            return Ok(None);
        };
        if let Some(name) = part
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
        {
            if name.is_empty() || name.starts_with('*') {
                return Err(PathMatchError::InvalidPattern);
            }
            if params.insert(name.to_owned(), actual.clone()).is_some() {
                return Err(PathMatchError::InvalidPattern);
            }
        } else if part.contains('{') || part.contains('}') {
            return Err(PathMatchError::InvalidPattern);
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

    #[test]
    fn dynamic_path_decodes_once_and_preserves_encoded_slash_as_data() {
        let invocation = admit_page_request(
            request(PageHttpMethod::Get, "/users/a%2Fb%252Fc"),
            PageState::default(),
            &["/users/{id}"],
        )
        .unwrap();
        assert_eq!(
            invocation.context.route_params["id"],
            "a/b%2Fc",
            "encoded slash is page-param data and percent decoding occurs exactly once"
        );
    }

    #[test]
    fn malformed_or_dot_segments_fail_before_generated_module() {
        for path in ["/users/%", "/users/%2e%2e", "/users/%00"] {
            let response = admit_page_request(
                request(PageHttpMethod::Get, path),
                PageState::default(),
                &["/users/{id}"],
            )
            .unwrap_err();
            assert_eq!(response.status, 400, "{path}");
        }
    }

    #[test]
    fn malformed_generated_pattern_is_server_failure_not_client_blame() {
        let response = admit_page_request(
            request(PageHttpMethod::Get, "/users/42"),
            PageState::default(),
            &["/users/{id}/{id}"],
        )
        .unwrap_err();
        assert_eq!(response.status, 500);
        assert_eq!(response.body, b"page host failed");
    }

    #[test]
    fn unsupported_method_fails_before_generated_module() {
        let response = admit_page_request(
            request(PageHttpMethod::Unsupported, "/users/42"),
            PageState::default(),
            &["/users/{id}"],
        )
        .unwrap_err();
        assert_eq!(response.status, 405);
    }

    #[test]
    fn head_uses_same_finalized_response_then_drops_body() {
        let invocation = admit_page_request(
            request(PageHttpMethod::Head, "/users/42"),
            PageState::default(),
            &["/users/{id}"],
        )
        .unwrap();
        let response = finish_page_response(
            FinalizedPageResponse {
                status: 200,
                headers: vec![("content-type".to_owned(), "text/html".to_owned())],
                body: b"rendered".to_vec(),
            },
            invocation.head,
        );
        assert_eq!(response.status, 200);
        assert!(response.body.is_empty());
        assert_eq!(
            response.headers,
            vec![("content-type".to_owned(), "text/html".to_owned())]
        );
    }

    #[test]
    fn page_hints_are_client_hints_not_provider_identity() {
        let mut req = request(PageHttpMethod::Get, "/users/42");
        req.headers.insert(
            "x-ores-wasm-have".to_owned(),
            vec!["sha256-fixture".to_owned()],
        );
        let invocation =
            admit_page_request(req, PageState::default(), &["/users/{id}"]).unwrap();
        assert_eq!(invocation.hints().wasm_have, Some("sha256-fixture"));
        assert_eq!(invocation.hints().dev_reload_script, None);
    }

    #[test]
    fn coalesced_sensitive_header_is_rejected() {
        let mut req = request(PageHttpMethod::Get, "/users/42");
        req.headers
            .insert("authorization".to_owned(), vec!["a,b".to_owned()]);
        let response = admit_page_request(req, PageState::default(), &["/users/{id}"])
            .unwrap_err();
        assert_eq!(response.status, 400);
    }

    #[test]
    fn repeated_set_cookie_headers_stay_separate_after_finalization() {
        let response = finish_page_response(
            FinalizedPageResponse {
                status: 200,
                headers: vec![
                    ("set-cookie".to_owned(), "a=1; Path=/".to_owned()),
                    (
                        "Set-Cookie".to_owned(),
                        "b=2; Expires=Wed, 21 Oct 2030 07:28:00 GMT".to_owned(),
                    ),
                ],
                body: Vec::new(),
            },
            false,
        );
        assert_eq!(response.set_cookies.len(), 2);
        assert!(response.headers.is_empty());
    }

    #[test]
    fn provenance_has_no_public_constructor_and_exposes_only_validated_fields() {
        let provenance = IngressProvenance::provider("test", "request-1");
        assert_eq!(provenance.provider_name(), "test");
        assert_eq!(provenance.request_id(), "request-1");
    }
}
