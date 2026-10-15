//! Schnorr/BIP340 challenge authentication. Private keys and seed phrases never reach the core.
use crate::{config::IdentityKey,errors::{DeltaError,Result},kernel::{cap,CallContext,CapSpec,Component},util::{now,random_hex}};
use async_trait::async_trait;use serde_json::{json,Value};use std::{collections::HashMap,sync::Mutex};use sha2::{Digest,Sha256};use k256::schnorr::{Signature,VerifyingKey};
struct Challenge { public_key:String, audience:String, text:String, expires:f64 }
pub struct Identity { challenges:Mutex<HashMap<String,Challenge>> }
impl Identity { pub fn new()->Self{Self{challenges:Mutex::new(HashMap::new())}} }
fn public_key(text:&str)->Result<VerifyingKey>{let bytes=hex::decode(text).map_err(|_|DeltaError::bad_request("public_key must be 32-byte x-only hex"))?;VerifyingKey::from_bytes(&bytes).map_err(|_|DeltaError::bad_request("invalid public key"))}
#[async_trait]impl Component for Identity {
 fn id(&self)->&str{"identity"} fn version(&self)->String{"0.3.0".into()}
 fn capabilities(&self)->Vec<CapSpec>{vec![
  cap("identity.challenge","Request a short-lived signed-login challenge",json!({"public_key":{"type":"string","required":true},"audience":{"type":"string","required":true}}),false,true),
  cap("identity.login","Verify a one-use BIP340 signature and issue a short-lived token",json!({"nonce":{"type":"string","required":true},"signature":{"type":"string","required":true}}),false,true),
  cap("identity.enroll","Enroll a public key; requires an existing owner session",json!({"public_key":{"type":"string","required":true},"name":{"type":"string","required":true},"scopes":{"type":"array","required":true}}),true,false),
  cap("identity.list","List enrolled public keys",json!({}),true,false),
  cap("identity.remove","Revoke a public key",json!({"id":{"type":"string","required":true}}),true,false),
 ]}
 async fn handle(&self,capability:&str,args:Value,ctx:&CallContext)->Result<Value>{
  let s=|k:&str|args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
  match capability {
   "identity.challenge"=>{let pk=s("public_key").to_lowercase();public_key(&pk)?;let audience=s("audience");if !ctx.kernel.config.read(|c|c.identity_origins.contains(&audience)){return Err(DeltaError::forbidden("origin is not configured for key login"))}
    let nonce=random_hex(32);let expires=(now()+90.0).floor();let text=format!("delta-login-v1\n{}\n{pk}\n{nonce}\n{}\n{audience}",ctx.kernel.node_id,expires as u64);
    let mut challenges=self.challenges.lock().unwrap();challenges.retain(|_,c|c.expires>now());if challenges.len()>=1024{return Err(DeltaError::unavailable("too many outstanding challenges"))}
    challenges.insert(nonce.clone(),Challenge{public_key:pk.clone(),audience:audience.clone(),text:text.clone(),expires});Ok(json!({"nonce":nonce,"message":text,"node_id":ctx.kernel.node_id,"public_key":pk,"audience":audience,"expires_at":expires}))
   }
   "identity.login"=>{let challenge=self.challenges.lock().unwrap().remove(&s("nonce")).ok_or_else(||DeltaError::unauthorized("invalid or consumed challenge"))?;if challenge.expires<now(){return Err(DeltaError::unauthorized("challenge expired"))}
    let bytes=hex::decode(s("signature")).map_err(|_|DeltaError::unauthorized("invalid signature"))?;let signature=Signature::try_from(bytes.as_slice()).map_err(|_|DeltaError::unauthorized("invalid signature"))?;
    public_key(&challenge.public_key)?.verify_raw(&Sha256::digest(challenge.text.as_bytes()),&signature).map_err(|_|DeltaError::unauthorized("signature verification failed"))?;
    let key=ctx.kernel.config.read(|c|c.identity_keys.iter().find(|k|k.public_key==challenge.public_key).cloned()).ok_or_else(||DeltaError::unauthorized("public key is not enrolled"))?;
    let (token,record)=ctx.kernel.tokens.create(&format!("key:{}",key.id),&key.scopes,false).map_err(DeltaError::bad_request)?;let expiry=now()+28800.0;ctx.kernel.tokens.set_expiry(&record.id,Some(expiry));Ok(json!({"token":token,"id":record.id,"name":key.name,"expires_at":expiry,"scopes":key.scopes,"audience":challenge.audience}))
   }
   "identity.enroll"=>{if !ctx.principal.is_admin(){return Err(DeltaError::forbidden("enrollment requires global admin"))}let pk=s("public_key").to_lowercase();public_key(&pk)?;let name=s("name");if name.trim().is_empty()||name.len()>128{return Err(DeltaError::bad_request("invalid name"))}
    let scopes:Vec<String>=args["scopes"].as_array().unwrap().iter().map(|v|v.as_str().unwrap_or("").to_string()).collect();if scopes.is_empty()||scopes.iter().any(|v|!crate::auth::valid_scope(v)){return Err(DeltaError::bad_request("invalid scopes"))}
    if ctx.kernel.config.read(|c|c.identity_keys.iter().any(|k|k.public_key==pk)){return Err(DeltaError::conflict("public key already enrolled"))}
    let key=IdentityKey{id:random_hex(12),name,public_key:pk,scopes,created_at:now()};ctx.kernel.config.update(|c|c.identity_keys.push(key.clone()));Ok(serde_json::to_value(key).unwrap())
   }
   "identity.list"=>Ok(ctx.kernel.config.read(|c|json!(c.identity_keys))),
   "identity.remove"=>{let id=s("id");let found=ctx.kernel.config.update_if(|c|{let before=c.identity_keys.len();c.identity_keys.retain(|k|k.id!=id);before!=c.identity_keys.len()});if !found{return Err(DeltaError::not_found("unknown key"))}for token in ctx.kernel.tokens.list(){if token["name"]==format!("key:{id}"){if let Some(t)=token["id"].as_str(){ctx.kernel.tokens.revoke(t);}}}Ok(json!({"revoked":id}))},
   _=>Err(DeltaError::not_found("unknown capability"))
  }
 }
}
