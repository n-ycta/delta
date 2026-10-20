import{migrateLifeOS}from'./migrate-lifeos.mjs';
// Local owner maintenance. No updater plugin, reset, shell commands or credential output.
import{readFile,mkdir,cp,writeFile}from'node:fs/promises';import{resolve,join}from'node:path';import{homedir}from'node:os';
export async function repair({home=process.env.DELTA_HOME||join(homedir(),'.delta'),fetcher=fetch,output=console.log,webInstall=true,pollMs=500}={}){
 home=resolve(home);const config=JSON.parse(await readFile(join(home,'config.json'),'utf8')),token=(await readFile(join(home,'admin.token'),'utf8')).trim();
 const call=async(capability,args={})=>{const r=await fetcher(`http://127.0.0.1:${config.port}/v1/call`,{method:'POST',redirect:'error',headers:{authorization:'Bearer '+token,'content-type':'application/json'},body:JSON.stringify({capability,args}),signal:AbortSignal.timeout(65000)});const d=await r.json();if(!r.ok||!d.ok)throw Object.assign(Error(d.error?.code||'request_failed'),{code:d.error?.code});return d.result};
 const ready=async(cap,predicate)=>{let last;for(let n=0;n<120;n++){try{last=await call(cap);if(predicate(last))return last}catch(e){last={error:e.code||e.message}}await new Promise(r=>setTimeout(r,pollMs))}throw Error(cap+' readiness timeout: '+JSON.stringify(last));};
 const info=await call('core.info');if(info.node_id!==config.node_id)throw Error('DELTA_HOME does not match the running node. Nothing changed.');await call('identity.list');
 const backup=join(home,'backups','repair-'+new Date().toISOString().replace(/[:.]/g,'-'));await mkdir(backup,{recursive:true,mode:0o700});await cp(join(home,'config.json'),join(backup,'config.json'));try{await cp(join(home,'data'),join(backup,'data'),{recursive:true,errorOnExist:true,force:false})}catch(e){if(e.code!=='ENOENT')throw e}output('Private configuration/data snapshot: '+backup+' (external PostgreSQL volumes are not copied).');
 let catalog=await call('packages.catalog'),managed=await call('supervisor.list');
 const retired=managed.find(x=>x.id==='vector'),selected=new Set(['storage','web','typing','lifeos','updater','observability']);
 for(const item of managed)if(item.id!=='vector'&&catalog.plugins.some(x=>x.component===item.id))selected.add(item.id);
 // Stop the retired writer only after the backup. Keep its files and restore on LifeOS failure.
 let stoppedLifeOS=false;
 if(retired?.enabled)await call('supervisor.stop',{id:'vector'});
 try{
 for(const component of selected){if(component==='lifeos'&&retired){await ready('storage.status',x=>x.ready);const running=managed.find(x=>x.id==='lifeos');if(running?.enabled){await call('supervisor.stop',{id:'lifeos'});stoppedLifeOS=true;}const m=await migrateLifeOS(call,{url:`http://127.0.0.1:${config.port}`,fetcher,oldName:retired.token_name||'dev.delta.vector',newName:running?.token_name||'dev.delta.lifeos',output});if(m.reason==='existing_lifeos')throw Error('Both Vector and LifeOS contain data. Both are preserved; merge/import the backup before removing Vector.');}const entry=catalog.plugins.find(x=>x.component===component);if(!entry)throw Error('Official catalog entry missing: '+component);const old=managed.find(x=>x.id===component);if(old&&old.enabled===false&&!['storage','web','lifeos'].includes(component)){output('Preserving disabled component: '+component);continue}
 output('Verify/install '+entry.id+' '+entry.version);const plan=await call('packages.plan',{id:entry.id});for(const x of plan.items)output('Approved package '+x.id+' scopes: '+(x.scopes||[]).join(', '));
 const result=await call('packages.install',{id:entry.id,expectedVersion:entry.version,expectedSha256:entry.sha256,approveDependencies:true,optionalDependencies:[]});let done=false;
 for(let n=0;n<600;n++){await new Promise(r=>setTimeout(r,pollMs));const job=(await call('packages.jobs')).find(j=>j.id===result.jobId);if(job?.status==='failed')throw Error(component+': '+(job.error?.code||'install_failed'));if(job?.status==='complete'){done=true;break}}if(!done)throw Error(component+': installation timeout');if((old?.enabled===false||component==='lifeos'&&retired)&&['storage','web','lifeos'].includes(component)){output('Enabling required component: '+component);await call('supervisor.start',{id:component})}
 }
 // Do not silently switch an existing PostgreSQL database to empty files.
 const state=await ready('storage.status',x=>x.ready);if(!state.ready)throw Error('Storage remains unavailable ('+(state.job?.error||state.job?.state||'unknown')+'). Existing PostgreSQL has not been replaced. Inspect storage.status and storage settings.');
 const lifeos=await ready('lifeos.status',x=>x.storage==='connected');if(lifeos.storage!=='connected')throw Error('LifeOS has not connected to Storage yet; rerun after storage.status is ready.');
 if(webInstall){const latest=JSON.parse(await readFile(join(home,'config.json'),'utf8')),web=latest.components?.find(x=>x.id==='web'),env=web?.env||{},origin=new URL(`http://127.0.0.1:${env.PORT||7710}`),consoleOrigin=new URL(env.PUBLIC_ORIGIN||origin.origin).origin;
 const app=await fetcher(origin.origin+'/apps/install',{method:'POST',redirect:'error',headers:{authorization:'Bearer '+token,origin:consoleOrigin,'content-type':'application/json'},body:JSON.stringify({id:'dev.delta.lifeos',replace:true}),signal:AbortSignal.timeout(120000)});if(!app.ok)throw Error('LifeOS portable app install failed: HTTP '+app.status);
 }
 if(retired){await call('supervisor.remove',{id:'vector',force:true});output('Retired Vector runtime removed; data and migration backups retained.');}
 const report={at:new Date().toISOString(),node:info.node_id,storage:state.ready,lifeos:lifeos.storage,components:(await call('core.components')).map(c=>({id:c.id,version:c.version}))};await writeFile(join(backup,'report.json'),JSON.stringify(report,null,2),{mode:0o600});output('Completed. Open Delta and select LifeOS in the app switcher. Refresh the Console once.');return report;
 }catch(e){if(stoppedLifeOS)await call('supervisor.start',{id:'lifeos'}).catch(()=>{});if(retired?.enabled)await call('supervisor.start',{id:'vector'}).catch(()=>{});throw e}
}
if(process.argv[1]&&import.meta.url===(await import('node:url')).pathToFileURL(resolve(process.argv[1])).href){try{await repair()}catch(e){console.error('Repair stopped:',e.message);process.exitCode=1}}
