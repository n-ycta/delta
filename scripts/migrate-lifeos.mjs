// Explicit local-owner identity migration. Temporary named credentials never enter UI/logs/files.
export async function migrateLifeOS(call,{url,fetcher=fetch,oldName='dev.delta.vector',newName='dev.delta.lifeos',output=()=>{}}){
 const tokens=[];
 const as=async(name,scopes)=>{const t=await call('tokens.create',{name,scopes,ttl_seconds:300});tokens.push(t.id);return async(capability,args)=>{const r=await fetcher(url+'/v1/call',{method:'POST',redirect:'error',headers:{authorization:'Bearer '+t.token,'content-type':'application/json'},body:JSON.stringify({capability,args}),signal:AbortSignal.timeout(30000)});const d=await r.json();if(!r.ok||!d.ok)throw Error('Storage migration request failed: '+(d.error?.code||r.status));return d.result}};
 try{const source=await as(oldName,['call:storage.document_get']),target=await as(newName,['call:storage.document_get','call:storage.document_put']);const old=await source('storage.document_get',{namespace:'delta_vector',key:'state'});if(!old)return {migrated:false,reason:'no_legacy'};
 await target('storage.document_put',{namespace:'delta_lifeos_backup',key:'state',value:old});const existing=await target('storage.document_get',{namespace:'delta_lifeos',key:'state'});
 if(existing&&JSON.stringify(existing)!==JSON.stringify(old)){output('Legacy Vector backup retained. Existing LifeOS data was not overwritten; merge/import explicitly.');return {migrated:false,reason:'existing_lifeos',backup:true}}
 await target('storage.document_put',{namespace:'delta_lifeos',key:'state',value:old});const verified=await target('storage.document_get',{namespace:'delta_lifeos',key:'state'});if(JSON.stringify(verified)!==JSON.stringify(old))throw Error('LifeOS migration verification failed');output('Vector copied to LifeOS with stable IDs; original and backup retained.');return {migrated:true};
 }finally{for(const id of tokens)await call('tokens.revoke',{id}).catch(()=>{})}
}
