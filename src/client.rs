//! HTTP client for the Delta API.
use crate::auth::{sign_request, verify_response};
use crate::errors::{DeltaError, Result};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Client {
    url: String,
    token: Option<String>,
    timeout: Duration,
    signer: Option<(String, String)>, // (node_id, secret): makes this a peer client
    http: reqwest::Client,
}

impl Client {
    pub fn new(url: &str, token: Option<String>) -> Self {
        let mut b = reqwest::Client::builder();
        if let Ok(ca) = std::env::var("DELTA_CA") {
            if let Ok(pem) = std::fs::read(ca) {
                if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
                    b = b.add_root_certificate(cert);
                }
            }
        }
        Self {
            url: url.trim_end_matches('/').to_string(),
            token,
            timeout: Duration::from_secs(35),
            signer: None,
            http: b.build().unwrap_or_default(),
        }
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn with_signer(mut self, node_id: String, secret: String) -> Self {
        self.signer = Some((node_id, secret));
        self
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        data: Option<&Value>,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let body: Vec<u8> = data.map(|d| serde_json::to_vec(d).unwrap_or_default()).unwrap_or_default();
        let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| DeltaError::bad_request("bad method"))?;
        let mut req = self
            .http
            .request(m, format!("{}{}", self.url, path))
            .timeout(timeout.unwrap_or(self.timeout))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        if let Some(t) = &self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let mut nonce = String::new();
        if let Some((node_id, secret)) = &self.signer {
            let sig_path = path.split('?').next().unwrap_or(path);
            for (k, v) in sign_request(secret, node_id, method, sig_path, &body) {
                if k == "X-Delta-Nonce" {
                    nonce = v.clone();
                }
                req = req.header(k, v);
            }
        }
        if method != "GET" {
            req = req.body(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| DeltaError::unavailable(format!("cannot reach {}: {}", self.url, e)))?;
        let status = resp.status().as_u16();
        let sig = resp
            .headers()
            .get("x-delta-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let raw = resp
            .bytes()
            .await
            .map_err(|e| DeltaError::unavailable(format!("cannot read response from {}: {}", self.url, e)))?;
        if let Some((_, secret)) = &self.signer {
            if !verify_response(secret, &nonce, status, &raw, &sig) {
                return Err(DeltaError::unauthorized("peer response signature invalid"));
            }
        }
        let payload: Value = serde_json::from_slice(&raw)
            .map_err(|_| DeltaError::unavailable(format!("non-JSON response from {} (HTTP {})", self.url, status)))?;
        if payload.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(DeltaError::from_value(payload.get("error").unwrap_or(&json!({}))));
        }
        Ok(payload.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn call(
        &self,
        capability: &str,
        args: Value,
        node: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let mut data = json!({"capability": capability, "args": args});
        if let Some(n) = node {
            data["node"] = json!(n);
        }
        self.request("POST", "/v1/call", Some(&data), timeout).await
    }

    pub async fn capabilities(&self, scope: &str) -> Result<Value> {
        self.request("GET", &format!("/v1/capabilities?scope={scope}"), None, None).await
    }

    pub async fn health(&self) -> Result<Value> {
        self.request("GET", "/v1/health", None, Some(Duration::from_secs(3))).await
    }
}
