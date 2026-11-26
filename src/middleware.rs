use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use http::Extensions;
use rand::Rng;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result};

const RETRYABLE_STATUS: [u16; 3] = [502, 503, 504];
const MAX_RETRY_AFTER_SECONDS: u64 = 60;

fn warned() -> &'static Mutex<HashSet<String>> {
    static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn uuid_pattern() -> &'static regex::Regex {
    static UUID: OnceLock<regex::Regex> = OnceLock::new();
    UUID.get_or_init(|| regex::Regex::new("(?i)[0-9a-f]{8}-[0-9a-f-]{27}").unwrap())
}

/// The single HTTP path of the SDK: adds key and metadata, retries what the API reserves for retries (429 with
/// Retry-After, 502/503/504, no response at all) and warns once per deprecated operation. Error answers stay answers;
/// [`crate::Error`] turns them into typed errors.
pub(crate) struct SdkMiddleware {
    pub api_key: String,
    pub user_agent: String,
    pub sdk_header: String,
    pub max_retries: u32,
    pub deprecation_warnings: bool,
    pub logger: Arc<dyn Fn(&str) + Send + Sync>,
}

#[async_trait::async_trait]
impl Middleware for SdkMiddleware {
    async fn handle(&self, mut request: Request, extensions: &mut Extensions, next: Next<'_>) -> Result<Response> {
        let headers = request.headers_mut();
        headers.insert(http::header::AUTHORIZATION, format!("Bearer {}", self.api_key).parse().expect("api key is a valid header"));
        headers.insert(http::header::USER_AGENT, self.user_agent.parse().expect("user agent is a valid header"));
        headers.insert("TrustComponent-Sdk", self.sdk_header.parse().expect("sdk header is valid"));
        let mut attempt = 0;
        loop {
            let this_try = request.try_clone().expect("SDK requests have cloneable bodies");
            match next.clone().run(this_try, extensions).await {
                Err(error) => {
                    if attempt >= self.max_retries {
                        return Err(error);
                    }
                    tokio::time::sleep(backoff(attempt)).await;
                }
                Ok(response) => {
                    self.warn_if_deprecated(&request, &response);
                    let status = response.status().as_u16();
                    let retry_after = retry_after_seconds(&response);
                    let retryable = RETRYABLE_STATUS.contains(&status) || (status == 429 && retry_after.is_some());
                    if !retryable || attempt >= self.max_retries || retry_after.unwrap_or(0) > MAX_RETRY_AFTER_SECONDS {
                        return Ok(response);
                    }
                    tokio::time::sleep(retry_after.map(Duration::from_secs).unwrap_or_else(|| backoff(attempt))).await;
                }
            }
            attempt += 1;
        }
    }
}

impl SdkMiddleware {
    fn warn_if_deprecated(&self, request: &Request, response: &Response) {
        if !self.deprecation_warnings || !response.headers().contains_key("Deprecation") {
            return;
        }
        let operation = format!("{} {}", request.method(), uuid_pattern().replace_all(request.url().path(), "{id}"));
        if !warned().lock().map(|mut w| w.insert(operation.clone())).unwrap_or(false) {
            return;
        }
        let sunset = response.headers().get("Sunset").and_then(|v| v.to_str().ok()).map(|s| format!(" and will be switched off on {s}")).unwrap_or_default();
        (self.logger)(&format!("TrustComponent: {operation} is deprecated{sunset}. Update the TrustComponent SDK."));
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis((500u64 << attempt).min(8000) + rand::rng().random_range(0..250))
}

fn retry_after_seconds(response: &Response) -> Option<u64> {
    let value = response.headers().get("Retry-After")?.to_str().ok()?.trim().to_owned();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    let date = httpdate::parse_http_date(&value).ok()?;
    Some(date.duration_since(SystemTime::now()).map(|d| d.as_secs()).unwrap_or(0))
}
