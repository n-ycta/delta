use crate::errors::{DeltaError, Result};
use crate::kernel::{cap, CallContext, CapSpec, Component};
use crate::util::now;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};

pub struct System;

fn memory() -> Value {
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        let field = |key: &str| -> Option<u64> {
            text.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|n| n.parse::<u64>().ok())
                .map(|kb| kb * 1024)
        };
        return json!({"total": field("MemTotal:"), "available": field("MemAvailable:")});
    }
    // macOS
    let total = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok());
    json!({"total": total, "available": Value::Null})
}

fn uptime() -> Value {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|t| t.split_whitespace().next().and_then(|n| n.parse::<f64>().ok()))
        .map(|v| json!(v))
        .unwrap_or(Value::Null)
}

fn load() -> Value {
    let mut l = [0f64; 3];
    // SAFETY: l has room for 3 doubles
    let n = unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    if n == 3 { json!(l) } else { Value::Null }
}

fn disk() -> Value {
    let path = CString::new("/").unwrap();
    // SAFETY: zeroed statvfs is a valid out-parameter; path is NUL-terminated
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut s) == 0 {
            let bs = s.f_frsize as u64;
            let total = s.f_blocks as u64 * bs;
            let free = s.f_bavail as u64 * bs;
            let used = (s.f_blocks as u64).saturating_sub(s.f_bfree as u64) * bs;
            return json!({"total": total, "used": used, "free": free});
        }
    }
    json!({"total": Value::Null, "used": Value::Null, "free": Value::Null})
}

fn uname() -> (String, String, String) {
    // SAFETY: zeroed utsname is a valid out-parameter; fields are NUL-terminated after success
    unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut u) == 0 {
            let f = |p: *const libc::c_char| CStr::from_ptr(p).to_string_lossy().into_owned();
            return (f(u.sysname.as_ptr()), f(u.release.as_ptr()), f(u.machine.as_ptr()));
        }
    }
    (std::env::consts::OS.into(), String::new(), std::env::consts::ARCH.into())
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: buffer is valid for buf.len() bytes
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc != 0 {
        return String::new();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

#[async_trait]
impl Component for System {
    fn id(&self) -> &str {
        "system"
    }
    fn version(&self) -> String {
        "0.1.0".into()
    }
    fn description(&self) -> String {
        "Host information".into()
    }

    fn capabilities(&self) -> Vec<CapSpec> {
        vec![
            cap("system.info", "OS, CPU, memory, disk and load of the host", json!({}), false, false),
            cap("system.ping", "Round-trip check", json!({"echo": {"type": "string"}}), false, false),
        ]
    }

    async fn handle(&self, capability: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        match capability {
            "system.info" => {
                let (os, release, machine) = uname();
                Ok(json!({
                    "hostname": hostname(),
                    "os": os,
                    "release": release,
                    "machine": machine,
                    "runtime": "rust",
                    "cpu_count": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
                    "load": load(),
                    "memory": memory(),
                    "disk": disk(),
                    "uptime": uptime(),
                    "time": now(),
                }))
            }
            "system.ping" => Ok(json!({
                "pong": true,
                "echo": args.get("echo").cloned().unwrap_or(Value::Null),
                "node": ctx.kernel.name,
                "time": now(),
            })),
            _ => Err(DeltaError::not_found(format!("unknown capability {capability}"))),
        }
    }
}
