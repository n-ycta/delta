//! HTTP transport for the kernel (protocol v1).
use crate::auth::{sign_response, Kind, Principal};
use crate::errors::{DeltaError, Result};
use crate::kernel::{ExternalComponent, Kernel};
use crate::{PROTOCOL, VERSION};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
    Router,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

pub const MAX_BODY: usize = 4 * 1024 * 1024;
pub const MAX_POLL: f64 = 25.0;

pub fn app(kernel: Arc<Kernel>) -> Router {
    Router::new()
        .fallback(handle)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(kernel)
}

async fn handle(
    State(k): State<Arc<Kernel>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut principal: Option<Principal> = None;
    let outcome = dispatch(&k, &method, &uri, &headers, &body, &mut principal).await;
    let (status, payload) = match outcome {
        Ok(v) => (200u16, json!({"ok": true, "result": v})),
        Err(e) => (e.status(), json!({"ok": false, "error": e.to_value()})),
    };
    send(&k, status, &payload, principal.as_ref(), &headers)
}

fn send(k: &Arc<Kernel>, status: u16, payload: &Value, principal: Option<&Principal>, req_headers: &HeaderMap) -> Response {
    let body = serde_json::to_vec(payload).unwrap_or_default();
    let mut b = Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .header("x-content-type-options", "nosniff");
    if let Some(p) = principal {
        if p.kind == Kind::Peer {
            if let Some(secret) = k.router().and_then(|r| r.secret_for(&p.peer_id)) {
                let nonce = req_headers.get("x-delta-nonce").and_then(|v| v.to_str().ok()).unwrap_or("");
                b = b.header("x-delta-signature", sign_response(&secret, nonce, status, &body));
            }
        }
    }
    b.body(Body::from(body)).unwrap_or_else(|_| Response::new(Body::empty()))
}

fn parse_json(raw: &[u8]) -> Result<Value> {
    if raw.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(_) => Err(DeltaError::bad_request("JSON object expected")),
        Err(_) => Err(DeltaError::bad_request("invalid JSON")),
    }
}

fn query_param(q: Option<&str>, key: &str) -> Option<String> {
    q?.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| v.to_string())
    })
}

async fn dispatch(
    k: &Arc<Kernel>,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
    principal: &mut Option<Principal>,
) -> Result<Value> {
    let path = uri.path();
    let all: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if all.first().copied() != Some(PROTOCOL) {
        return Err(DeltaError::not_found("unknown path"));
    }
    let parts = &all[1..];
    if *method == Method::GET && matches!(parts, ["health"]) {
        return Ok(json!({"status": "ok", "version": VERSION}));
    }
    let p = k.authenticate(method.as_str(), path, headers, body)?;
    *principal = Some(p.clone());
    let data = parse_json(body)?;
    route(k, method.as_str(), parts, uri.query(), data, &p).await
}

async fn route(
    k: &Arc<Kernel>,
    method: &str,
    parts: &[&str],
    query: Option<&str>,
    data: Value,
    p: &Principal,
) -> Result<Value> {
    if p.kind == Kind::Anonymous && !matches!(parts, ["call"]) {
        return Err(DeltaError::unauthorized("authentication required"));
    }
    match (method, parts) {
        ("POST", ["call"]) => {
            let cap = data
                .get("capability")
                .and_then(Value::as_str)
                .ok_or_else(|| DeltaError::bad_request("invalid capability name"))?;
            let args = data.get("args").cloned().unwrap_or(Value::Null);
            let node = data.get("node").and_then(Value::as_str);
            k.call(cap, args, p, node).await
        }
        ("GET", ["capabilities"]) => {
            let scope = query_param(query, "scope").unwrap_or_else(|| "local".into());
            if scope == "all" && p.kind != Kind::Peer {
                return k.call("core.catalog", json!({}), p, None).await;
            }
            Ok(Value::Array(k.catalog()))
        }
        (_, ["components", rest @ ..]) => components(k, method, rest, query, &data, p).await,
        _ => Err(DeltaError::not_found("unknown path")),
    }
}

async fn components(
    k: &Arc<Kernel>,
    method: &str,
    rest: &[&str],
    query: Option<&str>,
    data: &Value,
    p: &Principal,
) -> Result<Value> {
    if method == "POST" && rest.is_empty() {
        let cid = data.get("id").and_then(Value::as_str).unwrap_or("");
        if !p.can_register(cid) {
            return Err(DeltaError::forbidden(format!("not allowed to register {cid:?}")));
        }
        let comp = Arc::new(ExternalComponent::new(data, p.name.clone())?);
        k.add_external(comp.clone()).await?;
        return Ok(json!({
            "id": cid, "session": comp.session, "node": k.node_id,
            "capabilities": comp.capability_names(),
        }));
    }
    let Some(cid) = rest.first().copied() else {
        return Err(DeltaError::not_found("unknown path"));
    };
    if !p.can_register(cid) {
        return Err(DeltaError::forbidden("not your component"));
    }
    let session = query_param(query, "session")
        .or_else(|| data.get("session").and_then(Value::as_str).map(String::from))
        .unwrap_or_default();
    let comp = k.get_external(cid, &session)?;
    match (method, &rest[1..]) {
        ("GET", ["next"]) => {
            let t = query_param(query, "timeout")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(MAX_POLL)
                .min(MAX_POLL)
                .max(0.0);
            let inv = comp.next(Duration::from_secs_f64(t)).await;
            Ok(json!({"invocation": inv}))
        }
        ("POST", ["result"]) => {
            let id = data.get("id").and_then(Value::as_str).unwrap_or("");
            let ok = data.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let resp = json!({
                "ok": ok,
                "result": data.get("result").cloned().unwrap_or(Value::Null),
                "error": data.get("error").cloned().unwrap_or(Value::Null),
            });
            if !comp.resolve(id, resp) {
                return Err(DeltaError::not_found("unknown or expired invocation"));
            }
            Ok(json!({"accepted": true}))
        }
        ("DELETE", []) => Ok(json!({"removed": k.remove(cid).await})),
        _ => Err(DeltaError::not_found("unknown path")),
    }
}
