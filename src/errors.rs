//! Stable error codes shared by the whole protocol.
use serde_json::{json, Value};
use std::fmt;

const STATUS: &[(&str, u16)] = &[
    ("bad_request", 400),
    ("unauthorized", 401),
    ("forbidden", 403),
    ("not_found", 404),
    ("conflict", 409),
    ("missing_dependency", 424),
    ("timeout", 504),
    ("unavailable", 503),
    ("handler_error", 500),
];

#[derive(Debug, Clone)]
pub struct DeltaError {
    pub code: String,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, DeltaError>;

impl DeltaError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        let code = if STATUS.iter().any(|(c, _)| *c == code) { code } else { "handler_error" };
        let message = message.into();
        Self {
            code: code.to_string(),
            message: if message.is_empty() { code.to_string() } else { message },
        }
    }

    pub fn status(&self) -> u16 {
        STATUS.iter().find(|(c, _)| *c == self.code).map(|(_, s)| *s).unwrap_or(500)
    }

    pub fn to_value(&self) -> Value {
        json!({"code": self.code, "message": self.message})
    }

    pub fn from_value(v: &Value) -> Self {
        let code = v.get("code").and_then(Value::as_str).unwrap_or("handler_error");
        let msg = v.get("message").and_then(Value::as_str).unwrap_or("");
        Self::new(code, msg)
    }

    pub fn bad_request(m: impl Into<String>) -> Self { Self::new("bad_request", m) }
    pub fn unauthorized(m: impl Into<String>) -> Self { Self::new("unauthorized", m) }
    pub fn forbidden(m: impl Into<String>) -> Self { Self::new("forbidden", m) }
    pub fn not_found(m: impl Into<String>) -> Self { Self::new("not_found", m) }
    pub fn conflict(m: impl Into<String>) -> Self { Self::new("conflict", m) }
    pub fn timeout(m: impl Into<String>) -> Self { Self::new("timeout", m) }
    pub fn unavailable(m: impl Into<String>) -> Self { Self::new("unavailable", m) }
    pub fn handler(m: impl Into<String>) -> Self { Self::new("handler_error", m) }
}

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for DeltaError {}
