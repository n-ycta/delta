//! Supervisor: starts managed external components, restarts them, keeps their logs.
//!
//! Each managed process gets DELTA_URL and an in-memory token scoped to `register:<id>`
//! Default legacy scopes include `call:*`; admins can configure explicit scopes and token_name.
//! The injected credential is ephemeral and revoked on process replacement/stop.
use crate::config::{Config, ManagedSpec};
use crate::errors::{DeltaError, Result};
use crate::kernel::{cap, CallContext, CapSpec, Component, Kernel};
use crate::util::valid_id;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
    sync::watch,
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

type Logs = Arc<Mutex<VecDeque<String>>>;
type Items = Arc<Mutex<BTreeMap<String, Managed>>>;

struct Managed {
    spec: ManagedSpec,
    child: Option<Child>,
    logs: Logs,
    restarts: u32,
    started_at: Option<Instant>,
    wanted: bool,
    token_id: Option<String>,
    last_exit: Option<Value>,
    next_try: Option<Instant>,
}

impl Managed {
    fn new(spec: ManagedSpec) -> Self {
        let wanted = spec.enabled;
        Self {
            spec,
            child: None,
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(300))),
            restarts: 0,
            started_at: None,
            wanted,
            token_id: None,
            last_exit: None,
            next_try: None,
        }
    }

    fn running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn status(&mut self) -> Value {
        let running = self.running();
        let pid = if running { self.child.as_ref().and_then(|c| c.id()) } else { None };
        let uptime: Option<f64> = if running {
            self.started_at.map(|s| (s.elapsed().as_secs_f64() * 10.0).round() / 10.0)
        } else {
            None
        };
        json!({
            "id": self.spec.id, "command": self.spec.command, "enabled": self.wanted,
            "running": running, "pid": pid, "restarts": self.restarts, "last_exit": self.last_exit,
            "uptime": uptime,
        })
    }
}

async fn pump<R: AsyncRead + Unpin + Send + 'static>(r: R, logs: Logs) {
    let mut lines = BufReader::new(r).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut g = logs.lock().unwrap();
        if g.len() >= 300 {
            g.pop_front();
        }
        g.push_back(line);
    }
}

fn spawn_process(m: &mut Managed, kernel: &Arc<Kernel>, config: &Arc<Config>) {
    let id = m.spec.id.clone();
    let Some((program, rest)) = m.spec.command.split_first() else {
        m.last_exit = Some(json!("spawn failed: empty command"));
        m.next_try = Some(Instant::now() + Duration::from_secs(5));
        return;
    };
    let scopes = m.spec.scopes.clone().unwrap_or_else(|| vec![format!("register:{id}"), "call:*".to_string()]);
    let token_name = m.spec.token_name.clone().unwrap_or_else(|| format!("component:{id}"));
    let (token, rec) = match kernel.tokens.create(&token_name, &scopes, false) {
        Ok(v) => v,
        Err(e) => {
            m.last_exit = Some(json!(format!("spawn failed: {e}")));
            m.next_try = Some(Instant::now() + Duration::from_secs(5));
            return;
        }
    };
    if let Some(old) = m.token_id.take() {
        kernel.tokens.revoke(&old);
    }
    m.token_id = Some(rec.id);

    let data_dir = config.home.join("data").join(&id);
    let _ = std::fs::create_dir_all(&data_dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700));
    }

    let mut cmd = Command::new(program);
    cmd.args(rest);
    for (k, v) in &m.spec.env {
        cmd.env(k, v.as_str().map(String::from).unwrap_or_else(|| v.to_string()));
    }
    cmd.env("DELTA_HOME", &config.home)
        .env("DELTA_URL", kernel.local_url())
        .env("DELTA_TOKEN", token)
        .env("DELTA_COMPONENT_ID", &id)
        .env("DELTA_NODE", &kernel.name)
        .env("DELTA_NODE_ID", &kernel.node_id)
        .env("DELTA_DATA", &data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // new session, like Python's start_new_session=True
    // SAFETY: setsid is async-signal-safe and only touches the child between fork and exec
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match cmd.spawn() {
        Ok(mut child) => {
            m.started_at = Some(Instant::now());
            if let Some(out) = child.stdout.take() {
                tokio::spawn(pump(out, m.logs.clone()));
            }
            if let Some(err) = child.stderr.take() {
                tokio::spawn(pump(err, m.logs.clone()));
            }
            m.child = Some(child);
        }
        Err(e) => {
            let msg = format!("spawn failed: {e}");
            m.logs.lock().unwrap().push_back(msg.clone());
            m.last_exit = Some(json!(msg));
            m.next_try = Some(Instant::now() + Duration::from_secs(5));
        }
    }
}

fn tick(m: &mut Managed, kernel: &Arc<Kernel>, config: &Arc<Config>) {
    let exited = match m.child.as_mut() {
        Some(c) => c.try_wait().ok().flatten(),
        None => None,
    };
    let running = m.child.is_some() && exited.is_none();
    if let Some(st) = exited {
        if m.last_exit.is_none() {
            let code = st.code().unwrap_or_else(|| {
                #[cfg(unix)] { st.signal().map(|s| -s).unwrap_or(-1) }
                #[cfg(not(unix))] { -1 }
            });
            m.last_exit = Some(json!(code));
        }
    }
    let due = m.next_try.map(|t| Instant::now() >= t).unwrap_or(true);
    if m.wanted && !running && due {
        if m.child.is_some() {
            m.restarts += 1;
            let lived = m.started_at.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
            let delay = if lived > 30.0 { 1.0 } else { f64::from(2u32.pow(m.restarts.min(6))).min(60.0) };
            m.next_try = Some(Instant::now() + Duration::from_secs_f64(delay));
            m.child = None;
            return;
        }
        m.last_exit = None;
        m.next_try = None;
        spawn_process(m, kernel, config);
    }
}

async fn run_loop(items: Items, kernel: Arc<Kernel>, config: Arc<Config>, mut stop: watch::Receiver<bool>) {
    kernel.wait_ready().await;
    loop {
        {
            let mut g = items.lock().unwrap();
            for m in g.values_mut() {
                tick(m, &kernel, &config);
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            _ = stop.changed() => break,
        }
    }
}

pub struct Supervisor {
    config: Arc<Config>,
    items: Items,
    kernel: Mutex<Option<Arc<Kernel>>>,
    stop_tx: watch::Sender<bool>,
}

impl Supervisor {
    pub fn new(config: Arc<Config>) -> Arc<Self> {
        let (stop_tx, _) = watch::channel(false);
        Arc::new(Self { config, items: Arc::new(Mutex::new(BTreeMap::new())), kernel: Mutex::new(None), stop_tx })
    }

    fn kernel(&self) -> Result<Arc<Kernel>> {
        self.kernel.lock().unwrap().clone().ok_or_else(|| DeltaError::unavailable("supervisor is not running"))
    }

    fn persist(&self) {
        let specs: Vec<ManagedSpec> = {
            let g = self.items.lock().unwrap();
            g.values().map(|m| ManagedSpec { enabled: m.wanted, ..m.spec.clone() }).collect()
        };
        self.config.update(move |c| c.components = specs);
    }

    fn with_item<R>(&self, id: &str, f: impl FnOnce(&mut Managed) -> R) -> Result<R> {
        let mut g = self.items.lock().unwrap();
        match g.get_mut(id) {
            Some(m) => Ok(f(m)),
            None => Err(DeltaError::not_found(format!("no managed component {id}"))),
        }
    }

    /// SIGTERM, wait up to 5s, then SIGKILL. The child stays in `items` while it shuts down
    /// so the loop cannot start a second copy in the meantime.
    async fn terminate(&self, id: &str) {
        let pid: Option<u32> = {
            let mut g = self.items.lock().unwrap();
            let r = g.get_mut(id).and_then(|m| if m.running() { m.child.as_ref().and_then(|c| c.id()) } else { None });
            r
        };
        if let Some(pid) = pid {
            // SAFETY: plain signal delivery to a child we spawned
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            #[cfg(windows)] {
                if let Some(child) = self.items.lock().unwrap().get_mut(id).and_then(|m| m.child.as_mut()) { let _ = child.start_kill(); }
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let done: bool = {
                    let mut g = self.items.lock().unwrap();
                    let r = g.get_mut(id).map(|m| !m.running()).unwrap_or(true);
                    r
                };
                if done {
                    break;
                }
                if Instant::now() >= deadline {
                    let mut g = self.items.lock().unwrap();
                    if let Some(c) = g.get_mut(id).and_then(|m| m.child.as_mut()) {
                        let _ = c.start_kill();
                    }
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        let token = self.items.lock().unwrap().get_mut(id).and_then(|m| m.token_id.take());
        if let (Some(t), Ok(k)) = (token, self.kernel()) {
            k.tokens.revoke(&t);
        }
    }
}

#[async_trait]
impl Component for Supervisor {
    fn id(&self) -> &str {
        "supervisor"
    }
    fn version(&self) -> String {
        "0.1.0".into()
    }
    fn description(&self) -> String {
        "Lifecycle of managed external components".into()
    }

    fn capabilities(&self) -> Vec<CapSpec> {
        let id = json!({"id": {"type": "string", "required": true}});
        vec![
            cap("supervisor.list", "Managed components and their process state", json!({}), false, false),
            cap(
                "supervisor.add",
                "Add and start a managed component",
                json!({"id": {"type": "string", "required": true}, "command": {"type": "array", "required": true}, "env": {"type": "object"}, "scopes": {"type": "array"}, "token_name": {"type": "string"}}),
                true,
                false,
            ),
            cap("supervisor.remove", "Stop and forget a managed component", id.clone(), true, false),
            cap("supervisor.start", "Enable and start", id.clone(), true, false),
            cap("supervisor.stop", "Disable and stop", id.clone(), true, false),
            cap("supervisor.restart", "Restart the process", id, true, false),
            cap(
                "supervisor.logs",
                "Last output lines of the process",
                json!({"id": {"type": "string", "required": true}, "lines": {"type": "integer"}}),
                true,
                false,
            ),
        ]
    }

    fn start(&self, kernel: &Arc<Kernel>) {
        *self.kernel.lock().unwrap() = Some(kernel.clone());
        {
            let mut g = self.items.lock().unwrap();
            for spec in self.config.read(|c| c.components.clone()) {
                g.insert(spec.id.clone(), Managed::new(spec));
            }
        }
        tokio::spawn(run_loop(self.items.clone(), kernel.clone(), self.config.clone(), self.stop_tx.subscribe()));
    }

    async fn stop(&self) {
        let _ = self.stop_tx.send(true);
        let ids: Vec<String> = {
            let mut g = self.items.lock().unwrap();
            g.values_mut().for_each(|m| m.wanted = false);
            g.keys().cloned().collect()
        };
        for id in ids {
            self.terminate(&id).await;
        }
    }

    async fn handle(&self, capability: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        let id = args.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        match capability {
            "supervisor.list" => {
                let mut g = self.items.lock().unwrap();
                let out: Vec<Value> = g.values_mut().map(|m| m.status()).collect();
                Ok(Value::Array(out))
            }
            "supervisor.add" => {
                if !ctx.principal.is_admin() { return Err(DeltaError::forbidden("adding executable components requires global admin")); }
                let command: Vec<String> = args["command"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect())
                    .unwrap_or_default();
                if !valid_id(&id) || command.is_empty() {
                    return Err(DeltaError::bad_request("invalid id or empty command"));
                }
                let scopes = args.get("scopes").and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect::<Vec<_>>());
                if scopes.as_ref().map(|ss| ss.iter().any(|s| !crate::auth::valid_scope(s))).unwrap_or(false) {
                    return Err(DeltaError::bad_request("invalid managed token scope"));
                }
                let token_name = args.get("token_name").and_then(Value::as_str).map(String::from);
                if token_name.as_ref().map(|n| n.is_empty() || n.len() > 128).unwrap_or(false) { return Err(DeltaError::bad_request("invalid token_name")); }
                let env = args.get("env").and_then(Value::as_object).cloned().unwrap_or_default();
                {
                    let mut g = self.items.lock().unwrap();
                    if g.contains_key(&id) {
                        return Err(DeltaError::conflict(format!("{id} already managed")));
                    }
                    g.insert(id.clone(), Managed::new(ManagedSpec { id: id.clone(), command, env, enabled: args["enabled"].as_bool().unwrap_or(true), scopes, token_name }));
                }
                self.persist();
                Ok(json!({"added": id}))
            }
            "supervisor.remove" => {
                let blockers=crate::components::dependencies::removal_blockers(&ctx.kernel,&id);
                if !blockers.is_empty() && args["force"]!=true {return Err(DeltaError::conflict(format!("Required by {}; explicitly replace the provider or confirm force",blockers.join(", "))))}
                if args["force"]==true && !ctx.principal.is_admin(){return Err(DeltaError::forbidden("Force requires owner access"))}
                self.with_item(&id, |m| m.wanted = false)?;
                self.terminate(&id).await;
                self.items.lock().unwrap().remove(&id);
                self.persist();
                self.kernel()?.remove(&id).await;
                Ok(json!({"removed": id}))
            }
            "supervisor.start" => {
                self.with_item(&id, |m| m.wanted = true)?;
                self.persist();
                self.with_item(&id, |m| m.status())
            }
            "supervisor.stop" => {
                let blockers=crate::components::dependencies::removal_blockers(&ctx.kernel,&id);
                if !blockers.is_empty() && args["force"]!=true {return Err(DeltaError::conflict(format!("Required by {}; explicitly replace the provider or confirm force",blockers.join(", "))))}
                if args["force"]==true && !ctx.principal.is_admin(){return Err(DeltaError::forbidden("Force requires owner access"))}
                self.with_item(&id, |m| m.wanted = false)?;
                self.terminate(&id).await;
                self.kernel()?.remove(&id).await;
                self.persist();
                self.with_item(&id, |m| m.status())
            }
            "supervisor.restart" => {
                self.with_item(&id, |m| m.wanted = true)?;
                self.terminate(&id).await;
                self.kernel()?.remove(&id).await;
                Ok(json!({"restarting": id}))
            }
            "supervisor.logs" => {
                let n = args.get("lines").and_then(Value::as_i64).unwrap_or(100).clamp(1, 300) as usize;
                let logs = self.with_item(&id, |m| m.logs.clone())?;
                let g = logs.lock().unwrap();
                let skip = g.len().saturating_sub(n);
                let lines: Vec<String> = g.iter().skip(skip).cloned().collect();
                Ok(json!(lines))
            }
            _ => Err(DeltaError::not_found(format!("unknown capability {capability}"))),
        }
    }
}
