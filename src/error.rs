use std::fmt;

use crate::generated::apis;

/// Every error the SDK returns. Match on it, or ask [`Error::api`] for the answer of the API.
#[derive(Debug)]
pub enum Error {
    /// The API answered with an error status.
    Api(ApiError),
    /// No response arrived: timeout, DNS, connection refused.
    Connection(String),
    /// The string is not a token of the TrustCaptcha widget.
    VerificationTokenInvalid,
    /// Anything else, e.g. an answer that could not be read.
    Other(String),
}

/// An error answer of the API. `code` is the stable reason the API names.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: Option<String>,
    pub detail: Option<String>,
}

impl ApiError {
    pub fn is_invalid_request(&self) -> bool { self.status == 400 || self.status == 422 }
    pub fn is_authentication(&self) -> bool { self.status == 401 }
    pub fn is_plan_limit(&self) -> bool { self.status == 402 }
    pub fn is_permission(&self) -> bool { self.status == 403 }
    pub fn is_not_found(&self) -> bool { self.status == 404 }
    pub fn is_conflict(&self) -> bool { matches!(self.status, 409 | 412 | 423) }
    pub fn is_gone(&self) -> bool { self.status == 410 && !self.is_operation_retired() }
    pub fn is_rate_limit(&self) -> bool { self.status == 429 }
    pub fn is_server(&self) -> bool { self.status >= 500 }
    /// The operation was switched off after its sunset; a newer SDK calls its successor.
    pub fn is_operation_retired(&self) -> bool { self.status == 410 && self.code.as_deref() == Some("operation_retired") }

    fn of(status: u16, content: &str) -> Self {
        let problem: serde_json::Value = serde_json::from_str(content).unwrap_or(serde_json::Value::Null);
        let text = |key: &str| problem.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        ApiError { status, code: text("code"), detail: text("detail") }
    }
}

impl Error {
    pub fn api(&self) -> Option<&ApiError> {
        match self {
            Error::Api(e) => Some(e),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Api(e) => {
                write!(f, "TrustComponent API answered {}", e.status)?;
                if let Some(code) = &e.code { write!(f, " ({code})")?; }
                if let Some(detail) = &e.detail { write!(f, ": {detail}")?; }
                if e.is_operation_retired() { write!(f, ". Update the TrustComponent SDK.")?; }
                Ok(())
            }
            Error::Connection(m) => write!(f, "TrustComponent API not reachable: {m}"),
            Error::VerificationTokenInvalid => write!(f, "The verification token is not a TrustCaptcha token"),
            Error::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl<T> From<apis::Error<T>> for Error {
    fn from(error: apis::Error<T>) -> Self {
        match error {
            apis::Error::ResponseError(content) => Error::Api(ApiError::of(content.status.as_u16(), &content.content)),
            apis::Error::Reqwest(e) if e.is_connect() || e.is_timeout() => Error::Connection(e.to_string()),
            apis::Error::ReqwestMiddleware(reqwest_middleware::Error::Reqwest(e)) if e.is_connect() || e.is_timeout() => Error::Connection(e.to_string()),
            other => Error::Other(other.to_string()),
        }
    }
}
