use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_RETRIES: u32 = 3;

/// Minimum gap between requests: 20ms ≈ 50 req/s, safely under the
/// account-wide 60 req/s API limit even with a little parallel activity.
const MIN_REQUEST_GAP: Duration = Duration::from_millis(20);

/// A non-2xx response from the Appcues API. Carried inside anyhow so
/// main.rs can downcast it to pick an exit code.
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    /// The parsed JSON error body, when the server sent one (the new
    /// analytics routes return structured 400/429 bodies).
    pub body: Option<Value>,
    /// Rate-limit headers on the failed response; same shape as
    /// `Response::rate_limit`.
    pub rate_limit: Option<Value>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API error {}: {}", self.status, self.message)
    }
}
impl std::error::Error for ApiError {}

/// A parsed response body plus the meta state its headers carried.
pub struct Response {
    pub value: Value,
    /// Present only when the response sent X-RateLimit-* / X-Concurrency-* /
    /// Retry-After headers (the analytics routes do; legacy routes don't).
    pub rate_limit: Option<Value>,
    /// True when the server clamped a sync result (X-Rows-Truncated: true).
    pub rows_truncated: bool,
}

/// base64("key:secret") — the credential value both auth schemes carry:
/// `Authorization: Basic <this>` on the API, `appcues-api-key: <this>`
/// (no prefix) on the tools routes.
pub fn api_key_credential(api_key: &str, api_secret: &str) -> String {
    B64.encode(format!("{api_key}:{api_secret}"))
}

pub struct Client {
    base_url: String,
    auth_header: &'static str,
    auth: String,
    agent: ureq::Agent,
    last_request: Mutex<Option<Instant>>,
}

impl Client {
    pub fn new(base_url: &str, api_key: &str, api_secret: &str) -> Self {
        Self::with_header_auth(
            base_url,
            "Authorization",
            format!("Basic {}", api_key_credential(api_key, api_secret)),
        )
    }

    /// A client that authenticates with an arbitrary header instead of
    /// Basic auth — the tools routes take `appcues-api-key`. Everything
    /// else (retry, throttle, error mapping) is identical.
    pub fn with_header_auth(base_url: &str, auth_header: &'static str, value: String) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!("appcues-cli/", env!("CARGO_PKG_VERSION")))
            .build();
        Client {
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_header,
            auth: value,
            agent: config.into(),
            last_request: Mutex::new(None),
        }
    }

    /// The API origin this client talks to (for display, e.g. auth status).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        Ok(self.request("GET", path, None)?.value)
    }
    pub fn get_with_meta(&self, path: &str) -> Result<Response> {
        self.request("GET", path, None)
    }
    pub fn post(&self, path: &str, body: Option<&Value>) -> Result<Value> {
        Ok(self.request("POST", path, body)?.value)
    }
    pub fn post_with_meta(&self, path: &str, body: Option<&Value>) -> Result<Response> {
        self.request("POST", path, body)
    }
    pub fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        Ok(self.request("PATCH", path, Some(body))?.value)
    }
    pub fn delete(&self, path: &str) -> Result<Value> {
        Ok(self.request("DELETE", path, None)?.value)
    }

    /// GET an absolute (presigned) URL, streaming the body into `w`.
    /// Deliberately sends no Authorization header — export result URLs are
    /// presigned and hosted outside the API; Basic creds must not leak there.
    pub fn download(&self, url: &str, w: &mut dyn std::io::Write) -> Result<u64> {
        self.throttle();
        let mut res = self
            .agent
            .get(url)
            // Export files can be arbitrarily large; the agent-wide 30s
            // timeout_global also covers body streaming and would abort a
            // healthy long download.
            .config()
            .timeout_global(None)
            // ureq has no idle timeout, so bound a stalled stream with a
            // generous body cap: 1h is multi-GB at modest speeds, far above
            // any real export, but stops an unattended run hanging forever.
            .timeout_recv_body(Some(Duration::from_secs(3600)))
            .build()
            .call()
            .with_context(|| format!("download from {url} failed"))?;
        let status = res.status().as_u16();
        if status >= 400 {
            return Err(ApiError {
                status,
                message: format!("download from {url} failed"),
                body: None,
                rate_limit: None,
            }
            .into());
        }
        let mut reader = res.body_mut().as_reader();
        std::io::copy(&mut reader, w).context("failed to stream download")
    }

    /// GET an API path with auth, streaming the raw body (e.g. a
    /// screenshots ZIP) into `w`. No JSON parsing on success; error
    /// responses become ApiError like every other API call.
    pub fn download_api(&self, path: &str, w: &mut dyn std::io::Write) -> Result<u64> {
        self.throttle();
        let url = format!("{}{}", self.base_url, path);
        let mut res = self
            .agent
            .get(&url)
            .header(self.auth_header, &self.auth)
            // Same rationale as `download`: the 30s global timeout also
            // covers body streaming; bound a stalled stream instead.
            .config()
            .timeout_global(None)
            .timeout_recv_body(Some(Duration::from_secs(3600)))
            .build()
            .call()
            .with_context(|| format!("request to {url} failed"))?;
        let status = res.status().as_u16();
        if status >= 400 {
            let rate_limit = rate_limit_json(res.headers());
            let text = res.body_mut().read_to_string().unwrap_or_default();
            return Err(ApiError {
                status,
                message: api_error_message(&text),
                body: serde_json::from_str(&text).ok(),
                rate_limit,
            }
            .into());
        }
        let mut reader = res.body_mut().as_reader();
        std::io::copy(&mut reader, w).context("failed to stream download")
    }

    fn throttle(&self) {
        let mut last = self.last_request.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(wait) = last.and_then(|prev| MIN_REQUEST_GAP.checked_sub(prev.elapsed())) {
            std::thread::sleep(wait);
        }
        *last = Some(Instant::now());
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        body: Option<&Value>,
    ) -> Result<ureq::http::Response<ureq::Body>> {
        self.throttle();
        let res = match (method, body) {
            ("GET", None) => self
                .agent
                .get(url)
                .header(self.auth_header, &self.auth)
                .call(),
            ("DELETE", None) => self
                .agent
                .delete(url)
                .header(self.auth_header, &self.auth)
                .call(),
            ("POST", Some(b)) => self
                .agent
                .post(url)
                .header(self.auth_header, &self.auth)
                .send_json(b),
            ("POST", None) => self
                .agent
                .post(url)
                .header(self.auth_header, &self.auth)
                .send_empty(),
            ("PATCH", Some(b)) => self
                .agent
                .patch(url)
                .header(self.auth_header, &self.auth)
                .send_json(b),
            _ => unreachable!("unsupported method/body combination: {method}"),
        };
        res.with_context(|| format!("request to {url} failed"))
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Response> {
        let url = format!("{}{}", self.base_url, path);
        for attempt in 0..=MAX_RETRIES {
            let mut res = self.send(method, &url, body)?;
            let status = res.status().as_u16();
            if (status == 429 || status >= 500) && attempt < MAX_RETRIES {
                let retry_after = res
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok());
                std::thread::sleep(Duration::from_secs(retry_wait_secs(attempt, retry_after)));
                continue;
            }
            let rate_limit = rate_limit_json(res.headers());
            let rows_truncated = res
                .headers()
                .get("x-rows-truncated")
                .and_then(|h| h.to_str().ok())
                .is_some_and(|v| v == "true");
            let text = res
                .body_mut()
                .read_to_string()
                .context("failed to read response body")?;
            if status >= 400 {
                return Err(ApiError {
                    status,
                    message: api_error_message(&text),
                    body: serde_json::from_str(&text).ok(),
                    rate_limit,
                }
                .into());
            }
            let value = if text.trim().is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap_or(Value::String(text))
            };
            return Ok(Response {
                value,
                rate_limit,
                rows_truncated,
            });
        }
        unreachable!("retry loop always returns or bails")
    }
}

/// Wait time before a retry, honoring the server's `Retry-After` header when present,
/// falling back to exponential backoff (2^attempt seconds). Capped at 60s so a
/// misbehaving server can't stall the CLI indefinitely.
fn retry_wait_secs(attempt: u32, retry_after: Option<&str>) -> u64 {
    retry_after
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1 << attempt)
        .min(60)
}

/// Collect rate-limit headers into a JSON object, or None when the
/// response carried none.
fn rate_limit_json(headers: &ureq::http::HeaderMap) -> Option<Value> {
    let mut map = serde_json::Map::new();
    for (header, key) in [
        ("x-ratelimit-limit", "limit"),
        ("x-ratelimit-remaining", "remaining"),
        ("x-ratelimit-reset", "reset"),
        ("x-concurrency-limit", "concurrency_limit"),
        ("x-concurrency-used", "concurrency_used"),
        ("retry-after", "retry_after"),
    ] {
        if let Some(v) = headers
            .get(header)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
        {
            map.insert(key.to_string(), v.into());
        }
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn api_error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        let title = v.get("title").and_then(Value::as_str).unwrap_or_default();
        let detail = v.get("detail").and_then(Value::as_str).unwrap_or_default();
        if !title.is_empty() || !detail.is_empty() {
            let sep = if !title.is_empty() && !detail.is_empty() {
                " — "
            } else {
                ""
            };
            return format!("{title}{sep}{detail}");
        }
    }
    body.chars().take(200).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    // base64("test-key:test-secret")
    const AUTH: &str = "Basic dGVzdC1rZXk6dGVzdC1zZWNyZXQ=";

    #[test]
    fn retry_wait_caps_a_huge_retry_after_at_60() {
        assert_eq!(retry_wait_secs(0, Some("999999999")), 60);
    }

    #[test]
    fn retry_wait_honors_a_reasonable_retry_after() {
        assert_eq!(retry_wait_secs(0, Some("5")), 5);
    }

    #[test]
    fn retry_wait_falls_back_to_exponential_backoff() {
        assert_eq!(retry_wait_secs(2, None), 4);
    }

    fn client(server: &MockServer) -> Client {
        Client::new(&server.base_url(), "test-key", "test-secret")
    }

    #[test]
    fn with_header_auth_sends_the_custom_header() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/v1/tools")
                .header("appcues-api-key", "dGVzdC1rZXk6dGVzdC1zZWNyZXQ=");
            then.status(200).json_body(json!({"tools": []}));
        });
        let c = Client::with_header_auth(
            &server.base_url(),
            "appcues-api-key",
            api_key_credential("test-key", "test-secret"),
        );
        let v = c.get("/v1/tools").unwrap();
        m.assert();
        assert!(v["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn get_sends_basic_auth_and_parses_json() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET)
                .path("/v2/accounts/acct1/tags")
                .header("authorization", AUTH);
            then.status(200)
                .json_body(json!([{"id": "t1", "name": "onboarding"}]));
        });
        let v = client(&server).get("/v2/accounts/acct1/tags").unwrap();
        m.assert();
        assert_eq!(v[0]["name"], "onboarding");
    }

    #[test]
    fn post_sends_json_body() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(POST)
                .path("/v2/accounts/acct1/segments")
                .header("authorization", AUTH)
                .json_body(json!({"name": "power users"}));
            then.status(200)
                .json_body(json!({"id": "s1", "name": "power users"}));
        });
        let v = client(&server)
            .post(
                "/v2/accounts/acct1/segments",
                Some(&json!({"name": "power users"})),
            )
            .unwrap();
        m.assert();
        assert_eq!(v["id"], "s1");
    }

    #[test]
    fn problem_details_become_readable_errors() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/accounts/acct1/flows/nope");
            then.status(404)
                .json_body(json!({"error": true, "status": 404, "title": "Not Found", "detail": "flow not found"}));
        });
        let err = client(&server)
            .get("/v2/accounts/acct1/flows/nope")
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("404") && msg.contains("flow not found"),
            "got: {msg}"
        );
    }

    #[test]
    fn retries_429_honoring_retry_after_then_gives_up() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(GET).path("/v2/accounts/acct1/flows");
            then.status(429)
                .header("Retry-After", "0")
                .body("slow down");
        });
        let err = client(&server).get("/v2/accounts/acct1/flows").unwrap_err();
        m.assert_hits(4); // 1 initial + 3 retries
        assert!(err.to_string().contains("429"));
    }

    #[test]
    fn no_retry_on_4xx() {
        let server = MockServer::start();
        let m = server.mock(|when, then| {
            when.method(DELETE).path("/v2/accounts/acct1/segments/s1");
            then.status(400).body("bad request");
        });
        let _ = client(&server)
            .delete("/v2/accounts/acct1/segments/s1")
            .unwrap_err();
        m.assert_hits(1);
    }

    #[test]
    fn api_errors_downcast_with_status() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/accounts/acct1/flows/nope");
            then.status(404)
                .json_body(json!({"error": true, "status": 404, "title": "Not Found", "detail": "flow not found"}));
        });
        let err = client(&server)
            .get("/v2/accounts/acct1/flows/nope")
            .unwrap_err();
        let api = err.downcast_ref::<ApiError>().expect("should be ApiError");
        assert_eq!(api.status, 404);
        assert!(api.message.contains("flow not found"));
    }

    #[test]
    fn empty_success_body_is_null() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/v2/accounts/acct1/flows/f1/publish");
            then.status(204);
        });
        let v = client(&server)
            .post("/v2/accounts/acct1/flows/f1/publish", None)
            .unwrap();
        assert_eq!(v, serde_json::Value::Null);
    }

    #[test]
    fn with_meta_captures_rate_limit_headers() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/v2/accounts/acct1/analytics/exports/j1");
            then.status(200)
                .header("X-RateLimit-Limit", "60")
                .header("X-RateLimit-Remaining", "41")
                .header("X-RateLimit-Reset", "1755750000")
                .header("X-Concurrency-Limit", "2")
                .header("X-Concurrency-Used", "1")
                .json_body(json!({"job_id": "j1", "status": "queued"}));
        });
        let res = client(&server)
            .get_with_meta("/v2/accounts/acct1/analytics/exports/j1")
            .unwrap();
        assert_eq!(res.value["status"], "queued");
        let rl = res.rate_limit.expect("headers should be captured");
        assert_eq!(rl["limit"], 60);
        assert_eq!(rl["remaining"], 41);
        assert_eq!(rl["reset"], 1755750000u64);
        assert_eq!(rl["concurrency_limit"], 2);
        assert_eq!(rl["concurrency_used"], 1);
    }

    #[test]
    fn with_meta_is_none_when_no_rate_limit_headers() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/accounts/acct1/jobs/j1");
            then.status(200).json_body(json!({"id": "j1"}));
        });
        let res = client(&server)
            .get_with_meta("/v2/accounts/acct1/jobs/j1")
            .unwrap();
        assert!(res.rate_limit.is_none());
        assert!(!res.rows_truncated);
    }

    #[test]
    fn with_meta_captures_rows_truncated() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v2/accounts/acct1/analytics/query");
            then.status(200)
                .header("X-Rows-Truncated", "true")
                .json_body(json!([{"day": "2026-07-01"}]));
        });
        let res = client(&server)
            .post_with_meta("/v2/accounts/acct1/analytics/query", Some(&json!({})))
            .unwrap();
        assert!(res.rows_truncated);
    }

    #[test]
    fn api_error_carries_parsed_body_and_rate_limit() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v2/accounts/acct1/analytics/query");
            then.status(400)
                .header("X-RateLimit-Remaining", "40")
                .json_body(json!({
                    "error": true, "status": 400,
                    "title": "invalid spec",
                    "detail": "start_time is required"
                }));
        });
        let err = client(&server)
            .post("/v2/accounts/acct1/analytics/query", Some(&json!({})))
            .unwrap_err();
        let api = err.downcast_ref::<ApiError>().expect("should be ApiError");
        assert_eq!(api.status, 400);
        let body = api.body.as_ref().expect("body should be parsed");
        assert_eq!(body["detail"], "start_time is required");
        assert_eq!(api.rate_limit.as_ref().unwrap()["remaining"], 40);
    }

    #[test]
    fn error_429_carries_retry_after_in_rate_limit() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/v2/accounts/acct1/analytics/exports/j1");
            then.status(429)
                .header("Retry-After", "0")
                .json_body(json!({"error": true, "title": "rate limited"}));
        });
        let err = client(&server)
            .get("/v2/accounts/acct1/analytics/exports/j1")
            .unwrap_err();
        let api = err.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api.rate_limit.as_ref().unwrap()["retry_after"], 0);
        assert_eq!(api.body.as_ref().unwrap()["title"], "rate limited");
    }

    #[test]
    fn non_json_error_body_yields_no_structured_body() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/accounts/acct1/flows/nope");
            then.status(404).body("plain text not found");
        });
        let err = client(&server)
            .get("/v2/accounts/acct1/flows/nope")
            .unwrap_err();
        let api = err.downcast_ref::<ApiError>().unwrap();
        assert!(api.body.is_none());
    }

    #[test]
    fn back_to_back_requests_are_throttled() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v2/ping");
            then.status(200).json_body(json!([]));
        });
        let c = client(&server);
        let start = std::time::Instant::now();
        for _ in 0..3 {
            c.get("/v2/ping").unwrap();
        }
        // 3 requests with a 20ms minimum gap → at least 40ms total
        assert!(
            start.elapsed() >= Duration::from_millis(40),
            "requests were not throttled"
        );
    }
}
