//! Native root-document bridge. Never expose this transport directly to plugin frames.
use delta::{auth::{Kind, Principal},client::Client,config::{Config,default_config},errors::{DeltaError,Result},node::{Node,HostPolicy}};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{path::PathBuf,sync::Arc,time::Duration};
use tokio::sync::Mutex;
use fs2::FileExt;
pub const BRIDGE_VERSION:&str="1.0.0";
pub const MAX_REQUEST_BYTES:usize=65536;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {pub capability:String,#[serde(default="empty")]pub args:Value,pub node:Option<String>}
fn empty()->Value{json!({})}
impl Request {pub fn parse(value:Value)->Result<Self>{
 if serde_json::to_vec(&value).map_err(|_|DeltaError::bad_request("invalid_request"))?.len()>MAX_REQUEST_BYTES {return Err(DeltaError::bad_request("request_too_large"))}
 let request:Self=serde_json::from_value(value).map_err(|_|DeltaError::bad_request("invalid_request"))?;
 if request.capability.len()>128||!delta::util::valid_capability(&request.capability)||!request.args.is_object()||request.node.as_ref().is_some_and(|n|n.is_empty()||n.len()>128) {return Err(DeltaError::bad_request("invalid_request"))}
 Ok(request)
}}
enum Backend {Embedded(Mutex<Option<Node>>), Existing(Client)}
pub struct HostRuntime {backend:Backend,kind:String,node_id:String,core_version:String,_lease:std::fs::File}
impl HostRuntime {
 /// Caller supplies a platform-owned profile path, never a path received from JavaScript.
 pub async fn open(home:PathBuf,kind:&str)->Result<Self>{
  if !["desktop","android"].contains(&kind){return Err(DeltaError::bad_request("invalid_host_kind"))}
  let existed=Config::exists(&home);
  let cfg=Arc::new(if existed {Config::load(home.clone()).map_err(|_|DeltaError::unavailable("profile_unreadable"))?}else{
   let mut data=default_config("Delta", "127.0.0.1",0,"");data.node_kind=kind.into();
   #[cfg(not(feature="desktop"))]{data.builtin.system=false;data.builtin.supervisor=false;data.builtin.exec=false;}
   let c=Config::new(home.clone(),data);c.save().map_err(|_|DeltaError::unavailable("profile_not_saved"))?;c
  });
  let mut options=std::fs::OpenOptions::new();options.read(true).write(true).create(true);
  #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt;options.mode(0o600); }
  let lease=options.open(home.join("host-runtime.lock")).map_err(|_|DeltaError::unavailable("profile_lock_failed"))?;
  lease.try_lock_exclusive().map_err(|_|DeltaError::new("conflict","profile_already_open"))?;
  let node_id=cfg.read(|c|c.node_id.clone());
  if existed&&cfg.read(|c|c.port)!=0 {
   let port=cfg.read(|c|c.port);
   // Existing profile authentication stays in Rust; verify identity before attaching.
   if tokio::net::TcpStream::connect(("127.0.0.1",port)).await.is_ok(){
    let token=std::fs::read_to_string(home.join("admin.token")).map_err(|_|DeltaError::forbidden("existing_profile_requires_owner_credential"))?;
    let client=Client::new(&format!("http://127.0.0.1:{port}"),Some(token.trim().into())).with_timeout(Duration::from_secs(35));
    let info=client.call("core.info",json!({}),None,None).await?;
    if info["node_id"]!=node_id{return Err(DeltaError::forbidden("node_changed"))}
    return Ok(Self{backend:Backend::Existing(client),kind:kind.into(),node_id,core_version:info["version"].as_str().unwrap_or("unknown").into(),_lease:lease})
   }
  }
  // No unrequested second identity/profile is created. Existing bind failures remain errors.
  let policy=if kind=="android" {HostPolicy::MOBILE}else{HostPolicy::DESKTOP};
  let node=Node::start_with_policy(cfg,None,policy).await?;
  Ok(Self{backend:Backend::Embedded(Mutex::new(Some(node))),kind:kind.into(),node_id,core_version:delta::VERSION.into(),_lease:lease})
 }
 pub async fn call(&self,value:Value)->Result<Value>{let request=Request::parse(value)?;
  match &self.backend {
   Backend::Existing(client)=>client.call(&request.capability,request.args,request.node.as_deref(),None).await,
   Backend::Embedded(state)=>{let kernel=state.lock().await.as_ref().ok_or_else(||DeltaError::unavailable("node_stopped"))?.kernel.clone();
    let principal=Principal{kind:Kind::Admin,name:"native-owner".into(),scopes:vec!["admin".into()],peer_id:String::new()};
    kernel.call(&request.capability,request.args,&principal,request.node.as_deref()).await}
  }
 }
 pub fn info(&self,host_version:&str,ui_version:&str)->Value{json!({"kind":self.kind,"version":host_version,"bridgeVersion":BRIDGE_VERSION,"coreVersion":self.core_version,"uiVersion":ui_version,"nodeId":self.node_id,"features":["embedded-node","capability-api","persistent-profile"],"lifecycle":"foreground","development":true})}
 pub async fn stop(&self){if let Backend::Embedded(state)=&self.backend {let node=state.lock().await.take();if let Some(node)=node {node.stop().await}}}
}
#[cfg(test)]mod tests {use super::*;
 #[test]fn bounded_contract(){assert!(Request::parse(json!({"capability":"core.info"})).is_ok());for bad in [json!({"capability":"../exec"}),json!({"capability":"core.info","url":"http://evil"}),json!({"capability":"core.info","args":[]}),json!({"capability":"core.info","args":{"x":"x".repeat(70000)}})] {assert!(Request::parse(bad).is_err())}}
 #[tokio::test]async fn portable_identity_survives_restart(){let home=std::env::temp_dir().join(format!("delta-host-{}",delta::util::random_hex(10)));let h=HostRuntime::open(home.clone(),"android").await.unwrap();let a=h.call(json!({"capability":"core.info"})).await.unwrap();let caps=h.call(json!({"capability":"core.capabilities"})).await.unwrap();
 #[cfg(not(feature="desktop"))]assert!(!caps.as_array().unwrap().iter().any(|c|c["name"].as_str().unwrap_or("").starts_with("supervisor.")||c["name"].as_str().unwrap_or("").starts_with("packages.")));
 h.stop().await;drop(h);let next=HostRuntime::open(home.clone(),"android").await.unwrap();assert_eq!(a["node_id"],next.call(json!({"capability":"core.info"})).await.unwrap()["node_id"]);next.stop().await;std::fs::remove_dir_all(home).unwrap();}
}
