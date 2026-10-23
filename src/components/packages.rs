//! Official release installer. Only an owner (or an explicitly trusted peer) may execute packages.
use crate::{
    auth::Principal,
    errors::{DeltaError, Result},
    kernel::{cap, CallContext, CapSpec, Component, Kernel},
    util::random_hex,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
pub struct Packages {
    jobs: Arc<Mutex<BTreeMap<String, Value>>>,
}
impl Packages {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}
const INDEX: &str = "https://raw.githubusercontent.com/n-ycta/delta/main/registry/plugins.json";
async fn download(url: &str, limit: usize) -> Result<Vec<u8>> {
    let u = reqwest::Url::parse(url).map_err(|_| DeltaError::bad_request("Invalid release URL"))?;
    if u.scheme() != "https"
        || !matches!(
            u.host_str(),
            Some("github.com" | "raw.githubusercontent.com")
        )
    {
        return Err(DeltaError::forbidden(
            "Only official HTTPS GitHub releases are supported",
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .user_agent("Delta/0.3")
        .build()
        .map_err(|e| DeltaError::unavailable(e.to_string()))?;
    let mut r = client
        .get(url)
        .send()
        .await
        .map_err(|e| DeltaError::unavailable(e.to_string()))?
        .error_for_status()
        .map_err(|e| DeltaError::unavailable(e.to_string()))?;
    let mut bytes = Vec::new();
    while let Some(c) = r
        .chunk()
        .await
        .map_err(|e| DeltaError::unavailable(e.to_string()))?
    {
        if bytes.len() + c.len() > limit {
            return Err(DeltaError::bad_request("Release exceeds download limit"));
        }
        bytes.extend_from_slice(&c)
    }
    Ok(bytes)
}
// Diagnostic failure must never prevent rollback. Never put process output in API errors.
fn save_diagnostic(home: &std::path::Path, id: &str, value: &Value) -> Option<std::path::PathBuf> {
    let dir = home.join("data/package-diagnostics");
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    let path = dir.join(format!("{id}-{}.json", random_hex(8)));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options
        .open(&path)
        .ok()?
        .write_all(&serde_json::to_vec_pretty(value).ok()?)
        .ok()?;
    Some(path)
}
fn checked_item(item: &Value, args: &Value) -> Result<()> {
    if args
        .get("expectedVersion")
        .and_then(Value::as_str)
        .map(|v| item["version"] != v)
        .unwrap_or(false)
        || args
            .get("expectedSha256")
            .and_then(Value::as_str)
            .map(|v| item["sha256"] != v)
            .unwrap_or(false)
    {
        return Err(DeltaError::conflict(
            "The official catalog changed; check for updates again",
        ));
    }
    Ok(())
}
async fn catalog() -> Result<Value> {
    serde_json::from_slice(&download(INDEX, 2_000_000).await?)
        .map_err(|_| DeltaError::bad_request("Invalid plugin index"))
}
async fn install(k: Arc<Kernel>, p: Principal, item: Value) -> Result<Value> {
    let id = item["component"].as_str().unwrap_or("");
    if !crate::util::valid_id(id) {
        return Err(DeltaError::bad_request("Invalid package component"));
    }
    let url = item["asset"].as_str().unwrap_or("");
    if !url.starts_with("https://github.com/n-ycta/") || !url.contains("/releases/download/") {
        return Err(DeltaError::forbidden("Untrusted package origin"));
    }
    let bytes = download(url, 50_000_000).await?;
    if hex::encode(Sha256::digest(&bytes)) != item["sha256"].as_str().unwrap_or("") {
        return Err(DeltaError::forbidden("Package checksum mismatch"));
    }
    let version = item["version"].as_str().unwrap_or("");
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
        || version.is_empty()
    {
        return Err(DeltaError::bad_request("Invalid version"));
    }
    let root = k.config.home.join("plugins");
    std::fs::create_dir_all(&root).map_err(|e| DeltaError::unavailable(e.to_string()))?;
    let stage = root.join(format!(".stage-{}", random_hex(8)));
    std::fs::create_dir(&stage).map_err(|e| DeltaError::unavailable(e.to_string()))?;
    let dest = root.join(format!("{id}-{version}-{}", random_hex(4)));
    let s = stage.clone();
    let unpack = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
        let mut total = 0u64;
        let mut entries = 0;
        for e in archive.entries()? {
            entries += 1;
            if entries > 50000 {
                return Err(std::io::Error::other("Too many archive entries"));
            }
            let mut e = e?;
            let t = e.header().entry_type();
            if !t.is_file() && !t.is_dir() {
                return Err(std::io::Error::other(
                    "Links and special entries are not allowed",
                ));
            }
            total = total
                .checked_add(e.size())
                .ok_or_else(|| std::io::Error::other("Invalid archive size"))?;
            if total > 150_000_000 {
                return Err(std::io::Error::other("Unpacked package too large"));
            }
            if !e.unpack_in(&s)? {
                return Err(std::io::Error::other("Unsafe package path"));
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| DeltaError::unavailable(e.to_string()))?;
    if let Err(e) = unpack {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(DeltaError::bad_request(e.to_string()));
    }
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(stage.join("manifest.json"))
            .map_err(|e| DeltaError::bad_request(e.to_string()))?,
    )
    .map_err(|e| DeltaError::bad_request(e.to_string()))?;
    if manifest["id"] != id
        || manifest["version"] != version
        || manifest["metadata"]["ui"]["id"] != item["id"]
    {
        return Err(DeltaError::forbidden(
            "Manifest does not match release index",
        ));
    }
    if manifest["metadata"]["ui"].get("dependencies") != item["manifest"].get("dependencies") {
        return Err(DeltaError::forbidden(
            "Release dependency contract does not match the catalog",
        ));
    }
    crate::components::dependencies::enforce(&k, &manifest["metadata"]["ui"], &[])?;
    let node = std::env::var("DELTA_NODE_BINARY").unwrap_or_else(|_| {
        let p = k.config.home.join("runtime").join(if cfg!(windows) {
            "node.exe"
        } else {
            "bin/node"
        });
        if p.exists() {
            p.to_string_lossy().into_owned()
        } else {
            "node".into()
        }
    });
    let npm_cli = k.config.home.join("runtime").join(if cfg!(windows) {
        "node_modules/npm/bin/npm-cli.js"
    } else {
        "lib/node_modules/npm/bin/npm-cli.js"
    });
    let mut command = if npm_cli.exists() {
        let mut c = tokio::process::Command::new(&node);
        c.arg(npm_cli);
        c
    } else {
        let npm = std::env::var("DELTA_NPM_BINARY").unwrap_or_else(|_| {
            if cfg!(windows) {
                "npm.cmd".into()
            } else {
                "npm".into()
            }
        });
        #[cfg(windows)]
        let c = {
            let mut c = tokio::process::Command::new("cmd.exe");
            c.args(["/D", "/C", &npm]);
            c
        };
        #[cfg(not(windows))]
        let c = tokio::process::Command::new(&npm);
        c
    };
    let status = tokio::time::timeout(
        Duration::from_secs(300),
        command
            .args([
                "ci",
                "--omit=dev",
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
            ])
            .current_dir(&stage)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .map_err(|_| DeltaError::unavailable("Dependency install timed out"))?
    .map_err(|e| DeltaError::unavailable(format!("Node/npm runtime missing: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(stage);
        return Err(DeltaError::unavailable("Dependency installation failed"));
    }
    std::fs::rename(&stage, &dest).map_err(|e| DeltaError::unavailable(e.to_string()))?;
    let node = std::env::var("DELTA_NODE_BINARY").unwrap_or_else(|_| {
        let p = k.config.home.join("runtime").join(if cfg!(windows) {
            "node.exe"
        } else {
            "bin/node"
        });
        if p.exists() {
            p.to_string_lossy().into_owned()
        } else {
            "node".into()
        }
    });
    let launch = dest.join("launch.mjs");
    let cwd = serde_json::to_string(&dest.to_string_lossy()).unwrap();
    std::fs::write(
        &launch,
        format!("process.chdir({cwd});await import('./server/main.mjs');"),
    )
    .map_err(|e| DeltaError::unavailable(e.to_string()))?;
    let old = k
        .config
        .read(|c| c.components.iter().find(|m| m.id == id).cloned());
    if old.is_some() {
        k.call("supervisor.remove", json!({"id":id,"force":true}), &p, None)
            .await?;
    }
    let data = k.config.home.join("data").join(id);
    let mut env = json!({"DATA_DIR":data,"DELTA_DATA":data});
    if let Some(o) = &old {
        env = serde_json::to_value(&o.env).unwrap();
        env["DATA_DIR"] = json!(data);
        env["DELTA_DATA"] = json!(data);
    }
    let scopes = item["scopes"]
        .as_array()
        .ok_or_else(|| DeltaError::bad_request("Missing package scopes"))?;
    let result=k.call("supervisor.add",json!({"id":id,"command":[node,launch],"env":env,"enabled":old.as_ref().map(|o|o.enabled).unwrap_or(true),"scopes":scopes,"token_name":old.as_ref().and_then(|o|o.token_name.clone()).map(Value::String).unwrap_or_else(||item["id"].clone())}),&p,None).await;
    if let Err(e) = result {
        if let Some(o) = old {
            let _ = k
                .call("supervisor.add", serde_json::to_value(o).unwrap(), &p, None)
                .await;
        }
        return Err(e);
    }
    if old.as_ref().map(|o| !o.enabled).unwrap_or(false) {
        return Ok(json!({"installed":item["id"],"version":version,"enabled":false}));
    }
    for _ in 0..60 {
        if k.component_list()
            .iter()
            .any(|c| c["id"] == id && c["version"] == version)
        {
            return Ok(json!({"installed":item["id"],"version":version}));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // Capture diagnostics BEFORE removing the managed process (which drops its log).
    // Keep potentially sensitive process output out of API errors and job responses.
    let state = k
        .call("supervisor.list", json!({}), &p, None)
        .await
        .unwrap_or(json!([]));
    let state = state
        .as_array()
        .and_then(|items| items.iter().find(|m| m["id"] == id))
        .cloned()
        .unwrap_or(json!({}));
    let logs = k
        .call("supervisor.logs", json!({"id":id,"lines":80}), &p, None)
        .await
        .unwrap_or(json!([]));
    let diagnostic = save_diagnostic(&k.config.home, id, &json!({"process":state,"logs":logs}));
    let _ = k
        .call("supervisor.remove", json!({"id":id,"force":true}), &p, None)
        .await;
    let restoring = old.is_some();
    if let Some(o) = old {
        let _ = k
            .call("supervisor.add", serde_json::to_value(o).unwrap(), &p, None)
            .await;
    }
    Err(DeltaError::unavailable(format!(
        "Plugin {id} did not register within 60 seconds; {}. {}",
        if restoring {
            "previous configuration restored"
        } else {
            "failed component removed"
        },
        if let Some(path) = diagnostic {
            format!("Private diagnostic: {}", path.display())
        } else {
            "Could not save private diagnostic".into()
        }
    )))
}
#[async_trait]
impl Component for Packages {
    fn id(&self) -> &str {
        "packages"
    }
    fn version(&self) -> String {
        "0.3.0".into()
    }
    fn capabilities(&self) -> Vec<CapSpec> {
        vec![
            cap(
                "packages.catalog",
                "Browse official GitHub release packages",
                json!({}),
                false,
                false,
            ),
            cap(
                "packages.install",
                "Install or update a verified official package",
                json!({"id":{"type":"string","required":true},"expectedVersion":{"type":"string"},"expectedSha256":{"type":"string"},"optionalDependencies":{"type":"array"},"approveDependencies":{"type":"boolean"}}),
                true,
                false,
            ),
            cap(
                "packages.plan",
                "Resolve package dependencies before installation",
                json!({"id":{"type":"string","required":true},"optionalDependencies":{"type":"array"}}),
                false,
                false,
            ),
            cap(
                "packages.jobs",
                "Read installation job status",
                json!({}),
                true,
                false,
            ),
        ]
    }
    async fn handle(&self, c: &str, a: Value, ctx: &CallContext) -> Result<Value> {
        match c {
            "packages.catalog" => catalog().await,
            "packages.plan" => {
                let index = catalog().await?;
                let selected: Vec<String> = serde_json::from_value(
                    a.get("optionalDependencies").cloned().unwrap_or(json!([])),
                )
                .map_err(|_| DeltaError::bad_request("Invalid dependency selection"))?;
                crate::components::dependencies::plan_for_kernel(
                    &ctx.kernel,
                    &index,
                    a["id"].as_str().unwrap_or(""),
                    &selected,
                )
            }
            "packages.jobs" => Ok(json!(self
                .jobs
                .lock()
                .unwrap()
                .values()
                .cloned()
                .collect::<Vec<_>>())),
            "packages.install" => {
                if !ctx.principal.is_admin() {
                    return Err(DeltaError::forbidden(
                        "Installing executable packages requires global admin",
                    ));
                }
                let index = catalog().await?;
                let item = index["plugins"]
                    .as_array()
                    .and_then(|items| items.iter().find(|v| v["id"] == a["id"]))
                    .cloned()
                    .ok_or_else(|| DeltaError::not_found("Package not in official index"))?;
                checked_item(&item, &a)?;
                let selected: Vec<String> = serde_json::from_value(
                    a.get("optionalDependencies").cloned().unwrap_or(json!([])),
                )
                .map_err(|_| DeltaError::bad_request("Invalid dependency selection"))?;
                let plan = crate::components::dependencies::plan_for_kernel(
                    &ctx.kernel,
                    &index,
                    a["id"].as_str().unwrap_or(""),
                    &selected,
                )?;
                let items = plan["items"].as_array().unwrap().clone();
                if items.len() > 1 && a["approveDependencies"] != true {
                    return Err(DeltaError::conflict(
                        "Dependency installation requires explicit approval; inspect packages.plan",
                    ));
                }

                let id = random_hex(12);
                {
                    let mut jobs = self.jobs.lock().unwrap();
                    if jobs.values().any(|j| j["status"] == "installing") {
                        return Err(DeltaError::conflict("Another installation is running"));
                    }
                    if jobs.len() > 100 {
                        jobs.clear()
                    }
                    jobs.insert(id.clone(),json!({"id":id,"pluginId":a["id"],"status":"installing","startedAt":crate::util::now()}));
                }
                let jobs = self.jobs.clone();
                let k = ctx.kernel.clone();
                let p = ctx.principal.clone();
                let jid = id.clone();
                tokio::spawn(async move {
                    let r = async {
                        let mut completed = vec![];
                        for release in items {
                            let result = install(k.clone(), p.clone(), release).await?;
                            completed.push(result);
                        }
                        Ok::<Value, DeltaError>(
                            json!({"installed":completed,"warnings":plan["warnings"]}),
                        )
                    }
                    .await;
                    let mut jobs = jobs.lock().unwrap();
                    let job = jobs.get_mut(&jid).unwrap();
                    match r {
                        Ok(v) => {
                            job["status"] = json!("complete");
                            job["result"] = v
                        }
                        Err(e) => {
                            job["status"] = json!("failed");
                            job["error"] = json!({"code":e.code,"message":e.message})
                        }
                    }
                });
                Ok(json!({"jobId":id,"status":"installing"}))
            }
            _ => Err(DeltaError::not_found("Unknown package operation")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_release_guards_prevent_catalog_races() {
        let item = json!({"version":"1.2.3","sha256":"abc"});
        assert!(checked_item(&item, &json!({})).is_ok());
        assert!(checked_item(
            &item,
            &json!({"expectedVersion":"1.2.3","expectedSha256":"abc"})
        )
        .is_ok());
        assert!(checked_item(&item, &json!({"expectedVersion":"1.2.4"})).is_err());
        assert!(checked_item(&item, &json!({"expectedSha256":"different"})).is_err());
    }
    #[test]
    fn diagnostics_are_private_and_best_effort() {
        let home = std::env::temp_dir().join(format!("delta-diagnostic-{}", random_hex(8)));
        let path =
            save_diagnostic(&home, "storage", &json!({"logs":["private test output"]})).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("private test output"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(&home).unwrap();
        std::fs::write(&home, "not a directory").unwrap();
        assert!(save_diagnostic(&home, "storage", &json!({})).is_none());
        std::fs::remove_file(home).unwrap();
    }
}
