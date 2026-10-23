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
        Self::with_config(name, |_| {}).await
    }

    async fn with_config(name: &str, tweak: impl FnOnce(&mut delta::config::ConfigData)) -> TestNode {
        let home = std::env::temp_dir().join(format!("delta-{name}-{}", random_hex(4)));
        let mut data = default_config(name, "127.0.0.1", 0, "");
        tweak(&mut data);
        let cfg = Arc::new(Config::new(home.clone(), data));
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
    let inv = a.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
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
    // routing by node_id works too; the announced name is only a display label
    let b_id = b.admin.call("core.info", json!({}), None, None).await.unwrap()["node_id"].as_str().unwrap().to_string();
    let r = a.admin.call("system.ping", json!({"echo": "yo"}), Some(&b_id), None).await.unwrap();
    assert_eq!(r["node"], "node-b");
    let peers = a.admin.call("peers.list", json!({}), None, None).await.unwrap();
    assert_eq!(peers[0]["name"], "node-b");

    // an invite code is single-use
    let again = b.admin.call("peers.join", json!({"url": a.url, "code": inv["code"]}), None, None).await;
    assert_eq!(code_of(again).await, "forbidden");
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn invite_refuses_loopback_and_mints_a_phone_link() {
    let n = TestNode::new("phone-hub").await;
    assert_eq!(code_of(n.admin.call("peers.invite", json!({}), None, None).await).await, "bad_request");
    let minted = n.admin.call("peers.invite", json!({"url": "https://hub.example/delta"}), None, None).await.unwrap();
    let link = minted["link"].as_str().unwrap();
    assert!(link.starts_with("delta://pair?"), "{link}");
    assert!(link.contains("hub.example"), "{link}");
    assert!(link.contains("code="), "{link}");
    assert_eq!(minted["expires_in"], 600);
    assert_eq!(minted["url"], "https://hub.example/delta");
    assert_eq!(code_of(n.admin.call("peers.invite", json!({"url": "http://0.0.0.0:7700"}), None, None).await).await, "bad_request");
    n.close().await;
}

#[tokio::test]
async fn invite_refuses_port_zero_unless_allow_local() {
    let n = TestNode::new("zero-hub").await;
    for url in ["http://127.0.0.1:0", "http://hub.example:0"] {
        assert_eq!(code_of(n.admin.call("peers.invite", json!({"url": url}), None, None).await).await, "bad_request", "{url}");
        let ok = n.admin.call("peers.invite", json!({"url": url, "allow_local": true}), None, None).await.unwrap();
        assert!(ok["link"].as_str().unwrap().starts_with("delta://pair?"));
    }
    n.close().await;
}

#[tokio::test]
async fn peer_client_sends_no_bearer_and_plain_peers_cannot_forward() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    // a signing client must not leak its token as a Bearer header
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let n = s.read(&mut buf).await.unwrap();
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await;
        String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase()
    });
    let _ = Client::new(&url, Some("dlt_secret".into())).with_signer("n".into(), "s".into()).call("core.info", json!({}), None, None).await;
    let raw = seen.await.unwrap();
    assert!(raw.contains("x-delta-node") && !raw.contains("authorization"), "{raw}");

    // an ordinary (non-admin) peer cannot ask us to forward a call to a third node
    let a = TestNode::new("fwd-a").await;
    let b = TestNode::new("fwd-b").await;
    let inv = a.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    b.admin.call("peers.join", json!({"url": a.url, "code": inv["code"], "local_url": b.url}), None, None).await.unwrap();
    let secret = b.node.config.read(|c| c.peers[0].secret.clone());
    let b_id = b.admin.call("core.info", json!({}), None, None).await.unwrap()["node_id"].as_str().unwrap().to_string();
    let as_peer = Client::new(&a.url, None).with_signer(b_id, secret);
    assert_eq!(code_of(as_peer.call("system.ping", json!({}), Some("elsewhere"), None).await).await, "forbidden");
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn admin_phone_sees_hub_nodes() {
    let hub = TestNode::new("hub").await;
    let vps = TestNode::new("vps").await;
    let phone = TestNode::new("phone").await;
    phone.node.config.update(|c| c.url = "http://127.0.0.1:0".into());
    let id_of = |n: &TestNode| { let c = n.admin.clone(); async move { c.call("core.info", json!({}), None, None).await.unwrap()["node_id"].as_str().unwrap().to_string() } };
    let (vps_id, phone_id) = (id_of(&vps).await, id_of(&phone).await);

    let inv = hub.admin.call("peers.invite", json!({"allow_local": true, "trust_admin": true}), None, None).await.unwrap();
    vps.admin.call("peers.join", json!({"url": hub.url, "code": inv["code"], "local_url": vps.url}), None, None).await.unwrap();
    let inv = hub.admin.call("peers.invite", json!({"allow_local": true, "trust_admin": true}), None, None).await.unwrap();
    phone.admin.call("peers.join", json!({"url": hub.url, "code": inv["code"]}), None, None).await.unwrap();
    assert_eq!(hub.node.config.read(|c| c.peers.iter().find(|p| p.node_id == phone_id).unwrap().url.clone()), "", "phone is outbound-only on the hub");

    let mut rows = json!([]);
    for _ in 0..150 {
        rows = phone.admin.call("peers.list", json!({}), None, None).await.unwrap();
        if rows.as_array().unwrap().iter().any(|r| r["node_id"] == vps_id) { break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let rows = rows.as_array().unwrap();
    let v = rows.iter().find(|r| r["node_id"] == vps_id).expect("hub's node is discovered");
    assert_eq!(v["discovered"], true);
    assert_eq!(rows.iter().filter(|r| r["node_id"] == vps_id).count(), 1);
    assert!(rows.iter().all(|r| r["node_id"] != phone_id && !r["url"].as_str().unwrap_or("").ends_with(":0")), "no self or ghost rows: {rows:?}");
    assert!(rows.iter().any(|r| r["name"] == "hub" && r.get("discovered").is_none()));

    // the hub got no admin rights over the phone
    let hub_id = id_of(&hub).await;
    let on_phone = Client::new(&phone.url, None).with_signer(hub_id, phone.node.config.read(|c| c.peers[0].secret.clone()));
    assert_eq!(code_of(on_phone.call("tokens.list", json!({}), None, None).await).await, "forbidden");

    // the call goes phone -> hub -> vps
    let r = phone.admin.call("system.ping", json!({"echo": "via hub"}), Some(&vps_id), None).await.unwrap();
    assert_eq!((r["node"].as_str(), r["echo"].as_str()), (Some("vps"), Some("via hub")));

    // a non-admin token on the phone does not see discovered nodes
    let t = phone.admin.call("tokens.create", json!({"name": "viewer", "scopes": ["call:*"]}), None, None).await.unwrap();
    let listed = phone.client(t["token"].as_str()).call("peers.list", json!({}), None, None).await.unwrap();
    assert!(listed.as_array().unwrap().iter().all(|r| r["node_id"] != vps_id));
    hub.close().await;
    vps.close().await;
    phone.close().await;
}

#[tokio::test]
async fn hub_without_admin_trust_hides_its_nodes_from_the_phone() {
    let hub = TestNode::new("hub2").await;
    let vps = TestNode::new("vps2").await;
    let phone = TestNode::new("phone2").await;
    phone.node.config.update(|c| c.url = "http://127.0.0.1:0".into());
    let vps_id = vps.admin.call("core.info", json!({}), None, None).await.unwrap()["node_id"].as_str().unwrap().to_string();
    let inv = hub.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    vps.admin.call("peers.join", json!({"url": hub.url, "code": inv["code"], "local_url": vps.url}), None, None).await.unwrap();
    // the hub does not trust the phone as admin 
    let inv = hub.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    phone.admin.call("peers.join", json!({"url": hub.url, "code": inv["code"]}), None, None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let rows = phone.admin.call("peers.list", json!({}), None, None).await.unwrap();
    assert!(rows.as_array().unwrap().iter().all(|r| r["node_id"] != vps_id), "{rows:?}");
    assert_eq!(code_of(phone.admin.call("system.ping", json!({}), Some(&vps_id), None).await).await, "not_found");
    hub.close().await;
    vps.close().await;
    phone.close().await;
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

#[test]
fn cli_init_persists_node_kind() {
    let home = std::env::temp_dir().join(format!("delta-kind-{}", random_hex(4)));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_delta"))
        .env("DELTA_HOME", &home).args(["init", "--name", "test-vps", "--kind", "vps"]).output().unwrap();
    assert!(output.status.success());
    let config: Value = serde_json::from_slice(&std::fs::read(home.join("config.json")).unwrap()).unwrap();
    assert_eq!(config["node_kind"], "vps");
    let _ = std::fs::remove_dir_all(home);
}

#[tokio::test]
async fn signed_login_replay_revocation_and_scoped_admin() {
    use k256::schnorr::{SigningKey,signature::Signer};
    let n=TestNode::new("identity").await;
    let key=SigningKey::from_bytes(&[7u8;32]).unwrap();
    let pk=hex::encode(key.verifying_key().to_bytes());
    let enrolled=n.admin.call("identity.enroll",json!({"public_key":pk,"name":"Laptop key","scopes":["call:system.ping"]}),None,None).await.unwrap();
    let public=n.client(None);
    assert_eq!(code_of(public.call("identity.challenge",json!({"public_key":pk,"audience":"https://evil.example"}),None,None).await).await,"forbidden");
    let c=public.call("identity.challenge",json!({"public_key":pk,"audience":"http://localhost:7710"}),None,None).await.unwrap();
    let sig:k256::schnorr::Signature=key.sign(c["message"].as_str().unwrap().as_bytes());
    let a=json!({"nonce":c["nonce"],"signature":hex::encode(sig.to_bytes())});
    let login=public.call("identity.login",a.clone(),None,None).await.unwrap();
    assert_eq!(code_of(public.call("identity.login",a,None,None).await).await,"unauthorized");
    let session=n.client(login["token"].as_str());
    assert!(session.call("system.ping",json!({}),None,None).await.is_ok());
    assert_eq!(code_of(session.call("core.info",json!({}),None,None).await).await,"forbidden");
    n.admin.call("identity.remove",json!({"id":enrolled["id"]}),None,None).await.unwrap();
    assert_eq!(code_of(session.call("system.ping",json!({}),None,None).await).await,"unauthorized");
    let limited=n.admin.call("tokens.create",json!({"name":"MCP","scopes":["admin:tokens.create","admin:system.ping"]}),None,None).await.unwrap();
    assert_eq!(code_of(n.client(limited["token"].as_str()).call("tokens.create",json!({"name":"escalate","scopes":["admin"]}),None,None).await).await,"forbidden");
    n.close().await;
}

#[tokio::test]
async fn capabilities_publish_source_owned_localizations() {
    let n = TestNode::new("localizations").await;
    let catalog = n.node.kernel.catalog();
    let ping = catalog.iter().find(|c| c["name"] == "system.ping").unwrap();
    assert_eq!(ping["localizations"]["ru"]["description"], "Проверка связи с узлом");
    n.close().await;
}

#[tokio::test]
async fn supervisor_restart_removes_stale_registration() {
    let n = TestNode::new("restart").await;
    n.admin.call("supervisor.add",json!({"id":"restart_test","command":["nonexistent-delta-test-program"]}),None,None).await.unwrap();
    let manifest = json!({"id":"restart_test","version":"1.0.0","capabilities":[{"name":"status"}]});
    n.admin.request("POST", "/v1/components", Some(&manifest), None).await.unwrap();
    n.admin.call("supervisor.restart",json!({"id":"restart_test"}),None,None).await.unwrap();
    // A replacement must not wait for the old component's 30-second heartbeat timeout.
    n.admin.request("POST", "/v1/components", Some(&manifest), None).await.unwrap();
    n.close().await;
}

#[tokio::test]
async fn ready_state_survives_without_any_subscribers() {
    let home = std::env::temp_dir().join(format!("delta-ready-{}", random_hex(4)));
    let cfg = Arc::new(Config::new(home, default_config("late-ready", "127.0.0.1", 0, "")));
    let kernel = delta::kernel::Kernel::new(cfg);
    // The supervisor task may not subscribe until AFTER Node::start publishes readiness.
    kernel.set_ready();
    for _ in 0..3 {
        tokio::time::timeout(Duration::from_millis(100), kernel.wait_ready())
            .await.expect("readiness must remain true for late subscribers");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn late_supervisor_starts_real_process_after_node_is_ready() {
    use delta::components::Supervisor;
    let home = std::env::temp_dir().join(format!("delta-late-supervisor-{}", random_hex(4)));
    let mut data = default_config("late-supervisor", "127.0.0.1", 0, "");
    data.builtin.supervisor = false; // Guarantees no readiness subscribers during Node::start.
    let cfg = Arc::new(Config::new(home.clone(), data)); cfg.save().unwrap();
    let (token, _) = TokenStore::new(cfg.clone()).create("admin", &["admin".into()], true).unwrap();
    let node = Node::start(cfg.clone(), Some(0)).await.unwrap();
    node.kernel.add(Supervisor::new(cfg)).await.unwrap();
    let client = Client::new(&format!("http://127.0.0.1:{}", node.port()), Some(token));
    // The test binary's --list is a portable real child process on Linux and Windows.
    client.call("supervisor.add", json!({"id":"readiness_probe","command":[std::env::current_exe().unwrap(),"--list"],"scopes":["register:readiness_probe"]}), None, None).await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let rows = client.call("supervisor.list", json!({}), None, None).await.unwrap();
            if rows.as_array().unwrap().iter().any(|r| r["id"] == "readiness_probe" && !r["last_exit"].is_null()) { break; }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }).await;
    let logs = client.call("supervisor.logs", json!({"id":"readiness_probe"}), None, None).await.unwrap();
    node.stop().await; let _ = std::fs::remove_dir_all(home);
    assert!(outcome.is_ok(), "late supervisor never started its child process");
    assert!(logs.as_array().unwrap().iter().any(|l| l.as_str().unwrap_or("").contains("health_open_and_anonymous_rejected")), "real child must produce test-list output");
}

#[tokio::test]
async fn adding_disabled_component_does_not_start_it() {
    let n = TestNode::new("disabled-install").await;
    n.admin.call("supervisor.add",json!({"id":"disabled_probe","command":["nonexistent-delta-test-program"],"enabled":false}),None,None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let list=n.admin.call("supervisor.list",json!({}),None,None).await.unwrap();
    let item=list.as_array().unwrap().iter().find(|m|m["id"]=="disabled_probe").unwrap();
    assert_eq!(item["enabled"],false);assert_eq!(item["running"],false);
    assert_eq!(n.node.config.read(|c|c.components.iter().find(|m|m.id=="disabled_probe").unwrap().enabled),false);
    n.close().await;
}

#[tokio::test]
async fn explicit_peer_trust_is_directional_revocable_and_single_use() {
    let a = TestNode::new("trust-a").await;
    let b = TestNode::new("trust-b").await;
    let b_id = b.admin.call("core.info", json!({}), None, None).await.unwrap()["node_id"].as_str().unwrap().to_string();
    let inv = a.admin.call("peers.invite", json!({"trust_admin":true,"allow_local":true}), None, None).await.unwrap();
    b.admin.call("peers.join", json!({"url":a.url,"code":inv["code"],"trust_admin":true,"local_url":b.url}), None, None).await.unwrap();
    assert!(a.admin.call("peers.list",json!({}),None,None).await.unwrap()[0]["trusted_admin"].as_bool().unwrap());
    assert!(b.admin.call("peers.list",json!({}),None,None).await.unwrap()[0]["trusted_admin"].as_bool().unwrap());
    b.admin.call("supervisor.list",json!({}),Some("trust-a"),None).await.unwrap();
    // tokens.list is admin-only, and both directions were explicitly approved.
    b.admin.call("tokens.list",json!({}),Some("trust-a"),None).await.unwrap();
    a.admin.call("tokens.list",json!({}),Some(&b_id),None).await.unwrap();
    a.admin.call("peers.trust",json!({"node":b_id,"admin":false}),None,None).await.unwrap();
    assert_eq!(code_of(b.admin.call("tokens.list",json!({}),Some("trust-a"),None).await).await,"forbidden");
    a.admin.call("tokens.list",json!({}),Some(&b_id),None).await.unwrap();
    assert_eq!(code_of(b.admin.call("peers.join",json!({"url":a.url,"code":inv["code"]}),None,None).await).await,"forbidden");
    let narrow=a.admin.call("tokens.create",json!({"name":"pairing-only","scopes":["admin:peers.invite","admin:peers.join","admin:peers.trust"]}),None,None).await.unwrap();
    let restricted=a.client(narrow["token"].as_str());
    assert_eq!(code_of(restricted.call("peers.invite",json!({"trust_admin":true,"allow_local":true}),None,None).await).await,"forbidden");
    assert_eq!(code_of(restricted.call("peers.trust",json!({"node":b_id,"admin":true}),None,None).await).await,"forbidden");
    a.close().await;b.close().await;
}

#[tokio::test]
async fn remembered_signed_session_survives_disk_reload_and_revocation() {
    use k256::schnorr::{SigningKey,signature::Signer};
    let n=TestNode::new("remembered").await;
    let key=SigningKey::from_bytes(&[9u8;32]).unwrap();
    let pk=hex::encode(key.verifying_key().to_bytes());
    let enrolled=n.admin.call("identity.enroll",json!({"public_key":pk,"name":"Remembered laptop","scopes":["call:system.ping"]}),None,None).await.unwrap();
    let public=n.client(None);
    for remember in [false,true] {
        let c=public.call("identity.challenge",json!({"public_key":pk,"audience":"http://localhost:7710","remember":remember}),None,None).await.unwrap();
        assert_eq!(c["remember"],remember);
        assert_eq!(c["message"].as_str().unwrap().ends_with("\nremember:2592000"),remember);
        let sig:k256::schnorr::Signature=key.sign(c["message"].as_str().unwrap().as_bytes());
        let login=public.call("identity.login",json!({"nonce":c["nonce"],"signature":hex::encode(sig.to_bytes())}),None,None).await.unwrap();
        let expected=if remember{2592000.0}else{28800.0};
        assert!((login["expires_at"].as_f64().unwrap()-delta::util::now()-expected).abs()<5.0);
        let disk=Arc::new(Config::load(n.home.clone()).unwrap());
        let store=TokenStore::new(disk);
        let principal=store.authenticate(login["token"].as_str().unwrap()).expect("session survives a fresh token store loaded from disk");
        assert!(!principal.is_admin());
        n.admin.call("tokens.revoke",json!({"id":login["id"]}),None,None).await.unwrap();
        let store=TokenStore::new(Arc::new(Config::load(n.home.clone()).unwrap()));
        assert!(store.authenticate(login["token"].as_str().unwrap()).is_none());
    }
    // Revoking the key must also revoke persistent remembered sessions.
    let c=public.call("identity.challenge",json!({"public_key":pk,"audience":"http://localhost:7710","remember":true}),None,None).await.unwrap();
    let sig:k256::schnorr::Signature=key.sign(c["message"].as_str().unwrap().as_bytes());
    let login=public.call("identity.login",json!({"nonce":c["nonce"],"signature":hex::encode(sig.to_bytes())}),None,None).await.unwrap();
    n.admin.call("identity.remove",json!({"id":enrolled["id"]}),None,None).await.unwrap();
    assert!(TokenStore::new(Arc::new(Config::load(n.home.clone()).unwrap())).authenticate(login["token"].as_str().unwrap()).is_none());
    n.close().await;
}

#[tokio::test]
async fn forwarding_borrows_only_a_live_invocation_without_elevating_the_caller() {
    let n=TestNode::new("delegation").await;
    let manifest=json!({"id":"relay","version":"1.0.0","capabilities":[{"name":"read"}]});
    let reg=n.admin.request("POST","/v1/components",Some(&manifest),None).await.unwrap();
    let token=n.admin.call("tokens.create",json!({"name":"reader","scopes":["call:relay.read","call:core.info"]}),None,None).await.unwrap();
    let reader=n.client(token["token"].as_str());
    let pending=tokio::spawn(async move{reader.call("relay.read",json!({}),None,None).await});
    let ext=n.node.kernel.get_external("relay",reg["session"].as_str().unwrap()).unwrap();
    let inv=ext.next(Duration::from_secs(2)).await.unwrap();
    let base=json!({"session":reg["session"],"invocation":inv["id"],"capability":"core.info","args":{}});
    let info=n.admin.request("POST","/v1/components/relay/call",Some(&base),None).await.unwrap();
    assert_eq!(info["name"],"delegation");
    let mut forbidden=base.clone();forbidden["capability"]=json!("tokens.create");forbidden["args"]=json!({"name":"backdoor","scopes":["admin"]});forbidden["caller"]=json!({"scopes":["admin"]});
    assert_eq!(code_of(n.admin.request("POST","/v1/components/relay/call",Some(&forbidden),None).await).await,"forbidden");
    let mut wrong=base.clone();wrong["session"]=json!("wrong");assert!(n.admin.request("POST","/v1/components/relay/call",Some(&wrong),None).await.is_err());
    let restricted=n.admin.call("tokens.create",json!({"name":"other-component","scopes":["register:other"]}),None,None).await.unwrap();
    assert_eq!(code_of(n.client(restricted["token"].as_str()).request("POST","/v1/components/relay/call",Some(&base),None).await).await,"forbidden");
    ext.resolve(inv["id"].as_str().unwrap(),json!({"ok":true,"result":true}));assert_eq!(pending.await.unwrap().unwrap(),true);
    assert_eq!(code_of(n.admin.request("POST","/v1/components/relay/call",Some(&base),None).await).await,"not_found");
    n.close().await;
}

#[tokio::test]
async fn optional_dependencies_gate_only_the_declared_capability_and_recover_on_install() {
    let n=TestNode::new("dependency-gates").await;
    let manifest=json!({"id":"consumer","version":"1.0.0","metadata":{"ui":{"id":"dev.example.consumer","dependencies":[{"id":"dev.example.provider","version":"^1.0.0","required":false,"features":["reports"]}]}},"capabilities":[{"name":"base"},{"name":"reports","requires":["reports"]}]});
    let reg=n.admin.request("POST","/v1/components",Some(&manifest),None).await.unwrap();
    let status=n.admin.call("dependencies.status",json!({"id":"dev.example.consumer"}),None,None).await.unwrap();assert_eq!(status["ready"],true);assert_eq!(status["features"]["reports"]["available"],false);
    assert_eq!(code_of(n.admin.call("consumer.reports",json!({}),None,None).await).await,"missing_dependency");
    let admin=n.admin.clone();let pending=tokio::spawn(async move{admin.call("consumer.base",json!({}),None,None).await});let ext=n.node.kernel.get_external("consumer",reg["session"].as_str().unwrap()).unwrap();let inv=ext.next(Duration::from_secs(2)).await.unwrap();ext.resolve(inv["id"].as_str().unwrap(),json!({"ok":true,"result":"base works"}));assert_eq!(pending.await.unwrap().unwrap(),"base works");
    let provider=json!({"id":"provider","version":"1.0.1","metadata":{"ui":{"id":"dev.example.provider"}},"capabilities":[]});n.admin.request("POST","/v1/components",Some(&provider),None).await.unwrap();
    let status=n.admin.call("dependencies.status",json!({"id":"consumer"}),None,None).await.unwrap();assert_eq!(status["features"]["reports"]["available"],true);
    let admin=n.admin.clone();let pending=tokio::spawn(async move{admin.call("consumer.reports",json!({}),None,None).await});let inv=ext.next(Duration::from_secs(2)).await.unwrap();ext.resolve(inv["id"].as_str().unwrap(),json!({"ok":true,"result":true}));assert_eq!(pending.await.unwrap().unwrap(),true);
    n.close().await;
}

#[tokio::test]
async fn replaceable_providers_require_explicit_binding_and_protect_enabled_consumers() {
 let n=TestNode::new("provider-bindings").await;
 for id in ["store_a","store_b"] {let manifest=json!({"id":id,"version":"1.0.0","metadata":{"ui":{"id":format!("dev.example.{id}")},"contracts":{"delta.storage":"1.0.0"}},"capabilities":[]});n.admin.request("POST","/v1/components",Some(&manifest),None).await.unwrap();}
 let consumer=json!({"id":"reader","version":"1.0.0","metadata":{"ui":{"id":"dev.example.reader","dependencies":[{"id":"records","kind":"provider","contract":"delta.storage","version":"^1.0.0","required":true,"features":["history"]}]}},"capabilities":[]});n.admin.request("POST","/v1/components",Some(&consumer),None).await.unwrap();
 let before=n.admin.call("dependencies.status",json!({"id":"reader"}),None,None).await.unwrap();assert_eq!(before["dependencies"][0]["state"],"unbound");assert_eq!(before["ready"],false);
 let bound=n.admin.call("dependencies.bind",json!({"id":"dev.example.reader","dependency":"records","component":"store_b"}),None,None).await.unwrap();assert_eq!(bound["ready"],true);assert_eq!(bound["dependencies"][0]["component"],"store_b");
 assert!(delta::components::dependencies::removal_blockers(&n.node.kernel,"store_a").is_empty());assert_eq!(delta::components::dependencies::removal_blockers(&n.node.kernel,"store_b"),vec!["dev.example.reader"]);
 let again=n.admin.call("dependencies.status",json!({"id":"reader"}),None,None).await.unwrap();assert_eq!(again["dependencies"][0]["component"],"store_b");
 let key=n.admin.call("tokens.create",json!({"name":"not-owner","scopes":["call:dependencies.*"]}),None,None).await.unwrap();assert_eq!(code_of(n.client(key["token"].as_str()).call("dependencies.bind",json!({"id":"reader","dependency":"records","component":"store_a"}),None,None).await).await,"forbidden");
 n.close().await;
}

#[tokio::test]
async fn exec_run_requires_admin() {
    let n = TestNode::with_config("exec", |c| {
        c.builtin.exec = true;
        c.exec.allow = vec!["echo".into()];
    })
    .await;
    let all = n.admin.call("tokens.create", json!({"name": "all", "scopes": ["call:*"]}), None, None).await.unwrap();
    let all = n.client(all["token"].as_str());
    let args = json!({"command": ["echo", "hi"]});
    assert_eq!(code_of(all.call("exec.run", args.clone(), None, None).await).await, "forbidden");
    // read-only listing stays available to call:* tokens
    assert!(all.call("exec.allowed", json!({}), None, None).await.is_ok());
    let r = n.admin.call("exec.run", args, None, None).await.unwrap();
    assert_eq!(r["code"], 0);
    n.close().await;
}

#[tokio::test]
async fn challenge_limit_is_per_key() {
    use k256::schnorr::SigningKey;
    let n = TestNode::new("challenge-limit").await;
    let pk = |seed: u8| hex::encode(SigningKey::from_bytes(&[seed; 32]).unwrap().verifying_key().to_bytes());
    let public = n.client(None);
    let ask = |k: String| {
        let public = public.clone();
        async move { public.call("identity.challenge", json!({"public_key": k, "audience": "http://localhost:7710"}), None, None).await }
    };
    let mut last = "ok".to_string();
    for _ in 0..20 {
        last = code_of(ask(pk(1)).await).await;
    }
    assert_eq!(last, "unavailable", "one key must hit its own limit");
    // a different key is not locked out by the first one
    assert!(ask(pk(2)).await.is_ok());
    n.close().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn supervisor_stop_terminates_the_whole_process_group() {
    let n = TestNode::new("group-kill").await;
    let pidfile = n.home.join("grandchild.pid");
    let script = format!("sleep 300 & echo $! > {}; wait", pidfile.display());
    n.admin.call("supervisor.add", json!({"id": "group_probe", "command": ["sh", "-c", script]}), None, None).await.unwrap();
    let mut pid = None;
    for _ in 0..100 {
        if let Some(p) = std::fs::read_to_string(&pidfile).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
            pid = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let pid = pid.expect("grandchild never started");
    // a zombie awaiting reaping counts as dead
    let alive = |pid: u32| std::fs::read_to_string(format!("/proc/{pid}/stat")).map(|s| !s.rsplit(") ").next().unwrap_or("").starts_with('Z')).unwrap_or(false);
    assert!(alive(pid));
    n.admin.call("supervisor.stop", json!({"id": "group_probe"}), None, None).await.unwrap();
    for _ in 0..100 {
        if !alive(pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(pid), "grandchild survived supervisor.stop");
    n.close().await;
}

#[tokio::test]
async fn exec_enforces_the_allow_list_and_the_timeout() {
    let n = TestNode::with_config("exec-rules", |c| {
        c.builtin.exec = true;
        c.exec.allow = vec!["echo".into(), "sleep".into()];
    })
    .await;
    let run = |cmd: Value, extra: Value| {
        let mut args = json!({"command": cmd});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let admin = n.admin.clone();
        async move { admin.call("exec.run", args, None, None).await }
    };
    let ok = run(json!(["echo", "hello"]), json!({})).await.unwrap();
    assert_eq!(ok["code"], 0);
    assert!(ok["stdout"].as_str().unwrap().contains("hello"));
    assert_eq!(code_of(run(json!(["cat", "/etc/passwd"]), json!({})).await).await, "forbidden");
    assert_eq!(code_of(run(json!([]), json!({})).await).await, "forbidden");
    // a path is only allowed when that exact path is on the list, even if its basename is
    assert_eq!(code_of(run(json!(["/bin/echo", "x"]), json!({})).await).await, "forbidden");
    assert_eq!(code_of(run(json!(["./echo", "x"]), json!({})).await).await, "forbidden");
    assert_eq!(code_of(run(json!(["sleep", "5"]), json!({"timeout": 0.3})).await).await, "timeout");
    n.close().await;
}

#[tokio::test]
async fn offline_peer_does_not_delay_or_reclassify_calls_to_other_peers() {
    let a = TestNode::new("route-a").await;
    let b = TestNode::new("route-b").await;
    let c = TestNode::new("route-c").await;
    for n in [&b, &c] {
        let inv = a.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
        n.admin.call("peers.join", json!({"url": a.url, "code": inv["code"], "local_url": n.url}), None, None).await.unwrap();
    }
    c.close().await;
    let started = std::time::Instant::now();
    a.admin.call("system.ping", json!({}), Some("route-b"), None).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "offline peer slowed a call to another peer");

    let t = a.admin.call("tokens.create", json!({"name": "viewer", "scopes": ["call:system.*"]}), None, None).await.unwrap();
    let viewer = a.client(t["token"].as_str());
    assert_eq!(code_of(viewer.call("system.ping", json!({}), Some("route-c"), None).await).await, "unavailable");
    // authorization stays fail-closed on the target's real admin flag
    assert_eq!(code_of(viewer.call("tokens.list", json!({}), Some("route-b"), None).await).await, "forbidden");
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn port_zero_client_joins_outbound_only_and_last_contact_is_reported() {
    let hub = TestNode::new("ghost-hub").await;
    let client = TestNode::new("ghost-client").await;
    // what a native client has configured before it listens
    client.node.config.update(|c| c.url = "http://127.0.0.1:0".into());
    let inv = hub.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    client.admin.call("peers.join", json!({"url": hub.url, "code": inv["code"]}), None, None).await.unwrap();

    let started = std::time::Instant::now();
    let row = hub.admin.call("peers.list", json!({}), None, None).await.unwrap()[0].clone();
    assert!(started.elapsed() < Duration::from_secs(1), "nothing should be probed for an outbound-only peer");
    assert_eq!(row["url"], "");
    assert_eq!(row["outbound_only"], true);
    assert_eq!(row["online"], false);
    assert_eq!(code_of(hub.admin.call("system.ping", json!({}), Some("ghost-client"), None).await).await, "unavailable");

    // the client reaches the hub through the signed channel, which counts as contact
    client.admin.call("system.ping", json!({}), Some("ghost-hub"), None).await.unwrap();
    let row = hub.admin.call("peers.list", json!({}), None, None).await.unwrap()[0].clone();
    let seen = row["last_seen"].as_f64().expect("last_seen after a signed request");
    assert!((delta::util::now() - seen).abs() < 30.0);

    // an explicitly advertised port-0 address is refused
    let inv = hub.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    let bad = client.admin.call("peers.join", json!({"url": hub.url, "code": inv["code"], "local_url": "http://127.0.0.1:0"}), None, None).await;
    assert_eq!(code_of(bad).await, "bad_request");
    hub.close().await;
    client.close().await;
}

#[tokio::test]
async fn prune_only_forgets_port_zero_registrations_and_only_when_asked() {
    use delta::config::PeerRecord;
    let hub = TestNode::new("prune-hub").await;
    let mk = |id: &str, url: &str| PeerRecord { node_id: id.into(), name: "Delta".into(), url: url.into(), secret: "s".into(), scopes: vec![], added: 0.0 };
    hub.node.config.update(|c| {
        c.peers.push(mk("ghost1", "http://127.0.0.1:0"));
        c.peers.push(mk("phone", "")); // legitimate outbound-only client
        c.peers.push(mk("lan", "http://192.0.2.1:7710"));
    });
    let listed = hub.admin.call("peers.list", json!({}), None, None).await.unwrap();
    let ghost = listed.as_array().unwrap().iter().find(|p| p["node_id"] == "ghost1").unwrap();
    assert_eq!(ghost["ghost"], true);

    let dry = hub.admin.call("peers.prune", json!({}), None, None).await.unwrap();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(hub.node.config.read(|c| c.peers.len()), 3, "dry run must not delete");

    let t = hub.admin.call("tokens.create", json!({"name": "all", "scopes": ["call:*"]}), None, None).await.unwrap();
    assert_eq!(code_of(hub.client(t["token"].as_str()).call("peers.prune", json!({"dry_run": false}), None, None).await).await, "forbidden");

    let done = hub.admin.call("peers.prune", json!({"dry_run": false}), None, None).await.unwrap();
    assert_eq!(done["removed"], json!(["ghost1"]));
    let left: Vec<String> = hub.node.config.read(|c| c.peers.iter().map(|p| p.node_id.clone()).collect());
    assert_eq!(left, vec!["phone", "lan"]);
    hub.close().await;
}

#[tokio::test]
async fn calling_an_offline_peer_reports_unavailable() {
    let a = TestNode::new("off-a").await;
    let b = TestNode::new("off-b").await;
    let inv = a.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    b.admin.call("peers.join", json!({"url": a.url, "code": inv["code"], "local_url": b.url}), None, None).await.unwrap();
    a.admin.call("system.ping", json!({}), Some("off-b"), None).await.unwrap();
    b.close().await;
    assert_eq!(code_of(a.admin.call("system.ping", json!({}), Some("off-b"), None).await).await, "unavailable");
    let listed = a.admin.call("peers.list", json!({}), None, None).await.unwrap();
    assert_eq!(listed[0]["online"], false);
    assert_eq!(code_of(a.admin.call("system.ping", json!({}), Some("nobody"), None).await).await, "not_found");
    a.close().await;
}

#[tokio::test]
async fn peers_remove_notifies_the_other_side_and_leave_is_peer_only() {
    let a = TestNode::new("rm-a").await;
    let b = TestNode::new("rm-b").await;
    let inv = a.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    b.admin.call("peers.join", json!({"url": a.url, "code": inv["code"], "local_url": b.url}), None, None).await.unwrap();
    // an owner token is not a peer: it cannot make a node leave
    assert_eq!(code_of(a.admin.call("peers.leave", json!({}), None, None).await).await, "forbidden");
    assert_eq!(code_of(a.admin.call("peers.remove", json!({"node": "nobody"}), None, None).await).await, "not_found");

    a.admin.call("peers.remove", json!({"node": "rm-b"}), None, None).await.unwrap();
    assert!(a.admin.call("peers.list", json!({}), None, None).await.unwrap().as_array().unwrap().is_empty());
    assert!(b.admin.call("peers.list", json!({}), None, None).await.unwrap().as_array().unwrap().is_empty(), "the other side was told to forget us");
    // the removed peer can no longer reach us
    assert_ne!(code_of(b.admin.call("system.ping", json!({}), Some("rm-a"), None).await).await, "ok");

    // removal still works locally when the other side cannot be told
    let inv = a.admin.call("peers.invite", json!({"allow_local": true}), None, None).await.unwrap();
    b.admin.call("peers.join", json!({"url": a.url, "code": inv["code"], "local_url": b.url}), None, None).await.unwrap();
    b.close().await;
    a.admin.call("peers.remove", json!({"node": "rm-b"}), None, None).await.unwrap();
    assert!(a.admin.call("peers.list", json!({}), None, None).await.unwrap().as_array().unwrap().is_empty());
    a.close().await;
}
