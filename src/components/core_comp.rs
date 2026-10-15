use crate::errors::{DeltaError, Result};
use crate::kernel::{cap, CallContext, CapSpec, Component};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct Core;

#[async_trait]
impl Component for Core {
    fn id(&self) -> &str {
        "core"
    }
    fn version(&self) -> String {
        "0.1.0".into()
    }
    fn description(&self) -> String {
        "Node identity and discovery of components and capabilities".into()
    }

    fn capabilities(&self) -> Vec<CapSpec> {
        let none = json!({});
        vec![
            cap("core.info", "Identity, version and uptime of this node", none.clone(), false, false),
            cap("core.components", "Components registered on this node", none.clone(), false, false),
            cap("core.capabilities", "Capabilities offered by this node", none.clone(), false, false),
            cap("core.catalog", "Capabilities of this node and all reachable peers", none, false, false),
        ]
    }

    async fn handle(&self, capability: &str, _args: Value, ctx: &CallContext) -> Result<Value> {
        match capability {
            "core.info" => Ok(ctx.kernel.info()),
            "core.components" => Ok(Value::Array(ctx.kernel.component_list())),
            "core.capabilities" => Ok(Value::Array(ctx.kernel.catalog())),
            "core.catalog" => {
                let mut caps = ctx.kernel.catalog();
                if let Some(r) = ctx.kernel.router() {
                    caps.extend(r.remote_catalog().await);
                }
                Ok(Value::Array(caps))
            }
            _ => Err(DeltaError::not_found(format!("unknown capability {capability}"))),
        }
    }
}
