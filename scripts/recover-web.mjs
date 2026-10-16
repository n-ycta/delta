// Local-owner recovery ONLY. No init, install, profile reset or credential disclosure.
import { readFile, open, rename, rm, realpath } from 'node:fs/promises';
import { resolve, join, isAbsolute } from 'node:path';
import { homedir, platform } from 'node:os';
import { randomBytes, createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

export async function recoverWeb({ home, webData, origin, output = console.log } = {}) {
  home = resolve(home || process.env.DELTA_HOME || (platform() === 'win32' ? join(process.env.LOCALAPPDATA || homedir(), 'Delta') : join(homedir(), '.delta')));
  // Missing config/admin.token must fail, never initialise a replacement node.
  const config = JSON.parse(await readFile(join(home, 'config.json'), 'utf8'));
  if (!config.node_id || !Number.isInteger(config.port) || config.port < 1 || config.port > 65535) throw Error('Invalid existing node configuration. Nothing was changed.');
  const admin = (await readFile(join(home, 'admin.token'), 'utf8')).trim();
  if (!admin) throw Error('Local owner credential is missing. Nothing was changed.');
  const call = async (capability) => {
    const r = await fetch(`http://127.0.0.1:${config.port}/v1/call`, { method: 'POST', headers: { authorization: 'Bearer ' + admin, 'content-type': 'application/json' }, body: JSON.stringify({ capability, args: {} }), signal: AbortSignal.timeout(8000), redirect: 'error' });
    const d = await r.json();
    if (!r.ok || !d.ok) throw Error('Local owner access was rejected. Nothing was changed.');
    return d.result;
  };
  const info = await call('core.info');
  if (!info.principal?.scopes?.includes('admin')) throw Error('Local owner access was rejected. Nothing was changed.');
  if (info.node_id !== config.node_id) throw Error('The running node does not match this DELTA_HOME. Nothing was changed.');
  // An explicit admin-only request prevents issuing a link with a scoped or obsolete token.
  const keys = await call('identity.list');
  const components = config.components || [];
  const web = components.find(c => c.id === 'web');
  if (!web && !webData) throw Error('Web installation not found. Specify its existing absolute --web-data directory; do not create a new profile.');
  const env = web?.env || {};
  const configuredData = webData || env.DATA_DIR || env.DELTA_DATA || join(home, 'data', 'web');
  if (!isAbsolute(configuredData)) throw Error('Web data path is relative. Supply the existing absolute --web-data directory.');
  // Never invent a new Web data directory when recovering an existing installation.
  const dir = await realpath(configuredData);
  const publicOrigin = new URL(origin || env.PUBLIC_ORIGIN || `http://127.0.0.1:${env.PORT || 7710}`);
  if (publicOrigin.username || publicOrigin.password || publicOrigin.pathname !== '/' || publicOrigin.search || publicOrigin.hash || !['http:', 'https:'].includes(publicOrigin.protocol) || (publicOrigin.protocol !== 'https:' && !['127.0.0.1', 'localhost', '[::1]'].includes(publicOrigin.hostname))) throw Error('Use an HTTPS Console origin, or a localhost SSH tunnel. Nothing was changed.');
  const code = randomBytes(32).toString('hex');
  const path = join(dir, 'bootstrap.json'), temp = join(dir, '.recovery-' + randomBytes(12).toString('hex'));
  try {
    const file = await open(temp, 'wx', 0o600);
    try { await file.writeFile(JSON.stringify({ hash: createHash('sha256').update(code).digest('hex'), expires: Date.now() + 300000 })); await file.sync(); } finally { await file.close(); }
    await rename(temp, path);
  } finally { await rm(temp, { force: true }); }
  output(`Existing node: ${config.name || 'Delta'} (${config.node_id}). Plugins, settings and keys were not changed.`);
  output('Private one-use recovery link (5 minutes). Do not share it:');
  output(publicOrigin.origin + '/#/identity?code=' + code);
  output('Open the link, create or import your saved key, then register it. Registration does not create another node.');
  if (!config.identity_origins?.includes(publicOrigin.origin)) output('Warning: this Console origin is not in identity_origins. Use an allowed local origin/tunnel for signed login, or configure the verified HTTPS origin on this same node.');
  return { nodeId: config.node_id, enrolledKeys: keys.length, dataDir: dir };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const args = process.argv.slice(2), opts = {};
    for (let i = 0; i < args.length; i += 2) {
      const key = { '--home': 'home', '--web-data': 'webData', '--origin': 'origin' }[args[i]];
      if (!key || !args[i + 1]) throw Error('Usage: node recover-web.mjs [--home PATH] [--web-data PATH] [--origin https://console.example]');
      opts[key] = args[i + 1];
    }
    await recoverWeb(opts);
  } catch (e) { console.error('Recovery stopped:', e.code === 'ENOENT' ? 'Existing Delta files were not found. Run as the original OS user with the original DELTA_HOME. No new node was created.' : e.message); process.exitCode = 1; }
}
