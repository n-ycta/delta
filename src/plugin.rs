//! SDK for external components written in Rust.
//!
//! ```no_run
//! use delta::plugin::Plugin;
//! use serde_json::json;
//!
//! #[tokio::main]
//! async fn main() {
//!     let p = Plugin::new("hello", "1.0.0", "Says hello").capability(
//!         "greet", "Greets someone",
//!         json!({"name": {"type": "string", "required": true}}), false,
//!         |args, _caller| async move { Ok(json!({"text": format!("Hello, {}", args["name"])})) },
//!     );
//!     p.run(None, None).await.unwrap(); // DELTA_URL / DELTA_TOKEN from the environment
//! }
//! ```
use crate::client::Client;
use crate::errors::{DeltaError, Result};
use crate::kernel::CapSpec;
use serde_json::{json, Value};
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};

type HandlerFuture = Pin<Box<dyn Future<Output = Result<Value>> + Send>>;
type Handler = Arc<dyn Fn(Value, Value) -> HandlerFuture + Send + Sync>;

pub struct Plugin {
    id: String,
    version: String,
    description: String,
    handlers: HashMap<String, Handler>,
    specs: Vec<CapSpec>,
}

impl Plugin {
    pub fn new(id: &str, version: &str, description: &str) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            description: description.into(),
            handlers: HashMap::new(),
            specs: vec![],
        }
    }

    /// Register `<id>.<name>`. The handler gets (args, caller).
    pub fn capability<F, Fut>(mut self, name: &str, description: &str, params: Value, admin: bool, f: F) -> Self
    where
        F: Fn(Value, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value>> + Send + 'static,
    {
        let full = format!("{}.{}", self.id, name);
        let h: Handler = Arc::new(move |a: Value, c: Value| -> HandlerFuture { Box::pin(f(a, c)) });
        self.handlers.insert(full.clone(), h);
        self.specs.push(CapSpec { name: full, description: description.into(), params, admin, public: false });
        self
    }

    pub fn manifest(&self) -> Value {
        json!({
            "id": self.id, "version": self.version, "description": self.description,
            "capabilities": self.specs,
        })
    }

    /// Register, then serve invocations until a fatal error (forbidden/unauthorized/bad_request/conflict).
    pub async fn run(self, url: Option<String>, token: Option<String>) -> Result<()> {
        let url = url
            .or_else(|| std::env::var("DELTA_URL").ok())
            .unwrap_or_else(|| "http://127.0.0.1:7700".into());
        let token = token.or_else(|| std::env::var("DELTA_TOKEN").ok());
        let client = Arc::new(Client::new(&url, token));
        let manifest = self.manifest();
        let id = self.id.clone();
        let handlers = Arc::new(self.handlers);
        let mut session: Option<String> = None;
        let mut backoff = 0.5f64;
        loop {
            match step(&client, &id, &manifest, &mut session, &handlers).await {
                Ok(registered_now) => {
                    if registered_now {
                        backoff = 0.5;
                    }
                }
                Err(e) => {
                    if matches!(e.code.as_str(), "forbidden" | "unauthorized" | "bad_request" | "conflict") {
                        return Err(e);
                    }
                    session = None; // node restarted or unreachable: re-register
                    tokio::time::sleep(Duration::from_secs_f64(backoff)).await;
                    backoff = (backoff * 2.0).min(10.0);
                }
            }
        }
    }
}

async fn step(
    client: &Arc<Client>,
    id: &str,
    manifest: &Value,
    session: &mut Option<String>,
    handlers: &Arc<HashMap<String, Handler>>,
) -> Result<bool> {
    let mut registered_now = false;
    if session.is_none() {
        let r = client.request("POST", "/v1/components", Some(manifest), None).await?;
        *session = Some(r.get("session").and_then(Value::as_str).unwrap_or("").to_string());
        registered_now = true;
    }
    let s = session.clone().unwrap_or_default();
    let res = client
        .request(
            "GET",
            &format!("/v1/components/{id}/next?session={s}&timeout=20"),
            None,
            Some(Duration::from_secs(30)),
        )
        .await?;
    if let Some(inv) = res.get("invocation").filter(|v| !v.is_null()).cloned() {
        let client = client.clone();
        let handlers = handlers.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let inv_id = inv.get("id").cloned().unwrap_or(Value::Null);
            let cap = inv.get("capability").and_then(Value::as_str).unwrap_or("").to_string();
            let args = inv.get("args").cloned().unwrap_or_else(|| json!({}));
            let caller = inv.get("caller").cloned().unwrap_or_else(|| json!({}));
            let outcome = match handlers.get(&cap) {
                None => Err(DeltaError::not_found(cap.clone())),
                Some(h) => h(args, caller).await,
            };
            let reply = match outcome {
                Ok(v) => json!({"id": inv_id, "ok": true, "result": v}),
                Err(e) => json!({"id": inv_id, "ok": false, "error": e.to_value()}),
            };
            // the invocation may have expired or the node gone away; the caller already got an error
            let _ = client
                .request("POST", &format!("/v1/components/{id}/result?session={s}"), Some(&reply), None)
                .await;
        });
    }
    Ok(registered_now)
}
