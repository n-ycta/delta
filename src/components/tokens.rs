use crate::errors::{DeltaError, Result};
use crate::kernel::{cap, CallContext, CapSpec, Component};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct Tokens;

#[async_trait]
impl Component for Tokens {
    fn id(&self) -> &str {
        "tokens"
    }
    fn version(&self) -> String {
        "0.1.0".into()
    }
    fn description(&self) -> String {
        "Scoped API tokens (admin only)".into()
    }

    fn capabilities(&self) -> Vec<CapSpec> {
        vec![
            cap(
                "tokens.create",
                "Create a token. Scopes: admin, call:<glob>, register:<glob>",
                json!({"name": {"type": "string", "required": true}, "scopes": {"type": "array", "required": true}, "ttl_seconds": {"type": "integer"}}),
                true,
                false,
            ),
            cap("tokens.list", "List tokens (secrets are never returned)", json!({}), true, false),
            cap(
                "tokens.revoke",
                "Revoke a token by id",
                json!({"id": {"type": "string", "required": true}}),
                true,
                false,
            ),
        ]
    }

    async fn handle(&self, capability: &str, args: Value, ctx: &CallContext) -> Result<Value> {
        match capability {
            "tokens.create" => {
                let name = args["name"].as_str().unwrap_or("");
                let scopes: Vec<String> = args["scopes"]
                    .as_array()
                    .map(|a| a.iter().map(|s| s.as_str().map(String::from).unwrap_or_else(|| s.to_string())).collect())
                    .unwrap_or_default();
                let ttl = args.get("ttl_seconds").and_then(Value::as_u64);
                if args.get("ttl_seconds").is_some() && (ttl.is_none() || ttl == Some(0) || ttl.unwrap_or(0) > 315360000) {
                    return Err(DeltaError::bad_request("ttl_seconds must be 1..315360000"));
                }
                let (token, rec) = ctx
                    .kernel
                    .tokens
                    .create(name, &scopes, true)
                    .map_err(DeltaError::bad_request)?;
                let expires = ttl.map(|t| crate::util::now() + t as f64);
                ctx.kernel.tokens.set_expiry(&rec.id, expires);
                Ok(json!({"id": rec.id, "name": rec.name, "scopes": rec.scopes, "token": token, "created_at": rec.created_at, "expires_at": expires}))
            }
            "tokens.list" => Ok(Value::Array(ctx.kernel.tokens.list())),
            "tokens.revoke" => {
                let id = args["id"].as_str().unwrap_or("");
                if !ctx.kernel.tokens.revoke(id) {
                    return Err(DeltaError::not_found("no such token"));
                }
                Ok(json!({"revoked": id}))
            }
            _ => Err(DeltaError::not_found(format!("unknown capability {capability}"))),
        }
    }
}
