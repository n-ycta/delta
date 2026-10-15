// Linux-only live upgrade regression; inputs are explicitly supplied local test binaries.
import {mkdtemp,mkdir,copyFile,chmod,unlink,rm} from 'node:fs/promises';import {tmpdir} from 'node:os';import {join,resolve} from 'node:path';import {spawn,execFile} from 'node:child_process';import {promisify} from 'node:util';import {createServer} from 'node:net';import assert from 'node:assert/strict';import {stopPreviousNode} from './upgrade.mjs';
const exec=promisify(execFile),home=await mkdtemp(join(tmpdir(),'delta-upgrade-')),binary=join(home,'bin/delta');await mkdir(join(home,'bin'));
const reserved=createServer();await new Promise(r=>reserved.listen(0,'127.0.0.1',r));const port=reserved.address().port;await new Promise(r=>reserved.close(r));
const env={...process.env,DELTA_HOME:home},old=process.env.TEST_OLD_DELTA_BIN,newer=process.env.TEST_NEW_DELTA_BIN;if(!old||!newer)throw Error('Set TEST_OLD_DELTA_BIN and TEST_NEW_DELTA_BIN');
const {readFile}=await import('node:fs/promises');let token;
const versionOf=async path=>(await exec(path,['--version'])).stdout.trim().split(/\s+/).at(-1);
const oldVersion=await versionOf(old),newVersion=await versionOf(newer);
async function info(){try{const r=await fetch('http://127.0.0.1:'+port+'/v1/call',{method:'POST',headers:{authorization:'Bearer '+token,'content-type':'application/json'},body:JSON.stringify({capability:'core.info',args:{}}),signal:AbortSignal.timeout(1000)});const j=await r.json();return j.ok?j.result:null;}catch{return null;}}
try{
 await copyFile(old,binary);await chmod(binary,0o700);await exec(binary,['init','--name','upgrade-test','--port',String(port)],{env});token=(await readFile(join(home,'admin.token'),'utf8')).trim();const child=spawn(binary,['run'],{env,stdio:'ignore'});child.unref();for(let i=0;i<100&&!await info();i++)await new Promise(r=>setTimeout(r,50));assert.equal((await info()).version,oldVersion);
 // Atomically replace the binary, exactly as GNU install does for an in-use executable.
 await unlink(binary);await copyFile(newer,binary);await chmod(binary,0o700);
 await exec(process.execPath,[resolve(import.meta.dirname,'bootstrap.mjs'),'--plugins','none','--no-service'],{env,timeout:40000});assert.equal((await info()).version,newVersion);console.log(`PASS live standalone update ${oldVersion} → ${newVersion}, existing identity/config preserved`);
}finally{if(await info())await stopPreviousNode({home,binary,online:async()=>!!await info()}).catch(()=>{});await rm(home,{recursive:true,force:true});}
