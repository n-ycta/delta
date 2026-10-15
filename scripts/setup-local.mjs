// Run AFTER starting the core. Secrets stay in untracked 0600 .env files.
import {readFile,writeFile,mkdir} from 'node:fs/promises';import {resolve,dirname} from 'node:path';import {homedir} from 'node:os';
const root=resolve(import.meta.dirname,'../..'),url=process.env.DELTA_URL||'http://127.0.0.1:7700';
const home=process.env.DELTA_HOME||resolve(homedir(),'.delta');const token=process.env.DELTA_TOKEN||(await readFile(resolve(home,'admin.token'),'utf8')).trim();
// DATABASE_URL is optional; omitted means the file quick-start backend.
async function call(capability,args={}){const r=await fetch(url+'/v1/call',{method:'POST',headers:{authorization:'Bearer '+token,'content-type':'application/json'},body:JSON.stringify({capability,args})});const d=await r.json();if(!d.ok)throw Error(d.error.code+': '+d.error.message);return d.result}
const definitions=[['delta_db','storage','storage',['register:storage','call:storage.*']],['delta_type','typing','dev.delta.type',['register:typing','call:storage.write']],['delta-web','web','web-service',['admin']]];
for(const [repo,id,name,scopes] of definitions){const directory=resolve(root,repo);const env=`DELTA_URL=${url}\nDELTA_TOKEN=\nSTORAGE_COMPONENT=storage\n`+(id==='storage'?`DATABASE_URL=${process.env.DATABASE_URL||''}\n`:id==='web'?'VITE_CORE_MODE=http\nVITE_CORE_URL=/api\nHOST=127.0.0.1\nPORT=7710\n':'DATA_DIR='+resolve(directory,'data')+'\nPORT=7720\n');await writeFile(resolve(directory,'.env'),env,{mode:0o600});
 // Supervisor has no cwd field. Launch Node through an absolute bootstrap that chdirs.
 const bootstrap=resolve(directory,'server/launch.mjs');await writeFile(bootstrap,`process.chdir(${JSON.stringify(directory)});process.loadEnvFile('.env');await import('./main.mjs');\n`);
 await call('supervisor.add',{id,command:[process.execPath,bootstrap],scopes,token_name:name});console.log(repo,'registered with supervisor');}
const user=await call('tokens.create',{name:'local-console',scopes:['admin']});await writeFile(resolve(root,'console-user.token'),user.token+'\n',{mode:0o600});
const trainer=await call('tokens.create',{name:'local-trainer',scopes:['call:typing.ingest']});await writeFile(resolve(root,'trainer-user.token'),trainer.token+'\n',{mode:0o600});
console.log('User tokens saved outside repositories to console-user.token and trainer-user.token. Never commit them.');
