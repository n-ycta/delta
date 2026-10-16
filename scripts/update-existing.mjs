// Update only already-installed official plugins on an existing locally-owned node.
// Never init/reinstall a node, add optional plugins, change peers, or erase data.
import {readFile,mkdir,writeFile,realpath} from 'node:fs/promises';
import {resolve,join,dirname} from 'node:path';
import {homedir,platform} from 'node:os';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {recoverWeb} from './recover-web.mjs';
import {spawn} from 'node:child_process';
export function selectUpdates(config,components,catalog){
 const installed=components.filter(c=>config.components.some(s=>s.id===c.id));
 const result=[];
 for(const c of installed){const matches=catalog.plugins.filter(p=>p.component===c.id&&p.id===c.metadata?.ui?.id);if(matches.length>1)throw Error('Ambiguous official package: '+c.id);if(matches.length===1&&matches[0].version!==c.version)result.push(matches[0]);}
 return result;
}
export async function updateExisting({home,output=console.log,recover=false,openBrowser=false,pollMs=1000}={}){
 home=resolve(home||process.env.DELTA_HOME||(platform()==='win32'?join(process.env.LOCALAPPDATA||homedir(),'Delta'):join(homedir(),'.delta')));
 const original=JSON.parse(await readFile(join(home,'config.json'),'utf8')),token=(await readFile(join(home,'admin.token'),'utf8')).trim();
 const record=original.tokens?.find(t=>t.hash===createHash('sha256').update(token).digest('hex'));
 if(!original.node_id||!Number.isInteger(original.port)||!record?.scopes?.includes('admin')||(record.expires_at!=null&&record.expires_at*1000<=Date.now()))throw Error('Existing local owner credential is invalid. Nothing was changed.');
 const call=async(capability,args={})=>{const r=await fetch('http://127.0.0.1:'+original.port+'/v1/call',{method:'POST',headers:{authorization:'Bearer '+token,'content-type':'application/json'},body:JSON.stringify({capability,args}),signal:AbortSignal.timeout(190000),redirect:'error'});const d=await r.json();if(!r.ok||!d.ok)throw Error(d.error?.message||'Core request failed');return d.result};
 const info=await call('core.info');if(info.node_id!==original.node_id)throw Error('Running node does not match DELTA_HOME. Nothing was changed.');await call('identity.list');
 const config={...original,components:[...(original.components||[])]},components=await call('core.components');
 // Include disabled/offline installed packages without launching or enabling them.
 for(const spec of config.components){if(components.some(c=>c.id===spec.id))continue;try{const command=spec.command?.[1];if(!command)continue;const manifest=JSON.parse(await readFile(join(dirname(await realpath(command)),'manifest.json'),'utf8'));if(manifest.id===spec.id)components.push(manifest)}catch{/* Custom tasks are not official packages. */}}
 const catalog=await call('packages.catalog'),updates=selectUpdates(config,components,catalog);
 // Starting an updated process changes enabled state. Do not silently enable a disabled plugin.
 const active=updates.filter(p=>config.components.find(s=>s.id===p.component)?.enabled!==false);
 for(const p of updates.filter(p=>!active.includes(p)))output('Skipped disabled plugin: '+p.component);
 for(const p of active){const plan=await call('packages.plan',{id:p.id});if(plan.items.some(i=>!components.some(c=>c.id===i.component)))throw Error('An additional executable dependency requires explicit review: '+p.id);}
 const backup=join(home,'backups','update-config-'+new Date().toISOString().replace(/[:.]/g,'-'));await mkdir(backup,{recursive:true,mode:0o700});await writeFile(join(backup,'config.json'),JSON.stringify(original,null,2),{mode:0o600});output('Existing node: '+info.name+' ('+info.node_id+'); Core '+info.version);output('Configuration backup: '+backup);
 for(const p of active){output('Updating '+p.component+' → '+p.version);const job=await call('packages.install',{id:p.id,expectedVersion:p.version,expectedSha256:p.sha256});let complete=false;for(let n=0;n<600;n++){await new Promise(r=>setTimeout(r,pollMs));const state=(await call('packages.jobs')).find(j=>j.id===job.jobId);if(state?.status==='failed')throw Error('Update failed for '+p.component+': '+(state.error?.message||'inspect packages.jobs'));if(state?.status==='complete'){complete=true;break;}}if(!complete)throw Error('Update is still running; inspect packages.jobs before retrying.');}
 const after=await call('core.info'),saved=JSON.parse(await readFile(join(home,'config.json'),'utf8'));if(after.node_id!==original.node_id||saved.node_id!==original.node_id||JSON.stringify(saved.peers)!==JSON.stringify(original.peers))throw Error('Node/peer verification failed; inspect the private backup.');
 output(active.length?'Installed plugin updates completed.':'Installed official plugins are already up to date.');
 if(recover){let link='';await recoverWeb({home,output:s=>{output(s);if(/^https?:\/\//.test(s))link=s}});if(openBrowser&&link&&(platform()!=='linux'||process.env.DISPLAY||process.env.WAYLAND_DISPLAY)){const cmd=platform()==='win32'?'rundll32.exe':platform()==='darwin'?'open':'xdg-open';const child=spawn(cmd,platform()==='win32'?['url.dll,FileProtocolHandler',link]:[link],{detached:true,stdio:'ignore'});child.on('error',()=>output('Open the printed one-use link in your browser.'));child.unref();}}return{nodeId:after.node_id,updated:active.map(p=>p.component),backup};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){try{if(process.argv.slice(2).some(a=>!['--recover','--open'].includes(a)))throw Error('Usage: node update-existing.mjs [--recover] [--open]');await updateExisting({recover:process.argv.includes('--recover'),openBrowser:process.argv.includes('--open')})}catch(e){console.error('Stopped:',e.code==='ENOENT'?'Existing Delta files were not found. Run as the original OS user. No new node was created.':e.message);process.exitCode=1}}
