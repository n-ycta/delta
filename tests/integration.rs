//! Integration tests: real nodes on random local ports, each with its own DELTA_HOME.
use delta::auth::TokenStore;
use delta::client::Client;
use delta::config::{default_config, Config};
use delta::node::Node;
use delta::plugin::Plugin;
use delta::util::random_hex;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct TestNode {
    node: Node,
    home: PathBuf,
    url: String,
    admin: Client,
    admin_token: String,
}

impl TestNode {
    async fn new(name: &str) -> TestNode {
        let home = std::env::temp_dir().join(format!("delta-{name}-{}", random_hex(4)));
        let cfg = Arc::new(Config::new(home.clone(), default_config(name, "127.0.0.1", 0, "")));
        cfg.save().unwrap();
        let (admin_token, _) = TokenStore::new(cfg.clone()).create("admin", &["admin".to_string()], true).unwrap();
        let node = Node::start(cfg.clone(), Some(0)).await.unwrap();
        let url = format!("http://127.0.0.1:{}", node.port());
        let u = url.clone();
        cfg.update(move |c| c.url = u);
        let admin = Client::new(&url, Some(admin_token.clone()));
        TestNode { node, home, url, admin, admin_token }
    }

    fn client(&self, token: Option<&str>) -> Client {
        Client::new(&self.url, token.map(String::from))
    }

    async fn close(self) {
        self.node.stop().await;
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

async fn code_of(r: Result<Value, delta::errors::DeltaError>) -> String {
    r.err().map(|e| e.code).unwrap_or_else(|| "ok".into())
}

#[tokio::test]
async fn health_open_and_anonymous_rejected() {
    let n = TestNode::new("alpha").await;
    assert_eq!(n.client(None).health().await.unwrap()["status"], "ok");
    assert_eq!(code_of(n.client(None).capabilities("local").await).await, "unauthorized");
    assert_eq!(code_of(n.client(None).call("core.info", json!({}), None, None).await).await, "unauthorized");
    assert_eq!(code_of(n.client(Some("dlt_nope")).call("core.info", json!({}), None, None).await).await, "unauthorized");
    let info = n.admin.call("core.info", json!({}), None, None).await.unwrap();
    assert_eq!(info["name"], "alpha");
    n.close().await;
}

#[tokio::test]
async fn validation_and_scopes() {
    let n = TestNode::new("beta").await;
    assert_eq!(code_of(n.admin.call("system.ping", json!({"echo": 5}), None, None).await).await, "bad_request");
    assert_eq!(code_of(n.admin.call("nothing.here", json!({}), None, None).await).await, "not_found");
    assert_eq!(code_of(n.admin.call("BAD NAME", json!({}), None, None).await).await, "bad_request");

    let t = n.admin.call("tokens.create", json!({"name": "viewer", "scopes": ["call:system.*"]}), None, None).await.unwrap();
    let viewer = n.client(t["token"].as_str());
    assert!(viewer.call("system.info", json!({}), None, None).await.unwrap().get("hostname").is_some());
    assert_eq!(code_of(viewer.call("core.info", json!({}), None, None).await).await, "forbidden");

    // call:* never grants admin-only capabilities
    let t2 = n.admin.call("tokens.create", json!({"name": "all", "scopes": ["call:*"]}), None, None).await.unwrap();
    let all = n.client(t2["token"].as_str());
    let r = all.call("tokens.create", json!({"name": "x", "scopes": ["admin"]}), None, None).await;
    assert_eq!(code_of(r).await, "forbidden");
    n.close().await;
}

#[tokio::test]
async fn external_plugin_roundtrip() {
    let n = TestNode::new("gamma").await;
    let plugin = Plugin::new("hello", "1.0.0", "Says hello").capability(
        "greet",
        "Greets someone",
        json!({"name": {"type": "string", "required": true}}),
        false,
        |args, _caller| async move { Ok(json!({"text": format!("Hello, {}", args["name"].as_str().unwrap_or(""))})) },
    );
    let (url, token) = (n.url.clone(), n.admin_token.clone());
    let task = tokio::spawn(async move { plugin.run(Some(url), Some(token)).await });

    let mut result = None;
    for _ in 0..100 {
        if let Ok(v) = n.admin.call("hello.greet", json!({"name": "Rust"}), None, None).await {
            result = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(result.expect("plugin never came online")["text"], "Hello, Rust");
    task.abort();
    n.close().await;
}

#[tokio::test]
async fn two_nodes_pair_and_route_calls() {
    let a = TestNode::new("node-a").await;
    let b = TestNode::new("node-b").await;
    let inv = a.admin.call("peers.invite", json!({}), None, None).await.unwrap();
    let joined = b
        .admin
        .call("peers.join", json!({"url": a.url, "code": inv["code"]}), None, None)
        .await
        .unwrap();
    assert_eq!(joined["name"], "node-a");

    // B calls a capability that lives on A, through the signed peer channel
    let r = b.admin.call("system.ping", json!({"echo": "hi"}), Some("node-a"), None).await.unwrap();
    assert_eq!(r["echo"], "hi");
    assert_eq!(r["node"], "node-a");
    // and the other way round
    let r = a.admin.call("system.ping", json!({"echo": "yo"}), Some("node-b"), None).await.unwrap();
    assert_eq!(r["node"], "node-b");

    // an invite code is single-use
    let again = b.admin.call("peers.join", json!({"url": a.url, "code": inv["code"]}), None, None).await;
    assert_eq!(code_of(again).await, "forbidden");
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn external_metadata_and_caller_scopes_are_preserved() {
    let n = TestNode::new("metadata").await;
    let manifest = json!({"id":"inspect","version":"1.0.0","metadata":{"ui":{"id":"dev.delta.inspect","contract":"^1.4.0"}},"capabilities":[{"name":"identity"}]});
    let registration = n.admin.request("POST", "/v1/components", Some(&manifest), None).await.unwrap();
    let components = n.admin.call("core.components", json!({}), None, None).await.unwrap();
    assert_eq!(components.as_array().unwrap().iter().find(|v| v["id"] == "inspect").unwrap()["metadata"]["ui"]["id"], "dev.delta.inspect");
    let t = n.admin.call("tokens.create", json!({"name":"reader","scopes":["call:inspect.identity"],"ttl_seconds":60}), None, None).await.unwrap();
    assert!(t["expires_at"].as_f64().unwrap() > delta::util::now());
    let reader = n.client(t["token"].as_str());
    let pending = tokio::spawn(async move { reader.call("inspect.identity", json!({}), None, None).await });
    let ext = n.node.kernel.get_external("inspect", registration["session"].as_str().unwrap()).unwrap();
    let inv = ext.next(Duration::from_secs(2)).await.unwrap();
    assert_eq!(inv["caller"]["scopes"][0], "call:inspect.identity");
    ext.resolve(inv["id"].as_str().unwrap(), json!({"ok":true,"result":true}));
    assert_eq!(pending.await.unwrap().unwrap(), true);
    n.node.kernel.tokens.set_expiry(t["id"].as_str().unwrap(), Some(delta::util::now() - 1.0));
    assert_eq!(code_of(n.client(t["token"].as_str()).call("inspect.identity", json!({}), None, None).await).await, "unauthorized");
    n.close().await;
}
