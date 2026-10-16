import {test} from 'node:test';import assert from 'node:assert/strict';import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';import {tmpdir} from 'node:os';import {resolve} from 'node:path';import {execFile} from 'node:child_process';import {promisify} from 'node:util';import {createServer} from 'node:http';
const exec=promisify(execFile),root=resolve(import.meta.dirname,'..');
test('bootstrap connects to existing configured port, not hardcoded 7700',async()=>{const home=await mkdtemp(tmpdir()+'/delta-port-');const server=createServer(async(req,res)=>{let b='';for await(const c of req)b+=c;assert.equal(JSON.parse(b).capability,'core.info');res.setHeader('content-type','application/json');res.end(JSON.stringify({ok:true,result:{}}));});await new Promise(r=>server.listen(0,'127.0.0.1',r));try{await writeFile(home+'/config.json',JSON.stringify({port:server.address().port}));await writeFile(home+'/admin.token','test-only');const {stdout}=await exec(process.execPath,[root+'/scripts/bootstrap.mjs','--plugins','none','--no-service'],{env:{...process.env,DELTA_HOME:home}});assert(stdout.includes('Installed at'));}finally{server.closeAllConnections();await new Promise(r=>server.close(r));await rm(home,{recursive:true,force:true});}});
test('installer does not reuse stale bootstrap, but reuses same verified release',async()=>{const home=await mkdtemp(tmpdir()+'/delta-cache-');try{for(const d of ['bin','runtime/bin','fake'])await mkdir(home+'/'+d,{recursive:true});for(const f of ['bin/delta','runtime/bin/node'])await writeFile(home+'/'+f,'#!/bin/sh\necho CACHED_RUNTIME\n',{mode:0o700});await writeFile(home+'/bin/bootstrap.mjs','');await writeFile(home+'/fake/curl','#!/bin/sh\nexit 73\n',{mode:0o700});const env={...process.env,DELTA_HOME:home,PATH:home+'/fake:'+process.env.PATH,DELTA_VERSION:'v-test'};await assert.rejects(exec('sh',[root+'/install.sh'],{env}),e=>e.code===73);await writeFile(home+'/bin/release-version','v-test\n');assert((await exec('sh',[root+'/install.sh'],{env})).stdout.includes('CACHED_RUNTIME'));}finally{await rm(home,{recursive:true,force:true});}});

test('rerun repairs autostart even if the core is already online',async()=>{
 const home=await mkdtemp(tmpdir()+'/delta-autostart-');
 const server=createServer(async(req,res)=>{for await(const _ of req){}res.setHeader('content-type','application/json');res.end(JSON.stringify({ok:true,result:{version:'0.4.0'}}))});await new Promise(r=>server.listen(0,'127.0.0.1',r));
 try{await mkdir(home+'/bin',{recursive:true});await mkdir(home+'/fake',{recursive:true});await writeFile(home+'/config.json',JSON.stringify({port:server.address().port}));await writeFile(home+'/admin.token','fixture');
  await writeFile(home+'/bin/delta',`#!/bin/sh\nif [ "$1" = --version ]; then echo delta 0.4.0; exit 0; fi\nprintf '%s\\n' "$*" >> "$DELTA_HOME/commands"\nexit 0\n`,{mode:0o700});
  await writeFile(home+'/fake/systemctl','#!/bin/sh\nexit 0\n',{mode:0o700});
  await exec(process.execPath,[root+'/scripts/bootstrap.mjs','--plugins','none'],{env:{...process.env,DELTA_HOME:home,PATH:home+'/fake:'+process.env.PATH}});
  const {readFile}=await import('node:fs/promises');assert.match(await readFile(home+'/commands','utf8'),/service install/);
 }finally{server.closeAllConnections();await new Promise(r=>server.close(r));await rm(home,{recursive:true,force:true});}
});

test('failed service installation must not silently fall back to a temporary node',async()=>{
 const home=await mkdtemp(tmpdir()+'/delta-service-fail-');try{
  await mkdir(home+'/bin',{recursive:true});await writeFile(home+'/config.json',JSON.stringify({port:1}));await writeFile(home+'/admin.token','fixture');
  await writeFile(home+'/bin/delta',`#!/bin/sh\nif [ "$1" = --version ]; then echo delta 0.4.0; exit 0; fi\nif [ "$1" = run ]; then touch "$DELTA_HOME/unexpected-background-node"; fi\nexit 1\n`,{mode:0o700});
  await assert.rejects(exec(process.execPath,[root+'/scripts/bootstrap.mjs','--plugins','none'],{env:{...process.env,DELTA_HOME:home}}),e=>/Could not enable Delta autostart/.test(e.stderr));
  const {access}=await import('node:fs/promises');await assert.rejects(access(home+'/unexpected-background-node'));
 }finally{await rm(home,{recursive:true,force:true});}
});

test('bootstrap resolves arbitrary catalog components and does not silently add Storage',async()=>{
 const home=await mkdtemp(tmpdir()+'/delta-dynamic-plugin-'),calls=[];
 const item={id:'dev.example.sensor',component:'sensor',version:'1.0.0',sha256:'fixture',manifest:{dependencies:[]}};
 const server=createServer(async(req,res)=>{let b='';for await(const c of req)b+=c;const q=JSON.parse(b);calls.push(q);const values={'core.info':{},'packages.catalog':{plugins:[item]},'core.components':[],'packages.plan':{items:[item],warnings:[]},'packages.install':{jobId:'j'},'packages.jobs':[{id:'j',status:'complete'}]};res.setHeader('content-type','application/json');res.end(JSON.stringify({ok:true,result:values[q.capability]}))});await new Promise(r=>server.listen(0,'127.0.0.1',r));try{await writeFile(home+'/config.json',JSON.stringify({port:server.address().port}));await writeFile(home+'/admin.token','fixture');await exec(process.execPath,[root+'/scripts/bootstrap.mjs','--plugins','sensor','--no-service'],{env:{...process.env,DELTA_HOME:home}});assert.equal(calls.find(c=>c.capability==='packages.install').args.id,item.id);assert(!calls.some(c=>c.args.id==='dev.delta.db'));}finally{server.closeAllConnections();await new Promise(r=>server.close(r));await rm(home,{recursive:true,force:true})}
});
