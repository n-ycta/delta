//! Principals, scoped tokens and HMAC request signing.
//!
//! Scopes:
//!   admin           - everything, including capabilities marked admin
//!   call:<glob>     - call non-admin capabilities matching glob
//!   register:<glob> - register external components whose id matches glob
use crate::config::{Config, TokenRecord};
use crate::util::{ct_eq, glob_match, now, random_hex, sha256_hex};
use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub const SIG_WINDOW: f64 = 60.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Admin,
    Token,
    Peer,
    Anonymous,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Admin => "admin",
            Kind::Token => "token",
            Kind::Peer => "peer",
            Kind::Anonymous => "anonymous",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Principal {
    pub kind: Kind,
    pub name: String,
    pub scopes: Vec<String>,
    pub peer_id: String,
}

impl Principal {
    pub fn anonymous() -> Self {
        Self { kind: Kind::Anonymous, name: "anonymous".into(), scopes: vec![], peer_id: String::new() }
    }

    pub fn is_admin(&self) -> bool {
        self.scopes.iter().any(|s| s == "admin")
    }

    pub fn can_call(&self, capability: &str, admin_only: bool, public: bool) -> bool {
        if public || self.is_admin() {
            return true;
        }
        if admin_only {
            return false;
        }
        self.scopes
            .iter()
            .any(|s| s.strip_prefix("call:").map(|g| glob_match(g, capability)).unwrap_or(false))
    }

    pub fn can_register(&self, component_id: &str) -> bool {
        if self.is_admin() {
            return true;
        }
        self.scopes
            .iter()
            .any(|s| s.strip_prefix("register:").map(|g| glob_match(g, component_id)).unwrap_or(false))
    }

    pub fn to_value(&self) -> Value {
        json!({"kind": self.kind.as_str(), "name": self.name, "peer_id": self.peer_id})
    }
}

pub fn hash_token(token: &str) -> String {
    sha256_hex(token.as_bytes())
}

pub fn new_token() -> String {
    format!("dlt_{}", random_hex(32))
}

pub fn valid_scope(s: &str) -> bool {
    s == "admin"
        || ((s.starts_with("call:") || s.starts_with("register:"))
            && s.split_once(':').map(|(_, rest)| !rest.is_empty()).unwrap_or(false))
}

/// Persistent tokens live in config (hash only); ephemeral ones only in memory.
pub struct TokenStore {
    config: Arc<Config>,
    ephemeral: Mutex<HashMap<String, TokenRecord>>,
}

impl TokenStore {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config, ephemeral: Mutex::new(HashMap::new()) }
    }

    pub fn create(
        &self,
        name: &str,
        scopes: &[String],
        persist: bool,
    ) -> std::result::Result<(String, TokenRecord), String> {
        for s in scopes {
            if !valid_scope(s) {
                return Err(format!("invalid scope: {s}"));
            }
        }
        let token = new_token();
        let rec = TokenRecord {
            id: random_hex(4),
            name: name.to_string(),
            hash: hash_token(&token),
            scopes: scopes.to_vec(),
        };
        if persist {
            let r = rec.clone();
            self.config.update(move |c| c.tokens.push(r));
        } else {
            self.ephemeral.lock().unwrap().insert(rec.hash.clone(), rec.clone());
        }
        Ok((token, rec))
    }

    pub fn revoke(&self, token_id: &str) -> bool {
        let mut changed = self.config.update_if(|c| {
            let before = c.tokens.len();
            c.tokens.retain(|t| t.id != token_id);
            c.tokens.len() != before
        });
        let mut eph = self.ephemeral.lock().unwrap();
        let before = eph.len();
        eph.retain(|_, r| r.id != token_id);
        if eph.len() != before {
            changed = true;
        }
        changed
    }

    pub fn list(&self) -> Vec<Value> {
        let mut out: Vec<Value> = self.config.read(|c| {
            c.tokens
                .iter()
                .map(|t| json!({"id": t.id, "name": t.name, "scopes": t.scopes, "persistent": true}))
                .collect()
        });
        let eph = self.ephemeral.lock().unwrap();
        out.extend(
            eph.values().map(|t| json!({"id": t.id, "name": t.name, "scopes": t.scopes, "persistent": false})),
        );
        out
    }

    pub fn authenticate(&self, token: &str) -> Option<Principal> {
        let h = hash_token(token);
        let mut recs: Vec<TokenRecord> = self.config.read(|c| c.tokens.clone());
        recs.extend(self.ephemeral.lock().unwrap().values().cloned());
        for r in recs {
            if ct_eq(r.hash.as_bytes(), h.as_bytes()) {
                let kind = if r.scopes.iter().any(|s| s == "admin") { Kind::Admin } else { Kind::Token };
                return Some(Principal { kind, name: r.name, scopes: r.scopes, peer_id: String::new() });
            }
        }
        None
    }
}

// ---------------------------------------------------------------- HMAC signing

fn hmac_hex(secret: &str, msg: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(msg);
    hex::encode(mac.finalize().into_bytes())
}

fn canonical(method: &str, path: &str, ts: &str, nonce: &str, body: &[u8]) -> Vec<u8> {
    [method.to_uppercase().as_str(), path, ts, nonce, sha256_hex(body).as_str()].join("\n").into_bytes()
}

/// Returns the headers that sign a request. The nonce is in `X-Delta-Nonce`.
pub fn sign_request(secret: &str, node_id: &str, method: &str, path: &str, body: &[u8]) -> Vec<(String, String)> {
    let ts = (now() as u64).to_string();
    let nonce = random_hex(12);
    let sig = hmac_hex(secret, &canonical(method, path, &ts, &nonce, body));
    vec![
        ("X-Delta-Node".into(), node_id.to_string()),
        ("X-Delta-Ts".into(), ts),
        ("X-Delta-Nonce".into(), nonce),
        ("X-Delta-Signature".into(), sig),
    ]
}

pub fn sign_response(secret: &str, nonce: &str, status: u16, body: &[u8]) -> String {
    let msg = format!("RESPONSE\n{}\n{}\n{}", status, nonce, sha256_hex(body));
    hmac_hex(secret, msg.as_bytes())
}

pub fn verify_response(secret: &str, nonce: &str, status: u16, body: &[u8], sig: &str) -> bool {
    !sig.is_empty() && ct_eq(sign_response(secret, nonce, status, body).as_bytes(), sig.as_bytes())
}

#[derive(Default)]
pub struct NonceCache {
    seen: Mutex<HashMap<String, f64>>,
}

impl NonceCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check_and_add(&self, nonce: &str) -> bool {
        let t = now();
        let mut seen = self.seen.lock().unwrap();
        if seen.len() > 10_000 {
            seen.retain(|_, ts| t - *ts < SIG_WINDOW * 2.0);
        }
        if seen.contains_key(nonce) {
            return false;
        }
        seen.insert(nonce.to_string(), t);
        true
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

pub fn verify_request(
    secret: &str,
    method: &str,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
    nonces: &NonceCache,
) -> bool {
    let (ts, nonce, sig) = (header(headers, "x-delta-ts"), header(headers, "x-delta-nonce"), header(headers, "x-delta-signature"));
    if ts.is_empty() || !ts.bytes().all(|c| c.is_ascii_digit()) || nonce.is_empty() || sig.is_empty() {
        return false;
    }
    let ts_num: f64 = ts.parse().unwrap_or(0.0);
    if (now() - ts_num).abs() > SIG_WINDOW {
        return false;
    }
    let expected = hmac_hex(secret, &canonical(method, path, ts, nonce, body));
    if !ct_eq(expected.as_bytes(), sig.as_bytes()) {
        return false;
    }
    nonces.check_and_add(nonce)
}
