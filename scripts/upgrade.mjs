// Stop only a locally owned Linux node whose executable AND data home match.
// Never kill an arbitrary process just because it occupies the configured port.
import {readdir,readFile,readlink,realpath} from 'node:fs/promises';
import {resolve} from 'node:path';import {homedir} from 'node:os';import {spawnSync} from 'node:child_process';
export async function stopPreviousNode({home,binary,online}){
 if(process.platform!=='linux')throw Error('An older Delta node is running. Stop it, then rerun the installer. Data is preserved.');
 const expected=await realpath(binary),candidates=[];
 for(const name of await readdir('/proc')){
  if(!/^\d+$/.test(name)||Number(name)===process.pid)continue;
  try{
   const exe=(await readlink(`/proc/${name}/exe`)).replace(/ \(deleted\)$/,'');
   if(exe!==expected)continue;
   const env=(await readFile(`/proc/${name}/environ`,'utf8')).split('\0');
   const entry=env.find(x=>x.startsWith('DELTA_HOME='));
   if(resolve(entry?entry.slice(11):resolve(homedir(),'.delta'))!==resolve(home))continue;
   const args=(await readFile(`/proc/${name}/cmdline`,'utf8')).split('\0');
   if(!args.includes('run'))continue;candidates.push(Number(name));
  }catch{/* Not ours or no access: never signal it. */}
 }
 if(candidates.length!==1)throw Error('Cannot safely identify the old Delta node. Stop it manually, then rerun the installer. Data is preserved.');
 const pid=candidates[0],unit=spawnSync('systemctl',['--user','show','delta.service','--property=MainPID','--value'],{encoding:'utf8'});
 if(unit.status===0&&Number(unit.stdout.trim())===pid){const stopped=spawnSync('systemctl',['--user','stop','delta.service'],{stdio:'inherit'});if(stopped.status!==0)throw Error('Could not stop delta.service; stop it manually before updating.');}
 else process.kill(pid,'SIGTERM');
 for(let i=0;i<150;i++){if(!await online())return;await new Promise(r=>setTimeout(r,200));}
 throw Error('Old Delta node has not stopped. Stop it manually before updating; no forced kill was attempted.');
}
