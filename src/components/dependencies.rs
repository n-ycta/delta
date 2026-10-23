//! Declarative, local-node dependency resolution. No implicit provider selection or code execution.
use crate::{errors::{DeltaError,Result},kernel::{cap,CallContext,CapSpec,Component,Kernel}};
use async_trait::async_trait;
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{BTreeMap,HashSet},sync::Mutex};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
 pub id:String,
 #[serde(default="plugin_kind")] pub kind:String,
 pub version:String,
 #[serde(default)] pub required:bool,
 #[serde(default)] pub features:Vec<String>,
 #[serde(default)] pub contract:Option<String>,
}
fn plugin_kind()->String{"plugin".into()}
pub fn parse(ui:&Value)->Result<Vec<Dependency>> {
 let raw=ui.get("dependencies").cloned().unwrap_or(json!([]));
 let deps:Vec<Dependency>=serde_json::from_value(raw).map_err(|_|DeltaError::bad_request("Invalid dependency schema"))?;
 if deps.len()>32{return Err(DeltaError::bad_request("Too many dependencies"))}
 let mut ids=HashSet::new();
 for d in &deps {
  if d.id.is_empty()||d.id.len()>128||!d.id.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c))||!ids.insert(&d.id)||d.features.len()>32||d.features.iter().any(|f|f.is_empty()||f.len()>64)||!matches!(d.kind.as_str(),"core"|"plugin"|"provider"|"ui-package"|"prerequisite")||semver::VersionReq::parse(&d.version).is_err(){return Err(DeltaError::bad_request("Invalid dependency descriptor"))}
 }
 Ok(deps)
}
pub fn matches(version:&str,range:&str)->bool {semver::Version::parse(version).ok().zip(semver::VersionReq::parse(range).ok()).map(|(v,r)|r.matches(&v)).unwrap_or(false)}
fn bindings(k:&Kernel)->BTreeMap<String,String>{std::fs::read(k.config.home.join("data/dependency-bindings.json")).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default()}
pub fn installed(k:&Kernel)->Vec<Value>{
 let mut rows=k.component_list();
 k.config.read(|c|{for managed in &c.components{
  if let Some(row)=rows.iter_mut().find(|x|x["id"]==managed.id){row["enabled"]=json!(managed.enabled);continue}
  // Only read manifests next to absolute managed entry points, never caller-supplied paths.
  let manifest=managed.command.iter().filter_map(|a|{let p=std::path::Path::new(a);if !p.is_absolute(){return None}let path=p.parent()?.join("manifest.json");std::fs::read(path).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok())}).find(|v|v["id"]==managed.id);
  if let Some(mut v)=manifest{v["registered"]=json!(false);v["enabled"]=json!(managed.enabled);rows.push(v)}
 }});rows
}
pub fn ui_for<'a>(rows:&'a[Value],id:&str)->Option<&'a Value>{rows.iter().find(|r|r["id"]==id||r["metadata"]["ui"]["id"]==id).map(|r|&r["metadata"]["ui"])}
pub fn report(k:&Kernel,ui:&Value)->Result<Value>{
 let rows=installed(k);let binds=bindings(k);let source=ui["id"].as_str().unwrap_or("");
 let mut states=vec![];
 for d in parse(ui)? {
  let mut candidates:Vec<&Value>=rows.iter().filter(|r| if d.kind=="provider"{r["metadata"]["contracts"].get(d.contract.as_deref().unwrap_or(&d.id)).is_some()}else{r["id"]==d.id||r["metadata"]["ui"]["id"]==d.id}).collect();
  if let Some(selected)=binds.get(&format!("{source}:{}",d.id)){candidates.retain(|r|r["id"]==*selected)}else if d.kind=="provider"{candidates.clear()}
  let provider=candidates.first().copied();
  let version=if d.kind=="core"{Some(crate::VERSION)}else if d.kind=="provider"{provider.and_then(|r|r["metadata"]["contracts"].get(d.contract.as_deref().unwrap_or(&d.id))).and_then(Value::as_str)}else{provider.and_then(|r|r["version"].as_str())};
  let state=if matches!(d.kind.as_str(),"ui-package"|"prerequisite"){"unsupported"}else if version.is_none(){if d.kind=="provider"{"unbound"}else{"missing"}}else if !matches(version.unwrap(),&d.version){"incompatible"}else if provider.map(|r|r["enabled"]==false).unwrap_or(false){"disabled"}else if provider.map(|r|r["registered"]==false).unwrap_or(false){"offline"}else{"ready"};
  states.push(json!({"dependency":d,"state":state,"version":version,"component":provider.map(|r|r["id"].clone())}));
 }
 let mut features=BTreeMap::<String,Value>::new();
 for state in &states {for f in state["dependency"]["features"].as_array().unwrap_or(&vec![]) {if let Some(f)=f.as_str(){let entry=features.entry(f.into()).or_insert(json!({"available":true,"blockedBy":[]}));if state["state"]!="ready" {entry["available"]=json!(false);entry["blockedBy"].as_array_mut().unwrap().push(state["dependency"]["id"].clone());}}}}
 Ok(json!({"id":source,"ready":states.iter().all(|s|s["dependency"]["required"]!=true||s["state"]=="ready"),"dependencies":states,"features":features}))
}
pub fn enforce(k:&Kernel,ui:&Value,requires:&[String])->Result<()> {
 if ui.get("dependencies").is_none()&&requires.is_empty(){return Ok(())}
 let state=report(k,ui)?;
 let required_ready=state["ready"]==true;
 let features_ready=requires.iter().all(|f|state["features"].get(f).map(|s|s["available"]==true).unwrap_or(false));
 if !required_ready||!features_ready{return Err(DeltaError::new("missing_dependency","A required provider is missing, disabled, offline or incompatible; inspect dependencies.status"))}Ok(())
}
pub fn removal_blockers(k:&Kernel,id:&str)->Vec<String>{
 let rows=installed(k);let target=rows.iter().find(|r|r["id"]==id);let stable=target.and_then(|r|r["metadata"]["ui"]["id"].as_str()).unwrap_or(id);let binds=bindings(k);
 rows.iter().filter_map(|r|{if r["id"]==id||r["enabled"]==false{return None}let ui=&r["metadata"]["ui"];let source=ui["id"].as_str()?;let used=parse(ui).ok()?.iter().any(|d|d.required&&(d.id==stable||d.id==id||binds.get(&format!("{source}:{}",d.id)).map(|v|v==id).unwrap_or(false)));used.then(||source.to_string())}).collect()
}
/// Topological plan, bounded and cycle-checked. Optional dependencies require explicit selection.
pub fn plan(index:&Value,id:&str,selected:&[String],rows:&[Value])->Result<Value>{plan_bound(index,id,selected,rows,&BTreeMap::new())}
pub fn plan_for_kernel(k:&Kernel,index:&Value,id:&str,selected:&[String])->Result<Value>{plan_bound(index,id,selected,&installed(k),&bindings(k))}
fn plan_bound(index:&Value,id:&str,selected:&[String],rows:&[Value],bindings:&BTreeMap<String,String>)->Result<Value>{
 let catalog=index["plugins"].as_array().ok_or_else(||DeltaError::bad_request("Invalid package index"))?;
 let root=catalog.iter().find(|r|r["id"]==id).ok_or_else(||DeltaError::not_found("Package not found"))?;
 let allowed=parse(&root["manifest"])?;if selected.len()>32||selected.iter().any(|s|!allowed.iter().any(|d|d.id==*s&&!d.required&&d.kind=="plugin")){return Err(DeltaError::bad_request("Unknown optional dependency selection"))}
 struct Walk<'a>{catalog:&'a Vec<Value>,rows:&'a[Value],selected:&'a[String],bindings:&'a BTreeMap<String,String>,visiting:HashSet<String>,done:HashSet<String>,order:Vec<Value>,warnings:Vec<Value>}
 impl Walk<'_>{fn visit(&mut self,item:&Value,root:bool)->Result<()>{
  let id=item["id"].as_str().ok_or_else(||DeltaError::bad_request("Invalid package ID"))?;
  if let Some(platforms)=item["manifest"]["runtime"]["platforms"].as_array(){if !platforms.iter().any(|v|v==std::env::consts::OS){return Err(DeltaError::conflict("Package is not available for this operating system"))}}
  if self.visiting.contains(id){return Err(DeltaError::conflict("Dependency cycle detected"))}if self.done.contains(id){return Ok(())}if self.done.len()+self.visiting.len()>64{return Err(DeltaError::bad_request("Dependency graph too large"))}
  self.visiting.insert(id.into());
  for d in parse(&item["manifest"])?{
   let chosen=d.required||(root&&self.selected.contains(&d.id));
   if d.kind=="core" {if chosen&&!matches(crate::VERSION,&d.version){return Err(DeltaError::conflict("Incompatible core version"))}continue}
   let existing=if d.kind=="provider"{self.bindings.get(&format!("{id}:{}",d.id)).and_then(|component|self.rows.iter().find(|r|r["id"]==*component))}else{self.rows.iter().find(|r|r["metadata"]["ui"]["id"]==d.id||r["id"]==d.id)};
   let ready=existing.map(|r|matches(if d.kind=="provider"{r["metadata"]["contracts"].get(d.contract.as_deref().unwrap_or(&d.id)).and_then(Value::as_str).unwrap_or("")}else{r["version"].as_str().unwrap_or("")},&d.version)&&r["enabled"]!=false&&r["registered"]!=false).unwrap_or(false);
   if !chosen{if !ready{self.warnings.push(json!({"id":d.id,"features":d.features,"reason":"optional_dependency_unavailable"}))}continue}
   if d.kind!="plugin" {if !ready{return Err(DeltaError::conflict("Select a compatible provider explicitly before installation"))}
    if d.kind=="provider" {let current=existing.unwrap();let mut release=current.clone();release["manifest"]=current["metadata"]["ui"].clone();release["id"]=current["metadata"]["ui"]["id"].as_str().map(|s|json!(s)).unwrap_or_else(||current["id"].clone());self.visit(&release,false)?;}continue}
   if let Some(e)=existing {if e["enabled"]==false{return Err(DeltaError::conflict("A required dependency is disabled; enable it explicitly"))}if e["registered"]==false{return Err(DeltaError::conflict("A required dependency is offline; recover it before installation"))}}
   // Validate transitive required edges even when the immediate provider is already installed.
   if ready{let current=existing.unwrap();let mut release=current.clone();release["manifest"]=current["metadata"]["ui"].clone();release["id"]=json!(d.id);self.visit(&release,false)?;}else{let next=self.catalog.iter().find(|r|r["id"]==d.id).ok_or_else(||DeltaError::not_found("Required dependency is not in the catalog"))?;if !matches(next["version"].as_str().unwrap_or(""),&d.version){return Err(DeltaError::conflict("No compatible dependency release"))}self.visit(next,false)?}
  }
  self.visiting.remove(id);self.done.insert(id.into());
  let already=self.rows.iter().any(|r|(r["metadata"]["ui"]["id"]==id||r["id"]==id)&&r["version"]==item["version"]);
  if root||!already{self.order.push(item.clone())}Ok(())
 }}
 let mut w=Walk{catalog,rows,selected,bindings,visiting:HashSet::new(),done:HashSet::new(),order:vec![],warnings:vec![]};w.visit(root,true)?;Ok(json!({"items":w.order,"warnings":w.warnings}))
}
static BINDING_WRITE:Mutex<()>=Mutex::new(());
pub struct Dependencies;
#[async_trait]impl Component for Dependencies{
 fn id(&self)->&str{"dependencies"}
 fn capabilities(&self)->Vec<CapSpec>{vec![cap("dependencies.check","Check whether a capability and its declared features are available",json!({"capability":{"type":"string","required":true}}),false,false),cap("dependencies.status","Inspect dependency availability and feature gates",json!({"id":{"type":"string"}}),false,false),cap("dependencies.bind","Explicitly bind a replaceable local provider",json!({"id":{"type":"string","required":true},"dependency":{"type":"string","required":true},"component":{"type":"string","required":true}}),true,false)]}
 async fn handle(&self,c:&str,a:Value,ctx:&CallContext)->Result<Value>{let rows=installed(&ctx.kernel);match c{
 "dependencies.check"=>{let name=a["capability"].as_str().unwrap_or("");let cap=ctx.kernel.catalog().into_iter().find(|c|c["name"]==name);if let Some(cap)=cap {let row=rows.iter().find(|r|r["id"]==cap["component"]).ok_or_else(||DeltaError::not_found("Capability provider is not registered"))?;let req:Vec<String>=serde_json::from_value(cap["requires"].clone()).unwrap_or_default();let ui=&row["metadata"]["ui"];let status=report(&ctx.kernel,ui)?;let ready=enforce(&ctx.kernel,ui,&req).is_ok();Ok(json!({"available":ready,"pluginId":ui["id"],"requires":req,"status":status}))}else{Ok(json!({"available":false,"reason":"capability_unavailable"}))}},
 "dependencies.status"=>{if let Some(id)=a["id"].as_str(){let ui=ui_for(&rows,id).ok_or_else(||DeltaError::not_found("Plugin not installed"))?;let mut r=report(&ctx.kernel,ui)?;let component=rows.iter().find(|r|r["metadata"]["ui"]["id"]==id||r["id"]==id).and_then(|r|r["id"].as_str()).unwrap_or(id);r["requiredBy"]=json!(removal_blockers(&ctx.kernel,component));Ok(r)}else{Ok(json!(rows.iter().filter_map(|r|report(&ctx.kernel,&r["metadata"]["ui"]).ok()).filter(|r|r["id"]!="").collect::<Vec<_>>()))}},
 "dependencies.bind"=>{if !ctx.principal.is_admin(){return Err(DeltaError::forbidden("Owner access required"))}let id=a["id"].as_str().unwrap_or("");let dep=a["dependency"].as_str().unwrap_or("");let component=a["component"].as_str().unwrap_or("");let ui=ui_for(&rows,id).ok_or_else(||DeltaError::not_found("Plugin not installed"))?;
 let d=parse(ui)?.into_iter().find(|d|d.id==dep&&d.kind=="provider").ok_or_else(||DeltaError::bad_request("Not a replaceable provider dependency"))?;
 let r=rows.iter().find(|r|r["id"]==component).ok_or_else(||DeltaError::not_found("Provider not registered"))?;let version=r["metadata"]["contracts"].get(d.contract.as_deref().unwrap_or(&d.id)).and_then(Value::as_str).unwrap_or("");if !matches(version,&d.version){return Err(DeltaError::conflict("Provider contract is incompatible"))}
 let _lock=BINDING_WRITE.lock().unwrap();let mut b=bindings(&ctx.kernel);b.insert(format!("{id}:{dep}"),component.into());let dir=ctx.kernel.config.home.join("data");std::fs::create_dir_all(&dir).map_err(|_|DeltaError::unavailable("Cannot save provider binding"))?;let tmp=dir.join(format!(".dependency-{}",crate::util::random_hex(8)));use std::io::Write;let mut opts=std::fs::OpenOptions::new();opts.write(true).create_new(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}let mut f=opts.open(&tmp).map_err(|_|DeltaError::unavailable("Cannot save provider binding"))?;f.write_all(&serde_json::to_vec(&b).unwrap()).and_then(|_|f.sync_all()).map_err(|_|DeltaError::unavailable("Cannot persist provider binding"))?;std::fs::rename(tmp,dir.join("dependency-bindings.json")).map_err(|_|DeltaError::unavailable("Cannot activate provider binding"))?;report(&ctx.kernel,ui)},
 _=>Err(DeltaError::not_found("Unknown dependency operation"))}}
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn ranges_and_invalid_schema(){assert!(matches("1.2.3",">=1.0.0, <2.0.0"));assert!(!matches("2.0.0","^1.2"));assert!(parse(&json!({"dependencies":[{"id":"x","version":"??"}]})).is_err())}
 #[test]fn optional_dependencies_are_explicit_and_ordered(){let i=json!({"plugins":[{"id":"web","version":"1.0.0","manifest":{"dependencies":[{"id":"mcp","version":"^1","features":["mcp"]}]}},{"id":"mcp","version":"1.0.0","manifest":{}}]});let p=plan(&i,"web",&[],&[]).unwrap();assert_eq!(p["items"].as_array().unwrap().len(),1);assert_eq!(p["warnings"].as_array().unwrap().len(),1);let p=plan(&i,"web",&["mcp".into()],&[]).unwrap();assert_eq!(p["items"][0]["id"],"mcp");assert!(plan(&i,"web",&["unknown".into()],&[]).is_err())}
 #[test]fn cycles_and_incompatibility_fail_before_execution(){let i=json!({"plugins":[{"id":"a","version":"1.0.0","manifest":{"dependencies":[{"id":"b","version":"^1","required":true}]}},{"id":"b","version":"1.0.0","manifest":{"dependencies":[{"id":"a","version":"^1","required":true}]}}]});assert!(plan(&i,"a",&[],&[]).is_err());let i=json!({"plugins":[{"id":"a","version":"1.0.0","manifest":{"dependencies":[{"id":"b","version":"^2","required":true}]}},{"id":"b","version":"1.0.0","manifest":{}}]});assert!(plan(&i,"a",&[],&[]).is_err())}
 #[test]fn installed_third_party_and_provider_dependencies_are_checked(){
 let index=json!({"plugins":[{"id":"consumer","version":"1.0.0","manifest":{"dependencies":[{"id":"custom","version":"^1","required":true}]}}]});
 let rows=vec![json!({"id":"custom-runtime","version":"1.0.0","metadata":{"ui":{"id":"custom"}}})];
 assert_eq!(plan(&index,"consumer",&[],&rows).unwrap()["items"].as_array().unwrap().len(),1);
 let index=json!({"plugins":[{"id":"consumer","version":"1.0.0","manifest":{"dependencies":[{"id":"backend","kind":"provider","contract":"store","version":"^1","required":true}]}}]});
 let rows=vec![json!({"id":"custom-runtime","version":"1.0.0","metadata":{"contracts":{"store":"1.2.0"},"ui":{"id":"custom","dependencies":[{"id":"absent","version":"^1","required":true}]}}})];
 let binds=BTreeMap::from([("consumer:backend".into(),"custom-runtime".into())]);
 assert!(plan_bound(&index,"consumer",&[],&rows,&binds).is_err());
 assert!(plan(&index,"consumer",&[],&rows).is_err());
 }
 #[test]fn unsupported_platform_fails_before_install(){let index=json!({"plugins":[{"id":"other-os","version":"1.0.0","manifest":{"runtime":{"platforms":["not-this-os"]}}}]});assert!(plan(&index,"other-os",&[],&[]).is_err());}

}
