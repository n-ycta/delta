use crate::config::Config;
use crate::errors::{DeltaError, Result};
use crate::kernel::{cap, CallContext, CapSpec, Component};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

const MAX_OUT: usize = 64 * 1024;

/// Runs allow-listed executables (no shell). Disabled unless enabled in config.
pub struct Exec {
    config: Arc<Config>,
}

impl Exec {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

fn tail(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[b.len().saturating_sub(MAX_OUT)..]).into_owned()
}

#[async_trait]
impl Component for Exec {
    fn id(&self) -> &str {
        "exec"
    }
    fn version(&self) -> String {
        "0.1.0".into()
    }
    fn description(&self) -> String {
        "Run allow-listed commands on this host".into()
    }

    fn capabilities(&self) -> Vec<CapSpec> {
        vec![
            cap("exec.allowed", "Executables that may be run", json!({}), false, false),
            cap(
                "exec.run",
                "Run an allow-listed command without a shell",
                json!({"command": {"type": "array", "required": true}, "timeout": {"type": "number"}, "input": {"type": "string"}}),
                false,
                false,
            ),
        ]
    }

    async fn handle(&self, capability: &str, args: Value, _ctx: &CallContext) -> Result<Value> {
        match capability {
            "exec.allowed" => Ok(json!(self.config.read(|c| c.exec.allow.clone()))),
            "exec.run" => {
                let cmd: Vec<String> = args["command"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect())
                    .unwrap_or_default();
                let allow = self.config.read(|c| c.exec.allow.clone());
                let first = cmd.first().cloned().unwrap_or_default();
                let base = Path::new(&first).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
                let allowed = !cmd.is_empty()
                    && (allow.contains(&first) || allow.contains(&base))
                    && (!first.contains('/') || allow.contains(&first));
                if !allowed {
                    return Err(DeltaError::forbidden(format!("{first:?} is not allow-listed")));
                }
                let secs = args
                    .get("timeout")
                    .and_then(Value::as_f64)
                    .filter(|t| *t > 0.0)
                    .unwrap_or(30.0)
                    .min(300.0);
                let input = args.get("input").and_then(Value::as_str).map(String::from);

                let mut c = Command::new(&first);
                c.args(&cmd[1..])
                    .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true);
                let mut child = c.spawn().map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        DeltaError::not_found(format!("{first} not found"))
                    } else {
                        DeltaError::handler(e.to_string())
                    }
                })?;
                if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
                    tokio::spawn(async move {
                        let _ = stdin.write_all(text.as_bytes()).await;
                        let _ = stdin.shutdown().await;
                    });
                }
                match tokio::time::timeout(Duration::from_secs_f64(secs), child.wait_with_output()).await {
                    Ok(Ok(out)) => Ok(json!({
                        "code": out.status.code().unwrap_or(-1),
                        "stdout": tail(&out.stdout),
                        "stderr": tail(&out.stderr),
                    })),
                    Ok(Err(e)) => Err(DeltaError::handler(e.to_string())),
                    Err(_) => Err(DeltaError::timeout(format!("{first} exceeded {secs}s"))),
                }
            }
            _ => Err(DeltaError::not_found(format!("unknown capability {capability}"))),
        }
    }
}
