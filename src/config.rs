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
pub struct PeerRecord {
    pub node_id: String,
    pub name: String,
    pub url: String,
    pub secret: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub added: f64,
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
    pub node_id: String,
    pub name: String,
    pub listen: String,
    pub port: u16,
    pub url: String,
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
            node_id: String::new(),
            name: String::new(),
            listen: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            url: String::new(),
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
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".delta")
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: buffer is valid for buf.len() bytes
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc != 0 {
        return String::new();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).split('.').next().unwrap_or("").to_string()
}

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
}

fn write_atomic(home: &Path, data: &ConfigData) -> io::Result<()> {
    fs::create_dir_all(home)?;
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    }
    let tmp = home.join("config.tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    let text = serde_json::to_string_pretty(data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, home.join("config.json"))
}

impl Config {
    pub fn new(home: PathBuf, data: ConfigData) -> Self {
        Self { home, data: Mutex::new(data) }
    }

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
        write_atomic(&self.home, &self.data.lock().unwrap())
    }

    pub fn read<R>(&self, f: impl FnOnce(&ConfigData) -> R) -> R {
        f(&self.data.lock().unwrap())
    }

    /// Mutate and always persist.
    pub fn update<R>(&self, f: impl FnOnce(&mut ConfigData) -> R) -> R {
        let mut g = self.data.lock().unwrap();
        let r = f(&mut g);
        if let Err(e) = write_atomic(&self.home, &g) {
            eprintln!("delta: cannot save config: {e}");
        }
        r
    }

    /// Mutate; persist only when the closure returns true.
    pub fn update_if(&self, f: impl FnOnce(&mut ConfigData) -> bool) -> bool {
        let mut g = self.data.lock().unwrap();
        let changed = f(&mut g);
        if changed {
            if let Err(e) = write_atomic(&self.home, &g) {
                eprintln!("delta: cannot save config: {e}");
            }
        }
        changed
    }
}
