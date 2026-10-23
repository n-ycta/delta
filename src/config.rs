//! Node configuration: one JSON file, written atomically with 0600 rights.
//! The format is compatible with the Python implementation's `config.json`.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    fs,
    io::{self, Write},
    net::UdpSocket,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const DEFAULT_PORT: u16 = 7700;

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TokenRecord {
    pub id: String,
    pub name: String,
    pub hash: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub created_at: f64,
    #[serde(default)]
    pub expires_at: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IdentityKey { pub id: String, pub name: String, pub public_key: String, pub scopes: Vec<String>, pub created_at: f64 }

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PeerRecord {
    pub node_id: String,
    pub name: String,
    pub url: String,
    pub secret: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub added: f64,
    /// true only when this node itself ran `peers.join` to the peer (it may then ask it for routing)
    #[serde(default)]
    pub joined: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Builtin {
    pub core: bool,
    pub tokens: bool,
    pub peers: bool,
    pub system: bool,
    pub supervisor: bool,
    pub exec: bool,
}

impl Default for Builtin {
    fn default() -> Self {
        Self { core: true, tokens: true, peers: true, system: true, supervisor: true, exec: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ExecCfg {
    pub allow: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ManagedSpec {
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub token_name: Option<String>,
    pub id: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub env: Map<String, Value>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ConfigData {
    pub identity_keys: Vec<IdentityKey>,
    pub identity_origins: Vec<String>,
    pub node_kind: String,
    pub node_id: String,
    pub name: String,
    pub listen: String,
    pub port: u16,
    pub url: String,
    #[serde(default)]
    pub public_url: String,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub tokens: Vec<TokenRecord>,
    pub peers: Vec<PeerRecord>,
    pub builtin: Builtin,
    pub exec: ExecCfg,
    pub components: Vec<ManagedSpec>,
}

impl Default for ConfigData {
    fn default() -> Self {
        Self {
            identity_keys: vec![],
            identity_origins: vec!["http://127.0.0.1:7710".into(), "http://localhost:7710".into()],
            node_kind: "desktop".into(),
            node_id: String::new(),
            name: String::new(),
            listen: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            url: String::new(),
            public_url: String::new(),
            tls_cert: None,
            tls_key: None,
            tokens: vec![],
            peers: vec![],
            builtin: Builtin::default(),
            exec: ExecCfg::default(),
            components: vec![],
        }
    }
}

pub fn delta_home() -> PathBuf {
    if let Some(h) = std::env::var_os("DELTA_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    if cfg!(windows) { std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("Delta") } else { std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".delta") }
}

#[cfg(feature = "host-telemetry")]
fn hostname() -> String { sysinfo::System::host_name().unwrap_or_else(|| "node".into()) }
#[cfg(not(feature = "host-telemetry"))]
fn hostname() -> String { "node".into() }

fn guess_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("10.255.255.255:1")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

pub fn default_config(name: &str, listen: &str, port: u16, url: &str) -> ConfigData {
    let name = if !name.is_empty() {
        name.to_string()
    } else {
        let h = hostname();
        if h.is_empty() { "node".into() } else { h }
    };
    let url = if !url.is_empty() {
        url.to_string()
    } else {
        let host = if listen == "127.0.0.1" || listen == "localhost" { "127.0.0.1".to_string() } else { guess_ip() };
        format!("http://{host}:{port}")
    };
    ConfigData {
        node_id: crate::util::random_hex(8),
        name,
        listen: listen.to_string(),
        port,
        url,
        ..ConfigData::default()
    }
}

pub struct Config {
    pub home: PathBuf,
    data: Mutex<ConfigData>,
    codec:Option<std::sync::Arc<dyn ProfileCodec>>,
}

/// Platform-owned encryption; unavailable keys must fail closed, never fall back to plaintext.
pub trait ProfileCodec: Send + Sync { fn seal(&self, plain:&[u8])->io::Result<Vec<u8>>; fn open(&self, sealed:&[u8])->io::Result<Vec<u8>>; }
fn write_atomic(home: &Path, data: &ConfigData, codec:Option<&std::sync::Arc<dyn ProfileCodec>>) -> io::Result<()> {
    fs::create_dir_all(home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    }
    let tmp = home.join("config.tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    let text = serde_json::to_string_pretty(data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let bytes=match codec {Some(c)=>c.seal(text.as_bytes())?,None=>text.into_bytes()};
    f.write_all(&bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, home.join("config.json"))
}

impl Config {
    pub fn new(home: PathBuf, data: ConfigData) -> Self {
        Self { home, data: Mutex::new(data),codec:None }
    }

    pub fn new_with_codec(home:PathBuf,data:ConfigData,codec:std::sync::Arc<dyn ProfileCodec>)->Self {Self {home,data:Mutex::new(data),codec:Some(codec)}}
    pub fn load_with_codec(home:PathBuf,codec:std::sync::Arc<dyn ProfileCodec>)->io::Result<Self>{let bytes=codec.open(&fs::read(home.join("config.json"))?)?;let data=serde_json::from_slice(&bytes).map_err(|_|io::Error::new(io::ErrorKind::InvalidData,"Invalid encrypted profile"))?;Ok(Self::new_with_codec(home,data,codec))}
    pub fn path(&self) -> PathBuf {
        self.home.join("config.json")
    }

    pub fn exists(home: &Path) -> bool {
        home.join("config.json").exists()
    }

    pub fn load(home: PathBuf) -> io::Result<Self> {
        let text = fs::read_to_string(home.join("config.json"))?;
        let data: ConfigData =
            serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Self::new(home, data))
    }

    pub fn save(&self) -> io::Result<()> {
        write_atomic(&self.home, &self.data.lock().unwrap(),self.codec.as_ref())
    }

    pub fn read<R>(&self, f: impl FnOnce(&ConfigData) -> R) -> R {
        f(&self.data.lock().unwrap())
    }

    /// Mutate and always persist.
    pub fn update<R>(&self, f: impl FnOnce(&mut ConfigData) -> R) -> R {
        let mut g = self.data.lock().unwrap();
        let r = f(&mut g);
        if let Err(e) = write_atomic(&self.home, &g,self.codec.as_ref()) {
            eprintln!("delta: cannot save config: {e}");
        }
        r
    }

    /// Mutate; persist only when the closure returns true.
    pub fn update_if(&self, f: impl FnOnce(&mut ConfigData) -> bool) -> bool {
        let mut g = self.data.lock().unwrap();
        let changed = f(&mut g);
        if changed {
            if let Err(e) = write_atomic(&self.home, &g,self.codec.as_ref()) {
                eprintln!("delta: cannot save config: {e}");
            }
        }
        changed
    }
}
