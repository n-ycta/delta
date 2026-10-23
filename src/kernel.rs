//! Kernel: component registry + call router + access checks.
//!
//! The kernel is the bootstrap mechanism. It is deliberately NOT a component, so loading
//! it never depends on capabilities. Everything user-facing (core.*, peers.*, ...) is a
//! component that uses exactly the same contract as third-party plugins.
use crate::auth::{Kind, Principal, TokenStore};
use crate::config::Config;
use crate::errors::{DeltaError, Result};
use crate::util::{round1, valid_capability, valid_id, wait_true};
use crate::VERSION;
use async_trait::async_trait;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

// ------------------------------------------------------------------ contract

fn empty_obj() -> Value {
    json!({})
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub localizations: Value,
    #[serde(default = "empty_obj")]
    pub params: Value,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub requires: Vec<String>,
}

/// Convenience constructor used by built-in components (`name` is the full `<component>.<action>`).
pub fn cap(name: &str, description: &str, params: Value, admin: bool, public: bool) -> CapSpec {
    static LOCALES: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let catalog = LOCALES.get_or_init(|| serde_json::from_str(include_str!("locales/ru.json")).expect("bundled locales"));
    let localizations = catalog.get(name).map(|s| json!({"ru":{"description":s}})).unwrap_or(Value::Null);
    CapSpec { name: name.into(), description: description.into(), localizations, params, admin, public, requires: vec![] }
}

pub struct CallContext {
    pub kernel: Arc<Kernel>,
    pub principal: Principal,
    pub capability: String,
}

#[async_trait]
pub trait Component: Send + Sync {
    fn id(&self) -> &str;
    fn version(&self) -> String {
        "0.0.0".into()
    }
    fn description(&self) -> String {
        String::new()
    }
    fn kind(&self) -> &'static str {
        "builtin"
    }
    fn metadata(&self) -> Value { json!({}) }
    fn capabilities(&self) -> Vec<CapSpec>;
    async fn handle(&self, capability: &str, args: Value, ctx: &CallContext) -> Result<Value>;
    fn start(&self, _kernel: &Arc<Kernel>) {}
    async fn stop(&self) {}
}

/// Installed by the peers component: forwards calls to other nodes.
#[async_trait]
pub trait Router: Send + Sync {
    async fn call(&self, node: &str, capability: &str, args: Value) -> Result<Value>;
    async fn remote_catalog(&self) -> Vec<Value>;
    /// Capability list of one peer only (resolved by id or name); `unavailable` if it does not answer.
    async fn remote_capabilities(&self, node: &str) -> Result<Vec<Value>>;
    fn secret_for(&self, peer_id: &str) -> Option<String>;
}

/// Extra authentication schemes (peers use HMAC signatures).
pub trait Authenticator: Send + Sync {
    fn authenticate(&self, method: &str, path: &str, headers: &HeaderMap, body: &[u8]) -> Result<Option<Principal>>;
}

/// Minimal JSON-schema-like validation: {name: {type, required, description}}.
pub fn validate_args(params: &Value, args: &Value) -> Result<()> {
    let obj = args.as_object().ok_or_else(|| DeltaError::bad_request("args must be an object"))?;
    if let Some(ps) = params.as_object() {
        for (pname, spec) in ps {
            match obj.get(pname) {
                None | Some(Value::Null) => {
                    if spec.get("required").and_then(Value::as_bool) == Some(true) {
                        return Err(DeltaError::bad_request(format!("missing required argument '{pname}'")));
                    }
                }
                Some(v) => {
                    let ty = spec.get("type").and_then(Value::as_str).unwrap_or("");
                    let ok = match ty {
                        "string" => v.is_string(),
                        "integer" => v.is_i64() || v.is_u64(),
                        "number" => v.is_number(),
                        "boolean" => v.is_boolean(),
                        "object" => v.is_object(),
                        "array" => v.is_array(),
                        _ => true,
                    };
                    if !ok {
                        return Err(DeltaError::bad_request(format!("argument '{pname}' must be {ty}")));
                    }
                }
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ external components

/// A component living in another process. Calls are queued and fetched by long-poll.
pub struct ExternalComponent {
    id: String,
    version: String,
    description: String,
    pub session: String,
    pub owner: String,
    caps: Vec<CapSpec>,
    metadata: Value,
    tx: mpsc::UnboundedSender<Value>,
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Value>>,
    pending: Mutex<HashMap<String, (oneshot::Sender<Value>, Principal)>>,
    last_seen: Mutex<Instant>,
    polling: AtomicUsize,
    closed: watch::Sender<bool>,
}

fn value_to_string(v: &Value) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| v.to_string())
}

impl ExternalComponent {
    pub fn new(manifest: &Value, owner: String) -> Result<Self> {
        let id = manifest.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let version = manifest.get("version").map(value_to_string).unwrap_or_else(|| "0.0.0".into());
        let description = manifest.get("description").map(value_to_string).unwrap_or_default();
        let mut caps = vec![];
        let empty: Vec<Value> = vec![];
        for c in manifest.get("capabilities").and_then(Value::as_array).unwrap_or(&empty) {
            let name = c.get("name").and_then(Value::as_str).unwrap_or("");
            let full = if name.contains('.') { name.to_string() } else { format!("{id}.{name}") };
            if !valid_capability(&full) || !full.starts_with(&format!("{id}.")) {
                return Err(DeltaError::bad_request(format!("invalid capability name {name:?}")));
            }
            caps.push(CapSpec {
                name: full,
                description: c.get("description").map(value_to_string).unwrap_or_default(),
                localizations: c.get("localizations").cloned().unwrap_or(Value::Null),
                params: c.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(empty_obj),
                admin: c.get("admin").and_then(Value::as_bool).unwrap_or(false),
                public: false,
                requires: c.get("requires").map(|v|serde_json::from_value(v.clone())).transpose().map_err(|_|DeltaError::bad_request("Invalid capability feature requirements"))?.unwrap_or_default(),
            });
        }
        crate::components::dependencies::parse(&manifest["metadata"]["ui"])?;
        let (tx, rx) = mpsc::unbounded_channel();
        let (closed, _) = watch::channel(false);
        Ok(Self {
            id,
            version,
            description,
            session: crate::util::random_hex(12),
            owner,
            caps,
            metadata: manifest.get("metadata").filter(|v| v.is_object()).cloned().unwrap_or_else(empty_obj),
            tx,
            rx: tokio::sync::Mutex::new(rx),
            pending: Mutex::new(HashMap::new()),
            last_seen: Mutex::new(Instant::now()),
            polling: AtomicUsize::new(0),
            closed,
        })
    }

    pub fn capability_names(&self) -> Vec<String> {
        self.caps.iter().map(|c| c.name.clone()).collect()
    }

    fn touch(&self) {
        *self.last_seen.lock().unwrap() = Instant::now();
    }

    /// Long-poll: the next invocation for this component, or None on timeout.
    pub async fn next(&self, timeout: Duration) -> Option<Value> {
        self.polling.fetch_add(1, Ordering::SeqCst);
        self.touch();
        let mut closed = self.closed.subscribe();
        let result = {
            let mut rx = self.rx.lock().await;
            tokio::select! {
                v = rx.recv() => v,
                _ = tokio::time::sleep(timeout) => None,
                _ = wait_true(&mut closed) => None,
            }
        };
        self.polling.fetch_sub(1, Ordering::SeqCst);
        self.touch();
        result
    }

    pub fn resolve(&self, inv_id: &str, response: Value) -> bool {
        self.touch();
        let slot = self.pending.lock().unwrap().remove(inv_id);
        match slot {
            Some((tx, _)) => tx.send(response).is_ok(),
            None => false,
        }
    }

    /// Only the authenticated component and its live invocation can borrow this identity.
    /// The caller is retained by the kernel, never supplied by plugin JSON.
    pub fn invocation_principal(&self, id: &str) -> Result<Principal> {
        self.pending.lock().unwrap().get(id).map(|(_, p)| p.clone())
            .ok_or_else(|| DeltaError::not_found("unknown or expired invocation"))
    }

    pub fn alive(&self) -> bool {
        self.polling.load(Ordering::SeqCst) > 0 || self.last_seen.lock().unwrap().elapsed() < HEARTBEAT_TIMEOUT
    }
}

#[async_trait]
impl Component for ExternalComponent {
    fn id(&self) -> &str {
        &self.id
    }
    fn version(&self) -> String {
        self.version.clone()
    }
    fn description(&self) -> String {
        self.description.clone()
    }
    fn kind(&self) -> &'static str {
        "external"
    }
    fn metadata(&self) -> Value { self.metadata.clone() }
    fn capabilities(&self) -> Vec<CapSpec> {
        self.caps.clone()
    }

    async fn handle(&self, capability: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        let inv_id = crate::util::random_hex(8);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(inv_id.clone(), (tx, ctx.principal.clone()));
        let sent = self.tx.send(json!({
            "id": inv_id,
            "capability": capability,
            "args": args,
            "caller": ctx.principal.to_value(),
        }));
        let outcome = if sent.is_err() {
            Err(DeltaError::unavailable(format!("{} went offline", self.id)))
        } else {
            match tokio::time::timeout(ctx.kernel.call_timeout, rx).await {
                Ok(Ok(resp)) => {
                    if resp.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
                    } else {
                        let err = resp.get("error").cloned().unwrap_or(Value::Null);
                        Err(DeltaError::from_value(&err))
                    }
                }
                Ok(Err(_)) => Err(DeltaError::unavailable(format!("{} went offline", self.id))),
                Err(_) => Err(DeltaError::timeout(format!("{} did not answer {}", self.id, capability))),
            }
        };
        self.pending.lock().unwrap().remove(&inv_id);
        outcome
    }

    async fn stop(&self) {
        let _ = self.closed.send(true);
        let slots: Vec<oneshot::Sender<Value>> = self.pending.lock().unwrap().drain().map(|(_, (s, _))| s).collect();
        for s in slots {
            let _ = s.send(json!({
                "ok": false,
                "error": {"code": "unavailable", "message": format!("{} went offline", self.id)}
            }));
        }
    }
}

// ------------------------------------------------------------------ kernel

#[derive(Clone)]
struct Entry {
    comp: Arc<dyn Component>,
    ext: Option<Arc<ExternalComponent>>,
    seq: u64,
}

pub struct Kernel {
    pub config: Arc<Config>,
    pub node_id: String,
    pub name: String,
    pub version: &'static str,
    started_at: Instant,
    pub tokens: TokenStore,
    pub call_timeout: Duration,
    components: RwLock<HashMap<String, Entry>>,
    seq: AtomicU64,
    authenticators: RwLock<Vec<Arc<dyn Authenticator>>>,
    router: RwLock<Option<Arc<dyn Router>>>,
    local_url: RwLock<String>,
    stop_tx: watch::Sender<bool>,
    ready_tx: watch::Sender<bool>,
}

impl Kernel {
    pub fn new(config: Arc<Config>) -> Arc<Self> {
        let (node_id, name) = config.read(|c| (c.node_id.clone(), c.name.clone()));
        let (stop_tx, _) = watch::channel(false);
        let (ready_tx, _) = watch::channel(false);
        Arc::new(Self {
            tokens: TokenStore::new(config.clone()),
            config,
            node_id,
            name,
            version: VERSION,
            started_at: Instant::now(),
            call_timeout: DEFAULT_CALL_TIMEOUT,
            components: RwLock::new(HashMap::new()),
            seq: AtomicU64::new(0),
            authenticators: RwLock::new(vec![]),
            router: RwLock::new(None),
            local_url: RwLock::new(String::new()),
            stop_tx,
            ready_tx,
        })
    }

    // -- identity
    pub fn info(&self) -> Value {
        json!({
            "node_id": self.node_id, "kind": self.config.read(|c| c.node_kind.clone()),
            "name": self.name,
            "version": self.version,
            "url": self.config.read(|c| c.url.clone()),
            "uptime": round1(self.started_at.elapsed().as_secs_f64()),
        })
    }

    // -- wiring
    pub fn router(&self) -> Option<Arc<dyn Router>> {
        self.router.read().unwrap().clone()
    }
    pub fn set_router(&self, r: Option<Arc<dyn Router>>) {
        *self.router.write().unwrap() = r;
    }
    pub fn add_authenticator(&self, a: Arc<dyn Authenticator>) {
        self.authenticators.write().unwrap().push(a);
    }
    pub fn clear_authenticators(&self) {
        self.authenticators.write().unwrap().clear();
    }
    pub fn local_url(&self) -> String {
        self.local_url.read().unwrap().clone()
    }
    pub fn set_local_url(&self, url: String) {
        *self.local_url.write().unwrap() = url;
    }
    pub fn set_ready(&self) {
        // send() drops the value if the spawned supervisor has not subscribed yet.
        // Readiness is durable state, not a notification: retain it for late subscribers.
        self.ready_tx.send_replace(true);
    }
    pub async fn wait_ready(&self) {
        let mut rx = self.ready_tx.subscribe();
        wait_true(&mut rx).await;
    }

    // -- registry
    pub async fn add(self: &Arc<Self>, component: Arc<dyn Component>) -> Result<()> {
        self.add_entry(component, None).await
    }

    pub async fn add_external(self: &Arc<Self>, ext: Arc<ExternalComponent>) -> Result<()> {
        let comp: Arc<dyn Component> = ext.clone();
        self.add_entry(comp, Some(ext)).await
    }

    async fn add_entry(self: &Arc<Self>, comp: Arc<dyn Component>, ext: Option<Arc<ExternalComponent>>) -> Result<()> {
        if !valid_id(comp.id()) {
            return Err(DeltaError::bad_request(format!("invalid component id {:?}", comp.id())));
        }
        let replaced = {
            let mut map = self.components.write().unwrap();
            if let Some(existing) = map.get(comp.id()) {
                if existing.ext.is_none() || ext.is_none() {
                    return Err(DeltaError::conflict(format!("component {} already registered", comp.id())));
                }
            }
            let names: HashSet<String> = comp.capabilities().into_iter().map(|c| c.name).collect();
            for (cid, other) in map.iter() {
                if cid != comp.id() && other.comp.capabilities().iter().any(|c| names.contains(&c.name)) {
                    return Err(DeltaError::conflict(format!("capability name clash with {cid}")));
                }
            }
            let seq = self.seq.fetch_add(1, Ordering::SeqCst);
            let old = map.insert(comp.id().to_string(), Entry { comp: comp.clone(), ext, seq });
            old
        };
        // a restarted external process replaces its old session
        if let Some(old) = replaced {
            old.comp.stop().await;
        }
        comp.start(self);
        Ok(())
    }

    pub async fn remove(&self, component_id: &str) -> bool {
        let entry = self.components.write().unwrap().remove(component_id);
        match entry {
            Some(e) => {
                e.comp.stop().await;
                true
            }
            None => false,
        }
    }

    pub fn get_external(&self, component_id: &str, session: &str) -> Result<Arc<ExternalComponent>> {
        let entry = self.components.read().unwrap().get(component_id).cloned();
        match entry.and_then(|e| e.ext) {
            Some(ext) if crate::util::ct_eq(ext.session.as_bytes(), session.as_bytes()) => Ok(ext),
            _ => Err(DeltaError::not_found("unknown component session")),
        }
    }

    fn entries(&self) -> Vec<Entry> {
        self.components.read().unwrap().values().cloned().collect()
    }

    pub fn catalog(&self) -> Vec<Value> {
        let mut out: Vec<(String, Value)> = vec![];
        for e in self.entries() {
            for c in e.comp.capabilities() {
                out.push((
                    c.name.clone(),
                    json!({
                        "name": c.name, "description": c.description, "localizations": c.localizations, "params": c.params,
                        "admin": c.admin, "public": c.public, "requires": c.requires, "component": e.comp.id(),
                        "node": self.node_id, "node_name": self.name,
                    }),
                ));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.into_iter().map(|(_, v)| v).collect()
    }

    pub fn component_list(&self) -> Vec<Value> {
        let mut es = self.entries();
        es.sort_by_key(|e| e.seq);
        es.iter()
            .map(|e| {
                json!({
                    "id": e.comp.id(), "version": e.comp.version(), "description": e.comp.description(),
                    "kind": e.comp.kind(), "capabilities": e.comp.capabilities().len(),
                    "metadata": e.comp.metadata(),
                })
            })
            .collect()
    }

    fn find(&self, capability: &str) -> Option<(Arc<dyn Component>, CapSpec)> {
        for e in self.entries() {
            for c in e.comp.capabilities() {
                if c.name == capability {
                    return Some((e.comp.clone(), c));
                }
            }
        }
        None
    }

    // -- calls
    pub fn is_local(&self, node: Option<&str>) -> bool {
        match node {
            None => true,
            Some(n) => n.is_empty() || n == self.node_id || n == self.name || n == "local",
        }
    }

    pub async fn call(
        self: &Arc<Self>,
        capability: &str,
        args: Value,
        principal: &Principal,
        node: Option<&str>,
    ) -> Result<Value> {
        let args = if args.is_null() { json!({}) } else { args };
        if !valid_capability(capability) {
            return Err(DeltaError::bad_request("invalid capability name"));
        }
        if !self.is_local(node) {
            let node = node.unwrap_or("");
            if principal.kind == Kind::Peer && !principal.is_admin() {
                return Err(DeltaError::forbidden("forwarded calls cannot be forwarded again"));
            }
            let Some(router) = self.router() else {
                return Err(DeltaError::not_found(format!("unknown node {node}")));
            };
            // local authorization first; the remote node authorizes us again as a peer
            // only the target's own catalog is consulted (one slow peer must not delay calls to others);
            // fail-closed: a capability missing from the answer is treated as admin-only
            let caps = router.remote_capabilities(node).await?;
            let admin_only = caps.iter().find(|v| v["name"] == capability).and_then(|v| v["admin"].as_bool()).unwrap_or(true);
            if !principal.can_call(capability, admin_only, false) {
                return Err(DeltaError::forbidden(format!("not allowed to call {capability}")));
            }
            return router.call(node, capability, args).await;
        }
        let Some((comp, meta)) = self.find(capability) else {
            return Err(DeltaError::not_found(format!("capability {capability} not found on {}", self.name)));
        };
        if !principal.can_call(capability, meta.admin, meta.public) {
            let code = if principal.kind != Kind::Anonymous { "forbidden" } else { "unauthorized" };
            return Err(DeltaError::new(code, format!("not allowed to call {capability}")));
        }
        crate::components::dependencies::enforce(self, &comp.metadata()["ui"], &meta.requires)?;
        validate_args(&meta.params, &args)?;
        let ctx = CallContext { kernel: self.clone(), principal: principal.clone(), capability: capability.to_string() };
        comp.handle(capability, args, &ctx).await
    }

    pub fn authenticate(&self, method: &str, path: &str, headers: &HeaderMap, body: &[u8]) -> Result<Principal> {
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
        if let Some(token) = auth.strip_prefix("Bearer ") {
            return self
                .tokens
                .authenticate(token.trim())
                .ok_or_else(|| DeltaError::unauthorized("invalid token"));
        }
        let auths: Vec<Arc<dyn Authenticator>> = self.authenticators.read().unwrap().clone();
        for a in auths {
            if let Some(p) = a.authenticate(method, path, headers, body)? {
                return Ok(p);
            }
        }
        Ok(Principal::anonymous())
    }

    // -- lifecycle
    pub fn start(self: &Arc<Self>) {
        let k = self.clone();
        let mut stop = self.stop_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    _ = stop.changed() => break,
                }
                let dead: Vec<String> = k
                    .components
                    .read()
                    .unwrap()
                    .iter()
                    .filter(|(_, e)| e.ext.as_ref().map(|x| !x.alive()).unwrap_or(false))
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in dead {
                    k.remove(&id).await;
                }
            }
        });
    }

    pub async fn stop(&self) {
        let _ = self.stop_tx.send(true);
        let mut es: Vec<(String, u64)> =
            self.components.read().unwrap().iter().map(|(id, e)| (id.clone(), e.seq)).collect();
        es.sort_by(|a, b| b.1.cmp(&a.1)); // newest first: supervisor stops before the rest
        for (id, _) in es {
            self.remove(&id).await;
        }
    }
}
