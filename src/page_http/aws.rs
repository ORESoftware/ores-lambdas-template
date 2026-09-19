use super::{
    normalize_header, IngressProvenance, PageHttpMethod, PageHttpRequest, PageHttpResponse,
    RuntimeError, MAX_PAGE_BODY_BYTES,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use lambda_runtime::{service_fn, LambdaEvent};
use ores_api_docs_client::PageState;
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, future::Future};

const MAX_BASE64_BODY_BYTES: usize = MAX_PAGE_BODY_BYTES.div_ceil(3) * 4;
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
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| {
        let state = state.clone();
        let handler = handler.clone();
        async move {
            let request = match from_event(event) {
                Ok(request) => request,
                Err(response) => return Ok::<Value, lambda_runtime::Error>(to_event(response)),
            };
            let response = handler(state, request)
                .await
                .map_err(|error| -> lambda_runtime::Error { Box::new(error) })?;
            Ok::<Value, lambda_runtime::Error>(to_event(response))
        }
    }))
    .await
    .map_err(|error| RuntimeError::provider("aws", error))
}

fn from_event(event: LambdaEvent<Value>) -> Result<PageHttpRequest, PageHttpResponse> {
    let request_id = event.context.request_id;
    let value = event.payload;
    let method = match value
        .pointer("/requestContext/http/method")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "GET" => PageHttpMethod::Get,
        "HEAD" => PageHttpMethod::Head,
        _ => PageHttpMethod::Unsupported,
    };
    let raw_path = value.get("rawPath").and_then(Value::as_str).unwrap_or("");
    if raw_path.len() > MAX_RAW_PATH_BYTES {
        return Err(PageHttpResponse::text(414, "request path too long"));
    }
    let raw_query = value
        .get("rawQueryString")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if raw_query
        .as_ref()
        .is_some_and(|query| query.len() > MAX_RAW_QUERY_BYTES)
    {
        return Err(PageHttpResponse::text(414, "request query too long"));
    }

    let mut headers = BTreeMap::new();
    if let Some(object) = value.get("headers").and_then(Value::as_object) {
        if object.len() > MAX_HEADER_COUNT {
            return Err(PageHttpResponse::text(431, "request headers too large"));
        }
        let mut total_header_bytes = 0usize;
        for (name, value) in object {
            let Some(value) = value.as_str() else {
                return Err(PageHttpResponse::text(400, "invalid request headers"));
            };
            if value.len() > MAX_HEADER_VALUE_BYTES {
                return Err(PageHttpResponse::text(431, "request headers too large"));
            }
            total_header_bytes = total_header_bytes
                .checked_add(name.len())
                .and_then(|total| total.checked_add(value.len()))
                .ok_or_else(|| PageHttpResponse::text(431, "request headers too large"))?;
            if total_header_bytes > MAX_HEADER_BYTES {
                return Err(PageHttpResponse::text(431, "request headers too large"));
            }
            if normalize_header(&mut headers, name, value).is_err() {
                return Err(PageHttpResponse::text(400, "invalid request headers"));
            }
        }
    }

    let cookies = parse_cookies(value.get("cookies"))?;
    let encoded = value
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let base64_encoded = value
        .get("isBase64Encoded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let body = if base64_encoded {
        if encoded.len() > MAX_BASE64_BODY_BYTES {
            return Err(PageHttpResponse::text(413, "request body too large"));
        }
        STANDARD
            .decode(encoded)
            .map_err(|_| PageHttpResponse::text(400, "invalid request body"))?
    } else {
        if encoded.len() > MAX_PAGE_BODY_BYTES {
            return Err(PageHttpResponse::text(413, "request body too large"));
        }
        encoded.as_bytes().to_vec()
    };
    if body.len() > MAX_PAGE_BODY_BYTES {
        return Err(PageHttpResponse::text(413, "request body too large"));
    }

    Ok(PageHttpRequest {
        method,
        raw_path: raw_path.to_owned(),
        raw_query,
        headers,
        cookies,
        body,
        provenance: IngressProvenance::provider("aws", request_id),
    })
}

fn parse_cookies(value: Option<&Value>) -> Result<Vec<String>, PageHttpResponse> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    if items.len() > MAX_COOKIE_COUNT {
        return Err(PageHttpResponse::text(400, "too many request cookies"));
    }
    let mut total_cookie_bytes = 0usize;
    let mut cookies = Vec::with_capacity(items.len());
    for item in items {
        let Some(cookie) = item.as_str() else {
            return Err(PageHttpResponse::text(400, "invalid request cookies"));
        };
        total_cookie_bytes = total_cookie_bytes
            .checked_add(cookie.len())
            .ok_or_else(|| PageHttpResponse::text(400, "request cookies too large"))?;
        if cookie.len() > MAX_COOKIE_BYTES
            || total_cookie_bytes > MAX_COOKIE_TOTAL_BYTES
            || cookie.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err(PageHttpResponse::text(400, "invalid request cookies"));
        }
        cookies.push(cookie.to_owned());
    }
    Ok(cookies)
}

fn to_event(response: PageHttpResponse) -> Value {
    let mut grouped = BTreeMap::<String, Vec<String>>::new();
    for (name, value) in response.headers {
        grouped.entry(name).or_default().push(value);
    }
    let mut headers = Map::new();
    for (name, values) in grouped {
        headers.insert(name, Value::String(values.join(",")));
    }
    json!({
        "statusCode": response.status,
        "headers": headers,
        "cookies": response.set_cookies,
        "body": STANDARD.encode(response.body),
        "isBase64Encoded": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambda_runtime::Context;

    fn event(payload: Value) -> LambdaEvent<Value> {
        let mut context = Context::default();
        context.request_id = "lambda-request".to_owned();
        LambdaEvent::new(payload, context)
    }

    #[test]
    fn provider_context_not_headers_supplies_request_id() {
        let request = from_event(event(json!({
            "version": "2.0",
            "rawPath": "/",
            "requestContext": { "http": { "method": "GET" } },
            "headers": { "x-amzn-requestid": "attacker" }
        })))
        .unwrap();
        assert_eq!(request.provenance.request_id(), "lambda-request");
    }

    #[test]
    fn invalid_base64_and_oversized_encoded_body_fail_before_handler() {
        let invalid = from_event(event(json!({
            "rawPath": "/",
            "requestContext": { "http": { "method": "GET" } },
            "body": "%%%",
            "isBase64Encoded": true
        })))
        .unwrap_err();
        assert_eq!(invalid.status, 400);

        let oversized = from_event(event(json!({
            "rawPath": "/",
            "requestContext": { "http": { "method": "GET" } },
            "body": "A".repeat(MAX_BASE64_BODY_BYTES + 1),
            "isBase64Encoded": true
        })))
        .unwrap_err();
        assert_eq!(oversized.status, 413);
    }

    #[test]
    fn oversized_headers_path_query_and_cookies_fail_before_handler() {
        let headers = (0..=MAX_HEADER_COUNT)
            .map(|index| (format!("x-{index}"), Value::String("v".to_owned())))
            .collect::<Map<_, _>>();
        let too_many_headers = from_event(event(json!({
            "rawPath": "/",
            "requestContext": { "http": { "method": "GET" } },
            "headers": headers
        })))
        .unwrap_err();
        assert_eq!(too_many_headers.status, 431);

        let long_path = from_event(event(json!({
            "rawPath": format!("/{}", "x".repeat(MAX_RAW_PATH_BYTES)),
            "requestContext": { "http": { "method": "GET" } }
        })))
        .unwrap_err();
        assert_eq!(long_path.status, 414);

        let long_query = from_event(event(json!({
            "rawPath": "/",
            "rawQueryString": "x".repeat(MAX_RAW_QUERY_BYTES + 1),
            "requestContext": { "http": { "method": "GET" } }
        })))
        .unwrap_err();
        assert_eq!(long_query.status, 414);

        let cookies = vec!["x".repeat(MAX_COOKIE_BYTES); MAX_COOKIE_COUNT];
        let too_many_cookie_bytes = from_event(event(json!({
            "rawPath": "/",
            "cookies": cookies,
            "requestContext": { "http": { "method": "GET" } }
        })))
        .unwrap_err();
        assert_eq!(too_many_cookie_bytes.status, 400);
    }

    #[test]
    fn set_cookie_is_never_comma_folded() {
        let event = to_event(PageHttpResponse {
            status: 200,
            headers: vec![("content-type".to_owned(), "text/html".to_owned())],
            set_cookies: vec![
                "a=1; Path=/".to_owned(),
                "b=2; Expires=Wed, 21 Oct 2030 07:28:00 GMT".to_owned(),
            ],
            body: b"ok".to_vec(),
        });
        assert_eq!(event["cookies"].as_array().unwrap().len(), 2);
    }
}
