//! Peers: pairing of nodes and routing of calls between them.
//!
//! Pairing: A `peers.invite` -> one-time code. B `peers.join(url_of_A, code)` calls A's public
//! `peers.accept`; A creates a pair secret and both store each other. Every later request
//! between them is HMAC-signed (see auth.rs); responses are signed too.
use crate::auth::{verify_request, Kind, NonceCache, Principal};
use crate::client::Client;
use crate::config::{Config, PeerRecord};
use crate::errors::{DeltaError, Result};
use crate::kernel::{cap, Authenticator, CallContext, CapSpec, Component, Kernel, Router};
use crate::util::{now, random_hex, sha256_hex, trunc};
use async_trait::async_trait;
use axum::http::HeaderMap;
use futures::future::join_all;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

const INVITE_TTL: f64 = 600.0;
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Port 0 is what a native client records before it listens: nobody can ever dial that address.
fn is_ghost_url(url: &str) -> bool {
    reqwest::Url::parse(url).map(|u| u.port() == Some(0)).unwrap_or(false)
}

/// A peer we can actually dial (outbound-only peers have an empty url).
fn dialable(p: &PeerRecord) -> bool {
    !p.url.is_empty() && !is_ghost_url(&p.url)
}

pub struct Peers {
    me: Weak<Peers>,
    config: Arc<Config>,
    invites: Mutex<HashMap<String, (f64, bool)>>, // sha256(code) -> (expiry, grant_admin)
    nonces: NonceCache,
    kernel: Mutex<Option<Arc<Kernel>>>,
    seen: Mutex<HashMap<String, f64>>, // node_id -> last successful contact; memory only, no config fsync
}

impl Peers {
    pub fn new(config: Arc<Config>) -> Arc<Self> {
        Arc::new_cyclic(|w| Peers {
            me: w.clone(),
            config,
            invites: Mutex::new(HashMap::new()),
            nonces: NonceCache::new(),
            kernel: Mutex::new(None),
            seen: Mutex::new(HashMap::new()),
        })
    }

    fn touch(&self, node_id: &str) {
        self.seen.lock().unwrap().insert(node_id.to_string(), now());
    }

    fn kernel(&self) -> Result<Arc<Kernel>> {
        self.kernel.lock().unwrap().clone().ok_or_else(|| DeltaError::unavailable("peers component is not running"))
    }

    // ------------------------------------------------------------ store
    fn peers(&self) -> Vec<PeerRecord> {
        self.config.read(|c| c.peers.clone())
    }

    fn upsert(&self, peer: PeerRecord) {
        self.config.update(move |c| {
            c.peers.retain(|p| p.node_id != peer.node_id);
            c.peers.push(peer);
        });
    }

    fn delete(&self, node_id: &str) -> bool {
        self.config.update(|c| {
            let before = c.peers.len();
            c.peers.retain(|p| p.node_id != node_id);
            before != c.peers.len()
        })
    }

    fn resolve(&self, node: &str) -> Result<PeerRecord> {
        let peers = self.peers();
        if let Some(p) = peers.iter().find(|p| p.node_id == node) {
            return Ok(p.clone());
        }
        let named: Vec<&PeerRecord> = peers.iter().filter(|p| p.name == node).collect();
        if named.len() > 1 {
            return Err(DeltaError::bad_request(format!("node name {node:?} is ambiguous, use node id")));
        }
        named.first().map(|p| (*p).clone()).ok_or_else(|| DeltaError::not_found(format!("unknown node {node}")))
    }

    fn client(&self, peer: &PeerRecord, kernel: &Arc<Kernel>, timeout: Option<Duration>) -> Client {
        Client::new(&peer.url, None)
            .with_signer(kernel.node_id.clone(), peer.secret.clone())
            .with_timeout(timeout.unwrap_or(kernel.call_timeout + Duration::from_secs(5)))
    }
}

// ------------------------------------------------------------ kernel hooks

impl Authenticator for Peers {
    fn authenticate(&self, method: &str, path: &str, headers: &HeaderMap, body: &[u8]) -> Result<Option<Principal>> {
        let node_id = match headers.get("x-delta-node").and_then(|v| v.to_str().ok()) {
            Some(n) if !n.is_empty() => n,
            _ => return Ok(None),
        };
        let peer = self.peers().into_iter().find(|p| p.node_id == node_id);
        let Some(peer) = peer else {
            return Err(DeltaError::unauthorized("invalid peer signature"));
        };
        if !verify_request(&peer.secret, method, path, headers, body, &self.nonces) {
            return Err(DeltaError::unauthorized("invalid peer signature"));
        }
        self.touch(&node_id);
        let scopes = if peer.scopes.is_empty() { vec!["call:*".to_string()] } else { peer.scopes.clone() };
        Ok(Some(Principal { kind: Kind::Peer, name: format!("peer:{node_id}"), scopes, peer_id: node_id.to_string() }))
    }
}

#[async_trait]
impl Router for Peers {
    async fn call(&self, node: &str, capability: &str, args: Value) -> Result<Value> {
        let peer = self.resolve(node)?;
        let k = self.kernel()?;
        let out = self.client(&peer, &k, None).call(capability, args, None, None).await;
        if out.is_ok() {
            self.touch(&peer.node_id);
        }
        out
    }

    async fn remote_catalog(&self) -> Vec<Value> {
        let peers = self.peers();
        let Ok(k) = self.kernel() else { return vec![] };
        if peers.is_empty() {
            return vec![];
        }
        let futs = peers.iter().filter(|p| dialable(p)).map(|p| {
            let c = self.client(p, &k, Some(PROBE_TIMEOUT));
            async move {
                match c.capabilities("local").await {
                    Ok(Value::Array(a)) => a,
                    _ => vec![],
                }
            }
        });
        join_all(futs).await.into_iter().flatten().collect()
    }

    async fn remote_capabilities(&self, node: &str) -> Result<Vec<Value>> {
        let peer = self.resolve(node)?;
        let k = self.kernel()?;
        match self.client(&peer, &k, Some(PROBE_TIMEOUT)).capabilities("local").await {
            Ok(Value::Array(a)) => {
                self.touch(&peer.node_id);
                Ok(a)
            }
            _ => Err(DeltaError::unavailable(format!("node {node} is unreachable"))),
        }
    }

    fn secret_for(&self, peer_id: &str) -> Option<String> {
        self.peers().into_iter().find(|p| p.node_id == peer_id).map(|p| p.secret)
    }
}

// ------------------------------------------------------------ component

#[async_trait]
impl Component for Peers {
    fn id(&self) -> &str {
        "peers"
    }
    fn version(&self) -> String {
        "0.1.0".into()
    }
    fn description(&self) -> String {
        "Connect nodes into one system and route calls between them".into()
    }

    fn capabilities(&self) -> Vec<CapSpec> {
        let s = |req: bool| json!({"type": "string", "required": req});
        vec![
            cap("peers.invite", "Create a one-time invitation with explicit incoming trust", json!({"trust_admin": {"type":"boolean"}}), true, false),
            cap("peers.trust", "Change incoming administrative trust for a paired node", json!({"node": s(true), "admin": {"type":"boolean", "required":true}}), true, false),
            cap(
                "peers.accept",
                "Accept a join request carrying an invite code",
                json!({"code": s(true), "node_id": s(true), "name": s(true), "url": s(true)}),
                false,
                true,
            ),
            cap(
                "peers.join",
                "Join another node using its URL and invite code",
                json!({"url": s(true), "code": s(true), "trust_admin": {"type":"boolean"}, "local_url": s(false)}),
                true,
                false,
            ),
            cap("peers.list", "Known peers and whether they are reachable now", json!({}), false, false),
            cap(
                "peers.remove",
                "Forget a peer (and tell it to forget us)",
                json!({"node": s(true)}),
                true,
                false,
            ),
            cap(
                "peers.prune",
                "List (dry_run, default) or forget peers recorded with an undialable port-0 address",
                json!({"dry_run": {"type":"boolean"}}),
                true,
                false,
            ),
            cap("peers.leave", "Called by a peer that is disconnecting from us", json!({}), false, false),
        ]
    }

    fn start(&self, kernel: &Arc<Kernel>) {
        *self.kernel.lock().unwrap() = Some(kernel.clone());
        if let Some(me) = self.me.upgrade() {
            let router: Arc<dyn Router> = me.clone();
            kernel.set_router(Some(router));
            let auth: Arc<dyn Authenticator> = me;
            kernel.add_authenticator(auth);
        }
    }

    async fn stop(&self) {
        let k = self.kernel.lock().unwrap().take();
        if let Some(k) = k {
            k.set_router(None);
            k.clear_authenticators();
        }
    }

    async fn handle(&self, capability: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        let k = ctx.kernel.clone();
        let sarg = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        match capability {
            "peers.invite" => {
                let trust_admin = args["trust_admin"].as_bool().unwrap_or(false);
                if trust_admin && !ctx.principal.is_admin() { return Err(DeltaError::forbidden("granting administrative trust requires global admin")); }
                let code = random_hex(18);
                let t = now();
                {
                    let mut inv = self.invites.lock().unwrap();
                    inv.retain(|_, (exp, _)| *exp > t);
                    if inv.len() >= 128 { return Err(DeltaError::conflict("too many pending invitations")); }
                    inv.insert(sha256_hex(code.as_bytes()), (t + INVITE_TTL, trust_admin));
                }
                let url = self.config.read(|c| c.url.clone());
                Ok(json!({
                    "code": code, "url": url, "expires_in": INVITE_TTL as u64,
                    "command": format!("delta join {url} {code}"),
                }))
            }
            "peers.trust" => {
                if !ctx.principal.is_admin() { return Err(DeltaError::forbidden("changing administrative trust requires global admin")); }
                let admin = args["admin"].as_bool().ok_or_else(|| DeltaError::bad_request("admin boolean required"))?;
                let mut peer = self.resolve(&sarg("node"))?;
                peer.scopes = vec![if admin { "admin".into() } else { "call:*".into() }];
                let out = json!({"node_id":peer.node_id,"trusted_admin":admin});
                self.upsert(peer);
                Ok(out)
            }
            "peers.accept" => {
                let h = sha256_hex(sarg("code").as_bytes());
                let (expires, trust_admin) = self.invites.lock().unwrap().remove(&h).unwrap_or((0.0, false));
                if expires < now() {
                    return Err(DeltaError::forbidden("invalid or expired invite code"));
                }
                let node_id = sarg("node_id");
                if node_id == k.node_id {
                    return Err(DeltaError::bad_request("a node cannot join itself"));
                }
                let secret = random_hex(32);
                // `name` is only a display label for the UI chosen by the joiner, never an identity:
                // authorization uses node_id (see the `peer:<node_id>` Principal in `authenticate`).
                self.upsert(PeerRecord {
                    node_id: trunc(&node_id, 64),
                    name: trunc(&sarg("name"), 64),
                    // a port-0 address can never be dialled: keep the peer as outbound-only (empty url)
                    url: Some(trunc(&sarg("url"), 512)).filter(|u| !is_ghost_url(u)).unwrap_or_default(),
                    secret: secret.clone(),
                    scopes: if trust_admin { vec!["admin".into()] } else { vec!["call:*".into()] },
                    added: now(),
                });
                Ok(json!({
                    "node_id": k.node_id, "name": k.name,
                    "url": self.config.read(|c| c.url.clone()), "secret": secret,
                }))
            }
            "peers.join" => {
                let url = sarg("url");
                if args["trust_admin"].as_bool() == Some(true) && !ctx.principal.is_admin() { return Err(DeltaError::forbidden("granting administrative trust requires global admin")); }
                let explicit = args.get("local_url").and_then(Value::as_str).map(str::to_owned);
                let advertised = explicit.clone().unwrap_or_else(|| self.config.read(|c| c.url.clone()));
                // a node without a dialable address (native clients listen on port 0) joins outbound-only
                let advertised = if explicit.is_none() && (advertised.is_empty() || is_ghost_url(&advertised)) { String::new() } else { advertised };
                if !advertised.is_empty() {
                    let parsed = reqwest::Url::parse(&advertised).map_err(|_| DeltaError::bad_request("invalid local_url"))?;
                    if !["http", "https"].contains(&parsed.scheme()) || parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() || parsed.fragment().is_some() || advertised.len() > 512 || parsed.port() == Some(0) { return Err(DeltaError::bad_request("invalid local_url")); }
                }
                let res = Client::new(&url, None)
                    .with_timeout(Duration::from_secs(10))
                    .call(
                        "peers.accept",
                        json!({
                            "code": sarg("code"), "node_id": k.node_id, "name": k.name,
                            "url": advertised,
                        }),
                        None,
                        None,
                    )
                    .await?;
                let field = |key: &str| res.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                let rec = PeerRecord {
                    node_id: field("node_id"),
                    name: field("name"),
                    url: url.trim_end_matches('/').to_string(),
                    secret: field("secret"),
                    scopes: if args["trust_admin"].as_bool()==Some(true) { vec!["admin".into()] } else { vec!["call:*".into()] },
                    added: now(),
                };
                let out = json!({"node_id": rec.node_id, "name": rec.name, "url": url});
                self.upsert(rec);
                Ok(out)
            }
            "peers.list" => {
                let peers = self.peers();
                let futs = peers.iter().map(|p| {
                    let c = self.client(p, &k, Some(PROBE_TIMEOUT));
                    let p = p.clone();
                    async move {
                        let mut out = json!({"node_id": p.node_id, "name": p.name, "url": p.url, "trusted_admin": p.scopes.iter().any(|s| s == "admin"), "online": false});
                        if !dialable(&p) {
                            // nothing to probe: either an outbound-only client or a legacy port-0 registration
                            out["outbound_only"] = json!(p.url.is_empty());
                            out["ghost"] = json!(is_ghost_url(&p.url));
                            return out;
                        }
                        match c.call("core.info", json!({}), None, None).await {
                            Ok(info) => {
                                out["online"] = json!(true);
                                out["kind"] = info.get("kind").cloned().unwrap_or(json!("desktop"));
                                out["version"] = info.get("version").cloned().unwrap_or(Value::Null);
                                out["uptime"] = info.get("uptime").cloned().unwrap_or(Value::Null);
                            }
                            Err(e) => out["error"] = json!(e.message),
                        }
                        out
                    }
                });
                let mut rows = join_all(futs).await;
                for out in rows.iter_mut() {
                    let id = out["node_id"].as_str().unwrap_or("").to_string();
                    if out["online"] == json!(true) {
                        self.touch(&id);
                    }
                    // epoch seconds of the last successful contact since this node started; null = none yet
                    out["last_seen"] = self.seen.lock().unwrap().get(&id).map(|t| json!(t)).unwrap_or(Value::Null);
                }
                Ok(Value::Array(rows))
            }
            "peers.prune" => {
                // explicit only: never runs on its own and only touches registrations that can never connect
                let dry_run = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
                let ghosts: Vec<PeerRecord> = self.peers().into_iter().filter(|p| is_ghost_url(&p.url)).collect();
                let candidates: Vec<Value> = ghosts.iter().map(|p| json!({"node_id": p.node_id, "name": p.name, "url": p.url})).collect();
                let mut removed = vec![];
                if !dry_run {
                    for p in &ghosts {
                        if self.delete(&p.node_id) {
                            removed.push(p.node_id.clone());
                        }
                    }
                }
                Ok(json!({"dry_run": dry_run, "candidates": candidates, "removed": removed}))
            }
            "peers.remove" => {
                let peer = self.resolve(&sarg("node"))?;
                let _ = self.client(&peer, &k, Some(PROBE_TIMEOUT)).call("peers.leave", json!({}), None, None).await;
                self.delete(&peer.node_id);
                Ok(json!({"removed": peer.node_id}))
            }
            "peers.leave" => {
                if ctx.principal.kind != Kind::Peer {
                    return Err(DeltaError::forbidden("only a peer can leave"));
                }
                self.delete(&ctx.principal.peer_id);
                Ok(json!({"removed": ctx.principal.peer_id}))
            }
            _ => Err(DeltaError::not_found(format!("unknown capability {capability}"))),
        }
    }
}
