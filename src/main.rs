//! `delta` command line: node management and a generic client for any capability.
use clap::{Parser, Subcommand};
use delta::auth::TokenStore;
use delta::client::Client;
use delta::config::{default_config, delta_home, Config, DEFAULT_PORT};
use delta::errors::DeltaError;
use delta::node::Node;
use serde_json::{json, Map, Value};
use std::{io::Write, path::PathBuf, process::Command as Proc, sync::Arc};

#[derive(Parser)]
#[command(name = "delta", version, about = "Delta node and client")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// create node identity, config and admin token
    Init {
        #[arg(long, default_value = "desktop", value_parser = ["vps", "android", "desktop", "sbc", "container"])]
        kind: String,
        #[arg(long, default_value = "")]
        name: String,
        /// bind address (0.0.0.0 for other machines)
        #[arg(long, default_value = "127.0.0.1")]
        listen: String,
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
        /// URL other nodes use to reach this one
        #[arg(long, default_value = "")]
        url: String,
        #[arg(long)]
        force: bool,
    },
    /// run the node in the foreground
    Run,
    /// node, components and peers
    Status {
        #[arg(long)]
        json: bool,
    },
    /// list capabilities
    Caps {
        /// include all connected nodes
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// call a capability: delta call system.info [--node X] key=value ...
    Call {
        capability: String,
        args: Vec<String>,
        #[arg(long)]
        node: Option<String>,
        /// arguments as a JSON object
        #[arg(long = "json")]
        json_args: Option<String>,
    },
    /// create a one-time join code
    Invite,
    /// join another node
    Join { url: String, code: String },
    /// list or remove peers
    Peers {
        #[arg(default_value = "list")]
        action: String,
        node: Option<String>,
    },
    /// manage scoped tokens: create NAME SCOPES... | list | revoke ID
    Token {
        action: String,
        name: Option<String>,
        /// admin | call:<glob> | register:<glob>
        scopes: Vec<String>,
    },
    /// managed external components: add|list|remove|start|stop|restart|logs
    Component {
        action: String,
        id: Option<String>,
        /// KEY=VALUE for the process
        #[arg(long)]
        env: Vec<String>,
        /// command after `--`
        #[arg(last = true)]
        cmd: Vec<String>,
    },
    /// install/uninstall as a user service (systemd/launchd)
    Service { action: String },
}

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

fn load_config() -> Arc<Config> {
    let home = delta_home();
    if !Config::exists(&home) {
        die("Delta is not initialised. Run: delta init");
    }
    match Config::load(home) {
        Ok(c) => Arc::new(c),
        Err(e) => die(format!("cannot read config: {e}")),
    }
}

fn make_client() -> Client {
    let (url, token) = (std::env::var("DELTA_URL").ok(), std::env::var("DELTA_TOKEN").ok());
    if let (Some(u), Some(t)) = (&url, &token) {
        return Client::new(u, Some(t.clone()));
    }
    let cfg = load_config();
    let port = cfg.read(|c| c.port);
    let url = url.unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
    let token = token.or_else(|| std::fs::read_to_string(cfg.home.join("admin.token")).ok().map(|t| t.trim().to_string()));
    Client::new(&url, token)
}

fn print_json(v: &Value) {
    match v {
        Value::String(s) => println!("{s}"),
        _ => println!("{}", serde_json::to_string_pretty(v).unwrap_or_default()),
    }
}

fn parse_kv(pairs: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for p in pairs {
        let Some((k, v)) = p.split_once('=') else {
            die(format!("argument {p:?} must be key=value"));
        };
        out.insert(k.to_string(), serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_string())));
    }
    out
}

fn cmd_init(kind: String, name: String, listen: String, port: u16, url: String, force: bool) {
    let home = delta_home();
    if Config::exists(&home) && !force {
        println!("Already initialised at {} (use --force to recreate)", home.join("config.json").display());
        return;
    }
    let cfg = Arc::new(Config::new(home.clone(), default_config(&name, &listen, port, &url)));
    cfg.update(|c| c.node_kind = kind);
    if let Err(e) = cfg.save() {
        die(format!("cannot write config: {e}"));
    }
    let (token, _) = TokenStore::new(cfg.clone())
        .create("admin", &["admin".to_string()], true)
        .unwrap_or_else(|e| die(e));
    let tf: PathBuf = home.join("admin.token");
    {
        let mut options = std::fs::OpenOptions::new(); options.write(true).create(true).truncate(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let mut f = options.open(&tf).unwrap_or_else(|e| die(format!("cannot write admin token: {e}")));
        let _ = writeln!(f, "{token}");
    }
    let (n, id, l, p, u) = cfg.read(|c| (c.name.clone(), c.node_id.clone(), c.listen.clone(), c.port, c.url.clone()));
    println!("Initialised node '{n}' ({id}) at {}", home.display());
    println!("Listening on {l}:{p}, advertised URL {u}");
    println!("Admin token stored in {}", tf.display());
}

async fn cmd_run() -> Result<(), DeltaError> {
    let cfg = load_config();
    let (name, id, listen) = cfg.read(|c| (c.name.clone(), c.node_id.clone(), c.listen.clone()));
    let node = Node::start(cfg, None).await?;
    println!("delta {} node '{name}' ({id}) listening on {listen}:{}", delta::VERSION, node.port());
    node.serve_forever().await;
    Ok(())
}

async fn dispatch(cmd: Cmd) -> Result<(), DeltaError> {
    match cmd {
        Cmd::Init { kind, name, listen, port, url, force } => cmd_init(kind, name, listen, port, url, force),
        Cmd::Run => cmd_run().await?,
        Cmd::Status { json: _ } => {
            let c = make_client();
            let info = c.call("core.info", json!({}), None, None).await?;
            let comps = c.call("core.components", json!({}), None, None).await?;
            let peers = c.call("peers.list", json!({}), None, None).await?;
            print_json(&json!({"node": info, "components": comps, "peers": peers}));
        }
        Cmd::Caps { all, json: as_json } => {
            let caps = make_client().capabilities(if all { "all" } else { "local" }).await?;
            if as_json {
                print_json(&caps);
            } else {
                for c in caps.as_array().cloned().unwrap_or_default() {
                    let flag = if c["admin"].as_bool() == Some(true) {
                        " [admin]"
                    } else if c["public"].as_bool() == Some(true) {
                        " [public]"
                    } else {
                        ""
                    };
                    println!(
                        "{:<14} {:<28} {}{}",
                        c["node_name"].as_str().unwrap_or(""),
                        c["name"].as_str().unwrap_or(""),
                        c["description"].as_str().unwrap_or(""),
                        flag
                    );
                }
            }
        }
        Cmd::Call { capability, args, node, json_args } => {
            let mut obj: Map<String, Value> = match json_args {
                Some(j) => match serde_json::from_str::<Value>(&j) {
                    Ok(Value::Object(o)) => o,
                    _ => die("--json must be a JSON object"),
                },
                None => Map::new(),
            };
            obj.extend(parse_kv(&args));
            let r = make_client().call(&capability, Value::Object(obj), node.as_deref(), None).await?;
            print_json(&r);
        }
        Cmd::Invite => {
            let r = make_client().call("peers.invite", json!({}), None, None).await?;
            println!(
                "Invite code (valid {} min). On the other machine run:\n\n  {}\n",
                r["expires_in"].as_u64().unwrap_or(600) / 60,
                r["command"].as_str().unwrap_or("")
            );
        }
        Cmd::Join { url, code } => {
            let r = make_client().call("peers.join", json!({"url": url, "code": code}), None, None).await?;
            println!("Joined node '{}' ({})", r["name"].as_str().unwrap_or(""), r["node_id"].as_str().unwrap_or(""));
        }
        Cmd::Peers { action, node } => {
            let c = make_client();
            if action == "remove" {
                let n = node.unwrap_or_else(|| die("usage: delta peers remove NODE"));
                print_json(&c.call("peers.remove", json!({"node": n}), None, None).await?);
            } else {
                let peers = c.call("peers.list", json!({}), None, None).await?;
                for p in peers.as_array().cloned().unwrap_or_default() {
                    let state = if p["online"].as_bool() == Some(true) { "online " } else { "offline" };
                    println!(
                        "{state} {:<14} {}  {}",
                        p["name"].as_str().unwrap_or(""),
                        p["node_id"].as_str().unwrap_or(""),
                        p["url"].as_str().unwrap_or("")
                    );
                }
            }
        }
        Cmd::Token { action, name, scopes } => {
            let c = make_client();
            match action.as_str() {
                "create" => {
                    let n = name.unwrap_or_else(|| die("usage: delta token create NAME SCOPES..."));
                    let r = c.call("tokens.create", json!({"name": n, "scopes": scopes}), None, None).await?;
                    println!("{}", r["token"].as_str().unwrap_or(""));
                }
                "revoke" => {
                    let id = name.unwrap_or_else(|| die("usage: delta token revoke ID"));
                    print_json(&c.call("tokens.revoke", json!({"id": id}), None, None).await?);
                }
                "list" => {
                    let r = c.call("tokens.list", json!({}), None, None).await?;
                    for t in r.as_array().cloned().unwrap_or_default() {
                        let scopes: Vec<&str> =
                            t["scopes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                        println!(
                            "{}  {:<20} {}{}",
                            t["id"].as_str().unwrap_or(""),
                            t["name"].as_str().unwrap_or(""),
                            scopes.join(" "),
                            if t["persistent"].as_bool() == Some(true) { "" } else { "  (ephemeral)" }
                        );
                    }
                }
                _ => die("action must be create, list or revoke"),
            }
        }
        Cmd::Component { action, id, env, cmd } => {
            let c = make_client();
            let need_id = || id.clone().unwrap_or_else(|| die("component id required"));
            match action.as_str() {
                "add" => {
                    if cmd.is_empty() {
                        die("usage: delta component add ID -- COMMAND [ARGS...]");
                    }
                    let r = c
                        .call("supervisor.add", json!({"id": need_id(), "command": cmd, "env": parse_kv(&env)}), None, None)
                        .await?;
                    print_json(&r);
                }
                "list" => print_json(&c.call("supervisor.list", json!({}), None, None).await?),
                "logs" => {
                    let r = c.call("supervisor.logs", json!({"id": need_id(), "lines": 200}), None, None).await?;
                    let lines: Vec<&str> = r.as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                    println!("{}", lines.join("\n"));
                }
                "remove" | "start" | "stop" | "restart" => {
                    let r = c.call(&format!("supervisor.{action}"), json!({"id": need_id()}), None, None).await?;
                    print_json(&r);
                }
                _ => die("action must be add, list, remove, start, stop, restart or logs"),
            }
        }
        Cmd::Service { action } => cmd_service(&action),
    }
    Ok(())
}

const SYSTEMD_UNIT: &str = "[Unit]
Description=Delta node
After=network-online.target

[Service]
ExecStart={exe} run
Environment=DELTA_HOME={home}
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
";

const LAUNCHD_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>dev.delta.node</string>
  <key>ProgramArguments</key><array><string>{exe}</string><string>run</string></array>
  <key>EnvironmentVariables</key><dict><key>DELTA_HOME</key><string>{home}</string></dict>
  <key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{home}/delta.log</string>
  <key>StandardErrorPath</key><string>{home}/delta.log</string>
</dict></plist>
"#;

fn run(cmd: &str, args: &[&str], check: bool) {
    let ok = Proc::new(cmd).args(args).status().map(|s| s.success()).unwrap_or(false);
    if check && !ok {
        die(format!("`{cmd} {}` failed", args.join(" ")));
    }
}

fn cmd_service(action: &str) {
    if action != "install" && action != "uninstall" {
        die("action must be install or uninstall");
    }
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "delta".into());
    let home = delta_home().display().to_string();
    let user_home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let render = |t: &str| t.replace("{exe}", &exe).replace("{home}", &home);
    match std::env::consts::OS {
        "linux" => {
            let unit = user_home.join(".config/systemd/user/delta.service");
            if action == "install" {
                if let Some(d) = unit.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                std::fs::write(&unit, render(SYSTEMD_UNIT)).unwrap_or_else(|e| die(e));
                run("systemctl", &["--user", "daemon-reload"], true);
                run("systemctl", &["--user", "enable", "--now", "delta"], true);
                run("loginctl", &["enable-linger", &std::env::var("USER").unwrap_or_default()], false);
                println!("Installed systemd user service {}", unit.display());
            } else {
                run("systemctl", &["--user", "disable", "--now", "delta"], false);
                let _ = std::fs::remove_file(&unit);
                println!("Service removed");
            }
        }
        "macos" => {
            let plist = user_home.join("Library/LaunchAgents/dev.delta.node.plist");
            let p = plist.display().to_string();
            if action == "install" {
                if let Some(d) = plist.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                std::fs::write(&plist, render(LAUNCHD_PLIST)).unwrap_or_else(|e| die(e));
                run("launchctl", &["unload", &p], false);
                run("launchctl", &["load", "-w", &p], true);
                println!("Installed launchd agent {p}");
            } else {
                run("launchctl", &["unload", "-w", &p], false);
                let _ = std::fs::remove_file(&plist);
                println!("Service removed");
            }
        }
        "windows" => {
            let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default());
            let launcher = appdata.join("Microsoft/Windows/Start Menu/Programs/Startup/DeltaNode.cmd");
            if action == "install" {
                if let Some(parent) = launcher.parent() { std::fs::create_dir_all(parent).unwrap_or_else(|e| die(e)); }
                let script = format!("@echo off\r\nset \"DELTA_HOME={home}\"\r\nstart \"\" /B \"{exe}\" run\r\n");
                std::fs::write(&launcher, script).unwrap_or_else(|e| die(e));
                println!("Installed current-user startup launcher {}", launcher.display());
            } else { let _ = std::fs::remove_file(&launcher); println!("Startup launcher removed"); }
        }
        other => die(format!("service management is not supported on {other}; run `delta run` instead")),
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = dispatch(cli.cmd).await {
        eprintln!("error [{}]: {}", e.code, e.message);
        std::process::exit(1);
    }
}
