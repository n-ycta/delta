use crate::errors::{DeltaError, Result};
use crate::kernel::{cap, CallContext, CapSpec, Component};
use crate::util::now;
use async_trait::async_trait;
use serde_json::{json, Value};
use sysinfo::{Disks, System as Host, MINIMUM_CPU_UPDATE_INTERVAL};
pub struct System;
#[async_trait]
impl Component for System {
    fn id(&self) -> &str { "system" }
    fn version(&self) -> String { "0.3.0".into() }
    fn description(&self) -> String { "Cross-platform host telemetry".into() }
    fn capabilities(&self) -> Vec<CapSpec> { vec![
        cap("system.info", "OS, CPU, memory, disk and uptime", json!({}), false, false),
        cap("system.ping", "Round-trip check", json!({"echo":{"type":"string"}}), false, false),
    ] }
    async fn handle(&self, capability: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        match capability {
            "system.info" => {
                let mut host=Host::new(); host.refresh_memory(); host.refresh_cpu_all();
                tokio::time::sleep(MINIMUM_CPU_UPDATE_INTERVAL).await; host.refresh_cpu_all();
                let disks=Disks::new_with_refreshed_list(); let disk=disks.iter().next();
                let load=Host::load_average();
                Ok(json!({"hostname":Host::host_name(),"os":Host::name().unwrap_or_else(|| std::env::consts::OS.into()),"release":Host::kernel_version(),"machine":Host::cpu_arch(),"runtime":"rust","cpu_count":host.cpus().len(),"cpu_usage":host.global_cpu_usage(),"load":if cfg!(windows) {Value::Null} else {json!([load.one,load.five,load.fifteen])},"memory":{"total":host.total_memory(),"available":host.available_memory()},"disk":{"total":disk.map(|d|d.total_space()),"used":disk.map(|d|d.total_space()-d.available_space()),"free":disk.map(|d|d.available_space())},"uptime":Host::uptime(),"time":now()}))
            }
            "system.ping" => Ok(json!({"pong":true,"echo":args.get("echo").cloned().unwrap_or(Value::Null),"node":ctx.kernel.name,"time":now()})),
            _ => Err(DeltaError::not_found("unknown capability")),
        }
    }
}
