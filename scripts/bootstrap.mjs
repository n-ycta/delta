import {stopPreviousNode} from './upgrade.mjs';
// Installer orchestration; no package dependencies, no Rust toolchain.
import {readFile,writeFile,mkdir,access} from 'node:fs/promises';import {resolve,dirname} from 'node:path';import {homedir,hostname,platform} from 'node:os';import {spawn,spawnSync} from 'node:child_process';import {randomBytes,createHash} from 'node:crypto';
const argv=process.argv.slice(2),value=(k,d='')=>{const i=argv.indexOf(k);return i<0?d:argv[i+1]},has=k=>argv.includes(k);
const home=process.env.DELTA_HOME|| (platform()==='win32'?resolve(process.env.LOCALAPPDATA||homedir(),'Delta'):resolve(homedir(),'.delta'));
process.env.DELTA_HOME=home;process.env.DELTA_NODE_BINARY=process.execPath;
const binary=resolve(home,'bin',platform()==='win32'?'delta.exe':'delta');
let url='http://127.0.0.1:'+value('--port','7700');
await mkdir(home,{recursive:true,mode:0o700});
const run=args=>{const r=spawnSync(binary,args,{stdio:'inherit',env:process.env});if(r.status!==0)throw Error('Delta command failed: '+args[0]);};
try{await access(resolve(home,'config.json'))}catch{run(['init','--kind',value('--kind','desktop'),'--name',value('--name',hostname()),'--listen',value('--listen','127.0.0.1'),'--port',value('--port','7700'),...(value('--public-url')?['--url',value('--public-url')]:[])]);}
const config=JSON.parse(await readFile(resolve(home,'config.json'),'utf8'));
url='http://127.0.0.1:'+config.port;
const token=(await readFile(resolve(home,'admin.token'),'utf8')).trim();
async function call(capability,args={}){const r=await fetch(url+'/v1/call',{method:'POST',headers:{authorization:'Bearer '+token,'content-type':'application/json'},body:JSON.stringify({capability,args})});const d=await r.json();if(!d.ok)throw Error(d.error?.message||'Delta request failed');return d.result;}
async function online(){try{await call('core.info');return true;}catch{return false;}}
const versionOutput=spawnSync(binary,['--version'],{encoding:'utf8'});
const installedVersion=versionOutput.status===0?versionOutput.stdout.trim().split(/\s+/).at(-1):null;
if(await online()){const running=await call('core.info');if(installedVersion&&running.version&&running.version!==installedVersion){console.log(`Updating running node ${running.version} → ${installedVersion}…`);await stopPreviousNode({home,binary,online});}}
// Repair/enable autostart even when a previous --no-service node is already online.
let service=false;
if(!has('--no-service')){if(platform()==='linux'&&await online()){const active=spawnSync('systemctl',['--user','is-active','--quiet','delta.service']);if(active.status!==0)await stopPreviousNode({home,binary,online});}const r=spawnSync(binary,['service','install'],{stdio:'inherit',env:process.env});if(r.status!==0)throw Error('Could not enable Delta autostart. No reboot-ready installation was completed. Use --no-service only for an intentional temporary run.');service=true;}
if(!await online()){
 for(let i=0;service&&i<15&&!await online();i++)await new Promise(r=>setTimeout(r,1000));
 if(!await online()&&service)throw Error('Delta service did not become ready; inspect systemctl --user status delta.service and journalctl --user -u delta.service.');
 if(!await online()){const log=resolve(home,'node.log');const {openSync,closeSync}=await import('node:fs');const out=openSync(log,'a',0o600);const child=spawn(binary,['run'],{detached:true,stdio:['ignore',out,out],env:process.env,windowsHide:true});child.unref();closeSync(out);console.log('Node started in background. Log:',log);}
 for(let i=0;i<30&&!await online();i++)await new Promise(r=>setTimeout(r,500));if(!await online())throw Error('Node did not start; inspect '+resolve(home,'node.log'));
}
const plugins=value('--plugins','storage,web').split(',').filter(p=>p&&p!=='none');
const registry=plugins.length?await call('packages.catalog'):{plugins:[]};
const selected=[];
const resolvePackage=input=>{const matches=registry.plugins.filter(p=>p.id===input||p.component===input);if(matches.length!==1)throw Error('Select an unambiguous official plugin ID or component: '+input);return matches[0]};
for(const plugin of plugins){const expected=resolvePackage(plugin);selected.push(expected.component);const cs=await call('core.components');const current=cs.find(c=>c.metadata?.ui?.id===expected.id);if(current?.version===expected.version){console.log(plugin,expected.version,'already installed');continue;}console.log('Installing',plugin,'from verified GitHub release…');const optionalDependencies=has('--with-optional')?(expected.manifest?.dependencies||[]).filter(d=>!d.required&&d.kind==='plugin').map(d=>d.id):[];const plan=await call('packages.plan',{id:expected.id,optionalDependencies});if(plan.items.length>1&&!has('--approve-dependencies')&&!has('--with-optional'))throw Error('Additional executable dependencies require --approve-dependencies: '+plan.items.map(i=>i.id).join(', '));for(const item of plan.items)console.log('Approved package',item.id,item.version,'scopes:',(item.scopes||[]).join(', '));const j=await call('packages.install',{id:expected.id,expectedVersion:expected.version,expectedSha256:expected.sha256,optionalDependencies,approveDependencies:has('--approve-dependencies')||has('--with-optional')});let done=false;for(let i=0;i<480;i++){await new Promise(r=>setTimeout(r,1000));const jobs=await call('packages.jobs');const job=jobs.find(jj=>jj.id===j.jobId);if(job?.status==='failed')throw Error(job.error?.message||'Installation failed');if(job?.status==='complete'){done=true;break;}}if(!done)throw Error('Installation timed out; inspect packages.jobs');}
if(value('--database-url')){const configPath=resolve(home,'config.json'),cfg=JSON.parse(await readFile(configPath,'utf8'));const spec=cfg.components.find(c=>c.id==='storage');if(!spec)throw Error('Install storage first');console.log('Set DATABASE_URL in the private node configuration, then restart storage. Do not pass database credentials on the command line.');throw Error('--database-url is not supported; use private config environment');}
if(value('--join-url')){if(!value('--join-code')||!value('--public-url'))throw Error('Joining requires --join-code and --public-url');const hub=await call('peers.join',{url:value('--join-url'),code:value('--join-code'),trust_admin:has('--trust-hub')});console.log('Joined',hub.name,hub.node_id);}
if(selected.includes('web')||has('--open')){
 const code=randomBytes(32).toString('hex'),dir=resolve(home,'data','web');await mkdir(dir,{recursive:true,mode:0o700});await writeFile(resolve(dir,'bootstrap.json'),JSON.stringify({hash:createHash('sha256').update(code).digest('hex'),expires:Date.now()+300000}),{mode:0o600});
 const link='http://127.0.0.1:7710/#/identity?code='+code;
 console.log('\nDelta is ready: http://127.0.0.1:7710\nOne-use local owner link (expires in 5 minutes):\n'+link);
 if(platform()==='win32')spawn('rundll32.exe',['url.dll,FileProtocolHandler',link],{detached:true,stdio:'ignore'}).unref();
 else if(process.env.DISPLAY||process.env.WAYLAND_DISPLAY)spawn('xdg-open',[link],{detached:true,stdio:'ignore'}).on('error',()=>{}).unref();
 else console.log('Headless device: use an SSH tunnel: ssh -L 7710:127.0.0.1:7710 <host>, then open the one-use link locally.');
}
console.log('Installed at',home);
