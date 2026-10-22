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

fn is_unreachable_invite_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost"
        || host == "0.0.0.0"
        || host == "::"
        || host == "::1"
        || host.starts_with("127.")
}


pub struct Peers {
    me: Weak<Peers>,
    config: Arc<Config>,
    invites: Mutex<HashMap<String, (f64, bool)>>, // sha256(code) -> (expiry, grant_admin)
    nonces: NonceCache,
    kernel: Mutex<Option<Arc<Kernel>>>,
    discovered: Mutex<Vec<PeerRecord>>,
}

impl Peers {
    pub fn new(config: Arc<Config>) -> Arc<Self> {
        Arc::new_cyclic(|w| Peers {
            me: w.clone(),
            config,
            invites: Mutex::new(HashMap::new()),
            nonces: NonceCache::new(),
            kernel: Mutex::new(None),
            discovered: Mutex::new(Vec::new()),
        })
    }


    async fn refresh_discovered(&self) {
        let Ok(k) = self.kernel() else { return };
        let mut found = Vec::new();
        let mut seen: std::collections::HashSet<String> = self.peers().iter().map(|p| p.node_id.clone()).collect();
        seen.insert(k.node_id.clone());
        for peer in self.peers() {
            if peer.url.trim().is_empty() { continue; }
            let Ok(remote) = self.client(&peer, &k, Some(PROBE_TIMEOUT)).call("peers.list", json!({}), None, None).await else { continue };
            for item in remote.as_array().cloned().unwrap_or_default() {
                let id = item.get("node_id").and_then(Value::as_str).unwrap_or("").to_string();
                if id.is_empty() || !seen.insert(id.clone()) { continue; }
                found.push(PeerRecord { node_id: id, name: item.get("name").and_then(Value::as_str).unwrap_or("").to_string(), url: item.get("url").and_then(Value::as_str).unwrap_or("").to_string(), secret: String::new(), scopes: vec![], added: 0.0, kind: item.get("kind").and_then(Value::as_str).unwrap_or("").to_string(), last_seen: item.get("last_seen").and_then(Value::as_f64).unwrap_or(0.0), online: item.get("online").and_then(Value::as_bool).unwrap_or(false) });
            }
        }
        *self.discovered.lock().unwrap() = found;
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
        let scopes = if peer.scopes.is_empty() { vec!["call:*".to_string()] } else { peer.scopes.clone() };
        Ok(Some(Principal { kind: Kind::Peer, name: peer.name, scopes, peer_id: node_id.to_string() }))
    }
}

#[async_trait]
impl Router for Peers {
    async fn call(&self, node: &str, capability: &str, args: Value) -> Result<Value> {
        let k = self.kernel()?;
        if let Ok(peer) = self.resolve(node) {
            if !peer.url.trim().is_empty() {
                return self.client(&peer, &k, None).call(capability, args, None, None).await;
            }
            return Err(DeltaError::unavailable("peer is outbound-only"));
        }
        // The caller knows a peer this node does not dial directly. Ask a reachable admin peer to route it.
        for peer in self.peers() {
            if peer.url.trim().is_empty() || !peer.scopes.iter().any(|s| s == "admin") { continue; }
            if let Ok(v) = self.client(&peer, &k, None).call(capability, args.clone(), Some(node), None).await {
                return Ok(v);
            }
        }
        Err(DeltaError::not_found(format!("unknown node {node}")))
    }

    async fn remote_catalog(&self) -> Vec<Value> {
        let peers = self.peers();
        let Ok(k) = self.kernel() else { return vec![] };
        if peers.is_empty() {
            return vec![];
        }
        let futs = peers.iter().map(|p| {
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
            cap("peers.leave", "Called by a peer that is disconnecting from us", json!({}), false, false),
        ]
    }

    fn start(&self, kernel: &Arc<Kernel>) {
        *self.kernel.lock().unwrap() = Some(kernel.clone());
        if let Some(me) = self.me.upgrade() {
            tokio::spawn(async move { loop { me.refresh_discovered().await; tokio::time::sleep(Duration::from_secs(5)).await; } });
        }
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
                let configured = self.config.read(|c| c.url.clone());
                let url = args.get("url").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(configured.as_str()).to_string();
                let parsed = reqwest::Url::parse(&url).map_err(|_| DeltaError::bad_request("invitation endpoint is not a URL"))?;
                let host = parsed.host_str().unwrap_or("");
                if !["http", "https"].contains(&parsed.scheme()) || host.is_empty() {
                    return Err(DeltaError::bad_request("invitation endpoint must be http or https with a host"));
                }
                let allow_local = args.get("allow_local").and_then(Value::as_bool).unwrap_or(false);
                if is_unreachable_invite_host(host) && !allow_local {
                    return Err(DeltaError::bad_request("invitation endpoint is this machine only; set a LAN, VPN or public URL before adding a phone"));
                }
                let code = random_hex(18);
                let t = now();
                {
                    let mut inv = self.invites.lock().unwrap();
                    inv.retain(|_, (exp, _)| *exp > t);
                    if inv.len() >= 128 { return Err(DeltaError::conflict("too many pending invitations")); }
                    inv.insert(sha256_hex(code.as_bytes()), (t + INVITE_TTL, trust_admin));
                }
                let public_url = self.config.read(|c| c.public_url.clone());
                let preferred = if public_url.trim().is_empty() { url.clone() } else { public_url.trim().to_string() };
                let mut link = reqwest::Url::parse("delta://pair").expect("static scheme");
                link.query_pairs_mut().append_pair("url", &preferred).append_pair("code", &code);
                Ok(json!({
                    "code": code, "url": preferred, "local_url": url, "public_url": public_url, "expires_in": INVITE_TTL as u64,
                    "command": format!("delta join {preferred} {code}"),
                    "link": link.to_string(),
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
                self.upsert(PeerRecord {
                    node_id: trunc(&node_id, 64),
                    name: trunc(&sarg("name"), 64),
                    url: trunc(&sarg("url"), 512),
                    secret: secret.clone(),
                    scopes: if trust_admin { vec!["admin".into()] } else { vec!["call:*".into()] },
                    added: now(),
                    kind: trunc(&sarg("kind"), 32),
                    last_seen: now(),
                    online: false,
                });
                Ok(json!({
                    "node_id": k.node_id, "name": k.name,
                    "url": self.config.read(|c| c.url.clone()), "secret": secret,
                }))
            }
            "peers.join" => {
                let url = sarg("url");
                if args["trust_admin"].as_bool() == Some(true) && !ctx.principal.is_admin() { return Err(DeltaError::forbidden("granting administrative trust requires global admin")); }
                // A phone behind NAT has no address the hub can dial. Empty means outbound-only.
                let advertised = args.get("local_url").and_then(Value::as_str).unwrap_or("").trim().to_string();
                if !advertised.is_empty() {
                    let parsed = reqwest::Url::parse(&advertised).map_err(|_| DeltaError::bad_request("invalid local_url"))?;
                    if !["http", "https"].contains(&parsed.scheme()) || parsed.host_str().is_none() || !parsed.username().is_empty() || parsed.password().is_some() || parsed.fragment().is_some() || advertised.len() > 512 { return Err(DeltaError::bad_request("invalid local_url")); }
                }
                let res = Client::new(&url, None)
                    .with_timeout(Duration::from_secs(10))
                    .call(
                        "peers.accept",
                        json!({
                            "code": sarg("code"), "node_id": k.node_id, "name": k.name,
                            "url": advertised, "kind": k.config.read(|c| c.node_kind.clone()),
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
                    kind: field("kind"),
                    last_seen: now(),
                    online: false,
                };
                let out = json!({"node_id": rec.node_id, "name": rec.name, "url": url});
                self.upsert(rec);
                Ok(out)
            }
            "peers.list" => {
                let mut peers = self.peers();
                // An admin caller sees the same network, including peers it did not join directly.
                if ctx.principal.is_admin() {
                    let caller = ctx.principal.peer_id.clone();
                    let mut seen: std::collections::HashSet<String> = peers.iter().map(|p| p.node_id.clone()).collect();
                    for peer in self.discovered.lock().unwrap().clone() {
                        if peer.node_id == caller || !seen.insert(peer.node_id.clone()) { continue; }
                        peers.push(peer);
                    }
                }
                let futs = peers.iter().map(|p| {
                    let c = self.client(p, &k, Some(PROBE_TIMEOUT));
                    let p = p.clone();
                    async move {
                        let mut out = json!({"node_id": p.node_id, "name": p.name, "url": p.url, "kind": p.kind, "last_seen": p.last_seen, "trusted_admin": p.scopes.iter().any(|s| s == "admin"), "online": false});
                        if p.secret.is_empty() {
                            out["online"] = json!(p.online);
                            return out;
                        }
                        if p.url.trim().is_empty() {
                            out["error"] = json!("outbound-only peer");
                            return out;
                        }
                        match c.call("core.info", json!({}), None, None).await {
                            Ok(info) => {
                                out["online"] = json!(true);
                                out["kind"] = info.get("kind").cloned().filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false)).unwrap_or(json!(p.kind));
                                out["version"] = info.get("version").cloned().unwrap_or(Value::Null);
                                out["uptime"] = info.get("uptime").cloned().unwrap_or(Value::Null);
                                out["last_seen"] = json!(now());
                            }
                            Err(e) => out["error"] = json!(e.message),
                        }
                        out
                    }
                });
                Ok(Value::Array(join_all(futs).await))
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
