import {readFile} from 'node:fs/promises';import {resolve} from 'node:path';import assert from 'node:assert/strict';import {randomUUID} from 'node:crypto';
const token=(await readFile(resolve(process.env.DELTA_HOME,'admin.token'),'utf8')).trim();
async function call(capability,args={}){const r=await fetch('http://127.0.0.1:7700/v1/call',{method:'POST',headers:{authorization:'Bearer '+token,'content-type':'application/json'},body:JSON.stringify({capability,args})});const d=await r.json();assert(d.ok,d.error?.message);return d.result;}
const components=await call('core.components');for(const id of ['storage','web','typing','mcp','updater','provision','observability'])assert(components.some(c=>c.id===id),'Missing '+id);
assert.equal((await call('storage.health')).engine,'file');const now=Date.now();await call('typing.ingest',{session:{id:randomUUID(),startedAt:now-60000,endedAt:now,language:'en',characters:300,corrections:2,node:'install-test'}});await call('typing.flush');assert((await call('web.plugins_list')).filter(p=>p.instance.status==='active').length>=3);assert.equal((await fetch('http://127.0.0.1:7710')).status,200);
assert.equal((await call('mcp.info')).listen.port,7730);
assert.equal((await call('typing.status')).trainer,'listening');
assert((await call('dependencies.status',{id:'dev.delta.web'})).dependencies.every(x=>x.state==='ready'));
assert((await call('updater.status')).settings);assert((await call('provision.preflight')).core.supported);
const profile=await call('mcp.create',{profile:{name:'installation-smoke',capabilities:['system.info']}});
try{for(const port of[7710,7730]){const r=await fetch('http://127.0.0.1:'+port+'/mcp/'+profile.profile.id,{method:'POST',headers:{authorization:'Bearer '+profile.secret},body:JSON.stringify({jsonrpc:'2.0',id:1,method:'initialize'})});assert.equal(r.status,200);assert.equal((await r.json()).result.protocolVersion,'2025-06-18')}}finally{await call('mcp.revoke',{id:profile.profile.id})}
console.log('PASS prebuilt Core, seven plugin releases, file storage, typing, dependency gates and direct/proxied MCP HTTP');

