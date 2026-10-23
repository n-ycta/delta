//! Native root-document bridge. Never expose this transport directly to plugin frames.
use delta::{
    auth::{Kind, Principal},
    client::Client,
    config::{default_config, Config},
    errors::{DeltaError, Result},
    node::{HostPolicy, Node},
};
use fs2::FileExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
pub const BRIDGE_VERSION: &str = "1.0.0";
pub const MAX_REQUEST_BYTES: usize = 65536;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub capability: String,
    #[serde(default = "empty")]
    pub args: Value,
    pub node: Option<String>,
}
fn empty() -> Value {
    json!({})
}
impl Request {
    pub fn parse(value: Value) -> Result<Self> {
        if serde_json::to_vec(&value)
            .map_err(|_| DeltaError::bad_request("invalid_request"))?
            .len()
            > MAX_REQUEST_BYTES
        {
            return Err(DeltaError::bad_request("request_too_large"));
        }
        let request: Self = serde_json::from_value(value)
            .map_err(|_| DeltaError::bad_request("invalid_request"))?;
        if request.capability.len() > 128
            || !delta::util::valid_capability(&request.capability)
            || !request.args.is_object()
            || request
                .node
                .as_ref()
                .is_some_and(|n| n.is_empty() || n.len() > 128)
        {
            return Err(DeltaError::bad_request("invalid_request"));
        }
        Ok(request)
    }
}
enum Backend {
    Embedded(Mutex<Option<Node>>),
    Existing(Client),
}
pub struct HostRuntime {
    backend: Backend,
    kind: String,
    node_id: String,
    core_version: String,
    config: Arc<Config>,
    codec: Option<Arc<dyn delta::config::ProfileCodec>>,
    documents: std::sync::Mutex<()>,
    _lease: std::fs::File,
}
impl HostRuntime {
    /// Caller supplies a platform-owned profile path, never a path received from JavaScript.
    pub async fn open(home: PathBuf, kind: &str) -> Result<Self> {
        Self::open_with_codec(home, kind, None).await
    }
    pub async fn open_with_codec(
        home: PathBuf,
        kind: &str,
        codec: Option<Arc<dyn delta::config::ProfileCodec>>,
    ) -> Result<Self> {
        if !["desktop", "android"].contains(&kind) {
            return Err(DeltaError::bad_request("invalid_host_kind"));
        }
        let existed = Config::exists(&home);
        let cfg = Arc::new(if existed {
            (if let Some(c) = &codec {
                Config::load_with_codec(home.clone(), c.clone())
            } else {
                Config::load(home.clone())
            })
            .map_err(|_| DeltaError::unavailable("profile_unreadable"))?
        } else {
            let mut data = default_config("Delta", "127.0.0.1", 0, "");
            data.node_kind = kind.into();
            #[cfg(not(feature = "desktop"))]
            {
                data.builtin.system = false;
                data.builtin.supervisor = false;
                data.builtin.exec = false;
            }
            let c = if let Some(codec) = &codec {
                Config::new_with_codec(home.clone(), data, codec.clone())
            } else {
                Config::new(home.clone(), data)
            };
            c.save()
                .map_err(|_| DeltaError::unavailable("profile_not_saved"))?;
            c
        });
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lease = options
            .open(home.join("host-runtime.lock"))
            .map_err(|_| DeltaError::unavailable("profile_lock_failed"))?;
        lease
            .try_lock_exclusive()
            .map_err(|_| DeltaError::new("conflict", "profile_already_open"))?;
        let node_id = cfg.read(|c| c.node_id.clone());
        if existed && cfg.read(|c| c.port) != 0 {
            let port = cfg.read(|c| c.port);
            // Existing profile authentication stays in Rust; verify identity before attaching.
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                let token = std::fs::read_to_string(home.join("admin.token")).map_err(|_| {
                    DeltaError::forbidden("existing_profile_requires_owner_credential")
                })?;
                let client = Client::new(
                    &format!("http://127.0.0.1:{port}"),
                    Some(token.trim().into()),
                )
                .with_timeout(Duration::from_secs(35));
                let info = client.call("core.info", json!({}), None, None).await?;
                if info["node_id"] != node_id {
                    return Err(DeltaError::forbidden("node_changed"));
                }
                return Ok(Self {
                    backend: Backend::Existing(client),
                    kind: kind.into(),
                    node_id,
                    core_version: info["version"].as_str().unwrap_or("unknown").into(),
                    config: cfg.clone(),
                    codec: codec.clone(),
                    documents: std::sync::Mutex::new(()),
                    _lease: lease,
                });
            }
        }
        // No unrequested second identity/profile is created. Existing bind failures remain errors.
        let policy = if kind == "android" {
            HostPolicy::MOBILE
        } else {
            HostPolicy::DESKTOP
        };
        let node = Node::start_with_policy(cfg.clone(), None, policy).await?;
        Ok(Self {
            backend: Backend::Embedded(Mutex::new(Some(node))),
            kind: kind.into(),
            node_id,
            core_version: delta::VERSION.into(),
            config: cfg.clone(),
            codec: codec.clone(),
            documents: std::sync::Mutex::new(()),
            _lease: lease,
        })
    }
    pub async fn call(&self, value: Value) -> Result<Value> {
        let request = Request::parse(value)?;
        if request.capability.starts_with("host.document_") {
            return self.document(&request.capability, &request.args);
        }
        if request.capability == "host.manifests" {
            let mut rows = vec![];
            self.config.read(|c| {
                for m in &c.components {
                    for arg in &m.command {
                        let path = std::path::Path::new(arg);
                        if !path.is_absolute() {
                            continue;
                        }
                        if let Some(dir) = path.parent() {
                            if let Ok(b) = std::fs::read(dir.join("manifest.json")) {
                                if b.len() > 200_000 {
                                    continue;
                                }
                                if let Ok(mut v) = serde_json::from_slice::<Value>(&b) {
                                    if v["id"] == m.id {
                                        v["enabled"] = json!(m.enabled);
                                        rows.push(v);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            });
            return Ok(json!(rows));
        }

        match &self.backend {
            Backend::Existing(client) => {
                client
                    .call(
                        &request.capability,
                        request.args,
                        request.node.as_deref(),
                        None,
                    )
                    .await
            }
            Backend::Embedded(state) => {
                let kernel = state
                    .lock()
                    .await
                    .as_ref()
                    .ok_or_else(|| DeltaError::unavailable("node_stopped"))?
                    .kernel
                    .clone();
                let principal = Principal {
                    kind: Kind::Admin,
                    name: "native-owner".into(),
                    scopes: vec!["admin".into()],
                    peer_id: String::new(),
                };
                kernel
                    .call(
                        &request.capability,
                        request.args,
                        &principal,
                        request.node.as_deref(),
                    )
                    .await
            }
        }
    }
    pub fn info(&self, host_version: &str, ui_version: &str) -> Value {
        let embedded = matches!(&self.backend, Backend::Embedded(_));
        json!({"kind":self.kind,"version":host_version,"bridgeVersion":BRIDGE_VERSION,"coreVersion":self.core_version,"uiVersion":ui_version,"nodeId":self.node_id,"nodeMode":if embedded{"embedded"}else{"attached"},"features":[if embedded{"embedded-node"}else{"attached-node"},"capability-api","persistent-profile","native-documents","full-ui"],"lifecycle":if !embedded{"external"}else if self.kind=="android"{"foreground"}else{"tray"},"development":false})
    }

    fn document(&self, cap: &str, a: &Value) -> Result<Value> {
        let _guard = self.documents.lock().unwrap();
        let ns = a["namespace"].as_str().unwrap_or("");
        if !["dashboards", "notifications", "manifests", "preferences"].contains(&ns) {
            return Err(DeltaError::forbidden("Invalid document namespace"));
        }
        let dir = self.config.home.join("data/native-ui");
        std::fs::create_dir_all(&dir)
            .map_err(|_| DeltaError::unavailable("Document store unavailable"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| DeltaError::unavailable("Cannot secure document store"))?;
        }
        let path = dir.join(format!("{ns}.json"));
        let mut docs: serde_json::Map<String, Value> = match std::fs::read(&path) {
            Ok(b) => {
                let bytes = if let Some(c) = &self.codec {
                    c.open(&b)
                        .map_err(|_| DeltaError::unavailable("Document key unavailable"))?
                } else {
                    b
                };
                serde_json::from_slice(&bytes)
                    .map_err(|_| DeltaError::unavailable("Invalid document store"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
            Err(_) => return Err(DeltaError::unavailable("Document store unreadable")),
        };
        if cap == "host.document_list" {
            return Ok(json!(docs.values().collect::<Vec<_>>()));
        }
        let key = a["key"].as_str().unwrap_or("");
        if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
            return Err(DeltaError::bad_request("Invalid document key"));
        }
        let result = match cap {
            "host.document_put" => {
                if !a["value"].is_object() {
                    return Err(DeltaError::bad_request("Document must be an object"));
                }
                docs.insert(key.into(), a["value"].clone());
                a["value"].clone()
            }
            "host.document_delete" => {
                docs.remove(key);
                json!({"deleted":true})
            }
            _ => return Err(DeltaError::not_found("Unknown document operation")),
        };
        let b =
            serde_json::to_vec(&docs).map_err(|_| DeltaError::bad_request("Invalid documents"))?;
        if docs.len() > 1000 || b.len() > 2_000_000 {
            return Err(DeltaError::bad_request("Document capacity exceeded"));
        }
        let b = if let Some(c) = &self.codec {
            c.seal(&b)
                .map_err(|_| DeltaError::unavailable("Document key unavailable"))?
        } else {
            b
        };
        use std::io::Write;
        let tmp = dir.join(format!(".{ns}-{}", delta::util::random_hex(8)));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp)
            .map_err(|_| DeltaError::unavailable("Document write failed"))?;
        f.write_all(&b)
            .and_then(|_| f.sync_all())
            .map_err(|_| DeltaError::unavailable("Document write failed"))?;
        std::fs::rename(tmp, path)
            .map_err(|_| DeltaError::unavailable("Document commit failed"))?;
        Ok(result)
    }
    pub async fn stop(&self) {
        if let Backend::Embedded(state) = &self.backend {
            let node = state.lock().await.take();
            if let Some(node) = node {
                node.stop().await
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_contract() {
        assert!(Request::parse(json!({"capability":"core.info"})).is_ok());
        for bad in [
            json!({"capability":"../exec"}),
            json!({"capability":"core.info","url":"http://evil"}),
            json!({"capability":"core.info","args":[]}),
            json!({"capability":"core.info","args":{"x":"x".repeat(70000)}}),
        ] {
            assert!(Request::parse(bad).is_err())
        }
    }
    #[tokio::test]
    async fn portable_identity_survives_restart() {
        let home = std::env::temp_dir().join(format!("delta-host-{}", delta::util::random_hex(10)));
        let h = HostRuntime::open(home.clone(), "android").await.unwrap();
        let a = h.call(json!({"capability":"core.info"})).await.unwrap();
        let caps = h
            .call(json!({"capability":"core.capabilities"}))
            .await
            .unwrap();
        #[cfg(not(feature = "desktop"))]
        assert!(!caps.as_array().unwrap().iter().any(|c| c["name"]
            .as_str()
            .unwrap_or("")
            .starts_with("supervisor.")
            || c["name"].as_str().unwrap_or("").starts_with("packages.")));
        h.stop().await;
        drop(h);
        let next = HostRuntime::open(home.clone(), "android").await.unwrap();
        assert_eq!(
            a["node_id"],
            next.call(json!({"capability":"core.info"})).await.unwrap()["node_id"]
        );
        next.stop().await;
        std::fs::remove_dir_all(home).unwrap();
    }
}
#[cfg(test)]
mod persistence_tests {
    use super::*;
    struct TestCodec;
    impl delta::config::ProfileCodec for TestCodec {
        fn seal(&self, b: &[u8]) -> std::io::Result<Vec<u8>> {
            Ok([
                b"TEST:".as_slice(),
                &b.iter().map(|x| x ^ 0x5a).collect::<Vec<_>>(),
            ]
            .concat())
        }
        fn open(&self, b: &[u8]) -> std::io::Result<Vec<u8>> {
            if !b.starts_with(b"TEST:") {
                return Err(std::io::Error::other("wrong key"));
            }
            Ok(b[5..].iter().map(|x| x ^ 0x5a).collect())
        }
    }
    #[tokio::test]
    async fn encrypted_profile_and_documents_survive_restart() {
        let dir =
            std::env::temp_dir().join(format!("delta-private-{}", delta::util::random_hex(10)));
        let h = HostRuntime::open_with_codec(dir.clone(), "android", Some(Arc::new(TestCodec)))
            .await
            .unwrap();
        let id = h.call(json!({"capability":"core.info"})).await.unwrap()["node_id"].clone();
        h.call(json!({"capability":"host.document_put","args":{"namespace":"dashboards","key":"x","value":{"id":"x","private":"example"}}})).await.unwrap();
        assert!(h
            .call(json!({"capability":"host.document_list","args":{"namespace":"../config"}}))
            .await
            .is_err());
        assert!(
            !String::from_utf8_lossy(&std::fs::read(dir.join("config.json")).unwrap())
                .contains(id.as_str().unwrap())
        );
        assert!(!String::from_utf8_lossy(
            &std::fs::read(dir.join("data/native-ui/dashboards.json")).unwrap()
        )
        .contains("example"));
        h.stop().await;
        drop(h);
        assert!(Config::load(dir.clone()).is_err());
        let next = HostRuntime::open_with_codec(dir.clone(), "android", Some(Arc::new(TestCodec)))
            .await
            .unwrap();
        assert_eq!(
            next.call(json!({"capability":"core.info"})).await.unwrap()["node_id"],
            id
        );
        assert_eq!(
            next.call(json!({"capability":"host.document_list","args":{"namespace":"dashboards"}}))
                .await
                .unwrap()[0]["private"],
            "example"
        );
        next.stop().await;
        drop(next);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
pub use delta::config::ProfileCodec;
