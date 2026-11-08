#!/usr/bin/env node
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parse, stringify } from 'yaml';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const pause = ms => new Promise(resolve => setTimeout(resolve, ms).unref());
const steps = ['01-agent', '02-curl', '03-sqlite', '04-github'];
export function settings() {
  const id = crypto.createHash('sha256').update(root).digest('hex').slice(0, 8);
  const port = Number(process.env.DESKTOP_HTTP_PORT || 28190);
  if (!Number.isInteger(port) || port < 1024 || port > 65533) throw Error('DESKTOP_HTTP_PORT must be 1024–65533');
  return {
    state: path.resolve(process.env.DESKTOP_STATE_DIR || path.join(os.tmpdir(), `kcna26-desktop-${id}`)),
    port, apiPort: port + 1, attackerPort: port + 2,
  };
}

export function request(options, body) {
  return new Promise((resolve, reject) => {
    const req = http.request(options, res => {
      let text = '';
      res.setEncoding('utf8');
      res.on('data', chunk => { text += chunk; });
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, text }));
      res.on('error', reject);
    });
    req.setTimeout(120000, () => req.destroy(Error('request timed out')));
    req.on('error', reject);
    req.end(body);
  });
}

async function api(config, route, body, method) {
  const data = body === undefined ? undefined : JSON.stringify(body);
  const response = await request({
    socketPath: path.join(config.state, 'cosmonicd.sock'), path: route,
    method: method || (body === undefined ? 'GET' : 'POST'),
    headers: { 'Content-Type': 'application/json' },
  }, data);
  if (response.status >= 400) throw Error(`${route}: HTTP ${response.status}: ${response.text}`);
  return response.text ? JSON.parse(response.text) : null;
}

function digest(bytes) { return crypto.createHash('sha256').update(bytes).digest('hex'); }
async function checkedFiles(folder) {
  const sums = await fs.readFile(path.join(root, folder, 'SHA256SUMS'), 'utf8');
  const files = new Map();
  for (const line of sums.trim().split('\n')) {
    const match = line.match(/^([a-f0-9]{64})\s+\*?(?:artifacts\/|databases\/)?(.+)$/);
    if (!match) throw Error(`Bad checksum line in ${folder}/SHA256SUMS`);
    const bytes = await fs.readFile(path.join(root, folder, match[2]));
    if (digest(bytes) !== match[1]) throw Error(`Checksum mismatch: ${folder}/${match[2]}`);
    files.set(match[2], bytes);
  }
  return files;
}

async function importArtifacts(config) {
  const layout = path.join(config.state, 'layout');
  const blobs = path.join(layout, 'blobs', 'sha256');
  await fs.mkdir(blobs, { recursive: true });
  async function blob(bytes, mediaType) {
    const hex = digest(bytes);
    await fs.writeFile(path.join(blobs, hex), bytes);
    return { mediaType, digest: `sha256:${hex}`, size: bytes.length };
  }
  const manifests = [];
  for (const [name, bytes] of await checkedFiles('artifacts')) {
    if (!name.endsWith('.wasm')) continue;
    const component = name.slice(0, -5).replaceAll('_', '-');
    const ref = `localhost:15052/workshop/${component}:0.4.0`;
    const configBlob = await blob(Buffer.from('{}'), 'application/vnd.wasm.config.v0+json');
    const layer = await blob(bytes, 'application/wasm');
    const manifest = await blob(Buffer.from(JSON.stringify({
      schemaVersion: 2, mediaType: 'application/vnd.oci.image.manifest.v1+json', config: configBlob, layers: [layer],
    })), 'application/vnd.oci.image.manifest.v1+json');
    manifests.push({ ...manifest, annotations: { 'org.opencontainers.image.ref.name': ref } });
  }
  await fs.writeFile(path.join(layout, 'oci-layout'), JSON.stringify({ imageLayoutVersion: '1.0.0' }));
  await fs.writeFile(path.join(layout, 'index.json'), JSON.stringify({ schemaVersion: 2, manifests }));
  await api(config, '/v1/oci/import', { path: layout });
}

async function stageData(config) {
  const databases = await checkedFiles('databases');
  for (const dir of ['curl-data', 'sqlite-data', 'private-data']) {
    await fs.mkdir(path.join(config.state, 'data', dir), { recursive: true, mode: 0o700 });
  }
  for (const [source, target] of [['talks.db', 'sqlite-data/main.db'], ['rooms.db', 'sqlite-data/rooms.db'], ['payroll.db', 'private-data/payroll.db']]) {
    await fs.writeFile(path.join(config.state, 'data', target), databases.get(source));
  }
}

export function adaptManifest(text, config, { control = false } = {}) {
  const source = parse(text);
  const spec = source.spec.template?.spec || source.spec;
  const workload = { apiVersion: source.apiVersion, kind: 'Workload', metadata: { ...source.metadata, namespace: 'kcna26-workshop' }, spec };
  delete spec.hostSelector;
  for (const entry of spec.hostInterfaces) {
    if (entry.namespace === 'cosmonic' && entry.package === 'agent') entry.name = 'workshop-toolbox';
    if (entry.namespace === 'workshop' && entry.package === 'toolbox') {
      entry.name = entry.interfaces.some(i => i.startsWith('github-')) ? 'workshop-github' : 'workshop-toolbox';
    }
  }
  for (const component of spec.components) {
    component.image = component.image.replace('workshop-toolbox-registry:5000/workshop/', 'localhost:15052/workshop/');
    if (component.name === 'curl') {
      component.localResources.environment = { config: {
        WORKSHOP_MOCK_API: `http://127.0.0.1:${config.apiPort}`,
        WORKSHOP_MOCK_ATTACKER: `http://127.0.0.1:${config.attackerPort}`,
      } };
      component.localResources.allowedHosts = [`http://127.0.0.1:${config.apiPort}`];
      if (control) component.localResources.allowedHosts.push(`http://127.0.0.1:${config.attackerPort}`);
    }
  }
  for (const volume of spec.volumes || []) {
    volume.hostPath.path = path.join(config.state, 'data', path.basename(volume.hostPath.path));
  }
  return workload;
}

async function apply(config, file, options) {
  const workload = adaptManifest(await fs.readFile(path.join(root, file), 'utf8'), config, options);
  const name = workload.metadata.name;
  if (!name) throw Error(`Cannot find workload name: ${file}`);
  // PUT waits for the new revision; POST can leave the old one serving briefly.
  const result = await api(config, `/v1/workloads/kcna26-workshop/${name}`, workload, 'PUT');
  if (result.status?.state !== 'running') throw Error(`${file}: ${JSON.stringify(result.status)}`);
  await fs.mkdir(path.join(root, '.run', 'desktop'), { recursive: true });
  await fs.writeFile(path.join(root, '.run', 'desktop', `${name}.yaml`), stringify(workload));
  return result;
}

export function mockBridge(config, host) {
  return http.createServer((req, res) => {
    // A caller's Host header cannot select another workload through this bridge.
    const upstream = http.request({
      hostname: '127.0.0.1', port: config.port, path: req.url, method: req.method,
      headers: { ...req.headers, host },
    }, response => {
      const headers = { ...response.headers };
      if (headers.location === 'http://attacker.mock-sites.svc/collect') {
        headers.location = `http://127.0.0.1:${config.attackerPort}/collect`;
      }
      res.writeHead(response.statusCode, headers);
      response.pipe(res);
    });
    upstream.setTimeout(30000, () => upstream.destroy(Error('mock upstream timed out')));
    upstream.on('error', () => { if (!res.headersSent) res.writeHead(502); res.end('Mock workload unavailable'); });
    req.on('aborted', () => upstream.destroy());
    req.pipe(upstream);
  });
}

async function listen(server, port) {
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolve); });
}

async function ownedState(config, create = false) {
  const marker = path.join(config.state, 'workshop-owner.json');
  try {
    if (JSON.parse(await fs.readFile(marker, 'utf8')).root !== root) throw Error('State belongs to another checkout');
  } catch (error) {
    if (!create || error.code !== 'ENOENT') throw error;
    const entries = await fs.readdir(config.state).catch(e => { if (e.code === 'ENOENT') return []; throw e; });
    if (entries.length) throw Error(`Refusing to overwrite non-workshop state: ${config.state}`);
    await fs.mkdir(config.state, { recursive: true, mode: 0o700 });
    await fs.writeFile(marker, JSON.stringify({ root }));
  }
}

async function up(config) {
  if (process.platform === 'win32') throw Error('Run this helper in macOS or Linux; Windows needs a named-pipe adapter.');
  const binary = process.env.COSMONICD_BIN;
  if (!binary) throw Error('Set COSMONICD_BIN to the development cosmonicd with the shared tool-type fix. See DESKTOP.md.');
  await ownedState(config, true);
  let alive = false;
  try { await api(config, '/v1/host'); alive = true; } catch {}
  if (alive) throw Error('The workshop daemon is already running; use step, ask, verify, or down.');
  await checkedFiles('artifacts');
  await stageData(config);
  const bridges = [];
  let child;
  let stopping = false;
  async function stop() {
    if (stopping) return;
    stopping = true;
    for (const bridge of bridges) bridge.closeAllConnections();
    for (const bridge of bridges) bridge.close();
    if (child && child.exitCode === null) {
      try { await api(config, '/v1/shutdown', {}); } catch { child.kill('SIGTERM'); }
      await Promise.race([new Promise(resolve => child.once('exit', resolve)), pause(10000)]);
      if (child.exitCode === null) child.kill('SIGTERM');
    }
  }
  const logPath = path.join(config.state, 'daemon.log');
  async function start() {
    if (stopping) throw Error('Workshop startup stopped');
    const log = await fs.open(logPath, 'a', 0o600);
    child = spawn(binary, ['run'], {
      env: { ...process.env, COSMONIC_STATE_DIR: config.state, COSMONIC_HTTP_ADDR: `127.0.0.1:${config.port}` },
      stdio: ['ignore', log.fd, log.fd],
    });
    let spawnError;
    child.once('error', error => { spawnError = error; });
    await log.close();
    for (let i = 0; i < 240; i++) {
      if (spawnError) throw spawnError;
      if (child.exitCode !== null) throw Error(`cosmonicd exited (${child.exitCode}); see ${logPath}`);
      try { await api(config, '/v1/host'); return; } catch {}
      await pause(250);
    }
    throw Error(`Daemon startup timed out; see ${logPath}`);
  }
  process.once('SIGINT', () => { void stop(); });
  process.once('SIGTERM', () => { void stop(); });
  try {
    for (const [port, host] of [[config.apiPort, 'api.mock-sites.svc'], [config.attackerPort, 'attacker.mock-sites.svc']]) {
      const bridge = mockBridge(config, host); bridges.push(bridge); await listen(bridge, port);
    }
    await fs.writeFile(path.join(config.state, 'plugins.yaml'), JSON.stringify({ plugins: [] }));
    await start();
    await api(config, '/v1/plugins', { plugins: [
      { id: 'workshop-toolbox', file: path.join(root, 'artifacts', 'toolbox_plugin.wasm') },
      { id: 'workshop-github', file: path.join(root, 'artifacts', 'github_api.wasm'), allowed_hosts: ['github.com', 'api.github.com'], allowed_ip_name_lookups: ['github.com', 'api.github.com'] },
    ] }, 'PUT');
    const exited = new Promise(resolve => child.once('exit', resolve));
    await api(config, '/v1/shutdown', {});
    await Promise.race([exited, pause(10000)]);
    if (child.exitCode === null) throw Error('Daemon did not stop to apply the plugins');
    await start();
    const plugins = await api(config, '/v1/plugins');
    const failed = plugins.plugins.filter(p => p.state !== 'loaded');
    if (failed.length) throw Error(`Workshop plugins did not load: ${JSON.stringify(failed)}. See DESKTOP.md.`);
    await importArtifacts(config);
    await apply(config, 'mock-sites/workloaddeployment.yaml');
    await apply(config, `steps/${steps[0]}/workloaddeployment.yaml`);
    console.log(`Desktop workshop ready: http://agent.localhost:${config.port}`);
    console.log(`Mock API: http://127.0.0.1:${config.apiPort}/data`);
    console.log(`State and logs: ${config.state}`);
    console.log('In another terminal: make desktop-step STEP=2; make desktop-verify');
    console.log('Ctrl-C or make desktop-down stops only this workshop daemon.');
    await new Promise(resolve => child.once('exit', resolve));
  } finally { await stop(); }
}

async function ask(config, prompt) {
  const result = await request({ hostname: '127.0.0.1', port: config.port, path: '/', method: 'POST', headers: { host: 'agent.localhost' } }, prompt);
  if (result.status !== 200) throw Error(`Agent answered HTTP ${result.status}: ${result.text}`);
  return result.text;
}

async function verify(config, live) {
  const results = [];
  async function check(label, prompt, expected) {
    const text = await ask(config, prompt);
    if (!expected.test(text)) throw Error(`${label}: expected ${expected}\n${text}`);
    results.push({ label, prompt, text });
    console.log(`PASS ${label}`);
  }
  const apiUrl = `http://127.0.0.1:${config.apiPort}`;
  const attacker = `http://127.0.0.1:${config.attackerPort}/collect`;
  await apply(config, `steps/${steps[0]}/workloaddeployment.yaml`);
  await check('agent greeting', 'hello', /Hello! I am the workshop agent/);
  await check('no tools before a grant', `fetch ${apiUrl}/data`, /\[tool curl\] error:.*(?:no tool named|unknown-tool)/);
  await apply(config, `steps/${steps[1]}/workloaddeployment.yaml`);
  await check('allowed API', `fetch ${apiUrl}/data`, /\[tool curl\] ok:.*"service":"releases"/);
  await check('browser example uses the cluster-style address', 'fetch http://api.mock-sites.svc/data', /\[tool curl\] ok:.*"service":"releases"/);
  await check('save inside the mounted folder', `save ${apiUrl}/data to page.html`, /\[tool curl\] ok:/);
  await check('read the saved file', 'curl -s file:///data/page.html', /\[tool curl\] ok:.*"service":"releases"/);
  await check('metadata refused by the grant', 'curl -s http://169.254.169.254/latest/meta-data/', /"access-denied":.*allowedHosts/);
  await check('attacker refused by the grant', `curl -s ${attacker}`, /"access-denied":.*allowedHosts/);
  await check('cluster-style attacker address is also refused', 'curl -s http://attacker.mock-sites.svc/collect', /"access-denied":.*allowedHosts/);
  await check('redirect destination refused', `curl -sL ${apiUrl}/redirect`, /"access-denied":.*allowedHosts/);
  await check('file read outside the mount refused', 'curl -s file:///etc/passwd', /"access-denied":/);
  await check('file write outside the mount refused', `curl -s -o /etc/evil ${apiUrl}/data`, /"access-denied":/);
  try {
    await apply(config, 'steps/02-curl/control-wide-open.yaml', { control: true });
    await check('control: attacker is reachable when granted', `curl -s ${attacker}`, /\[tool curl\] ok: attacker: got your request/);
  } finally { await apply(config, `steps/${steps[1]}/workloaddeployment.yaml`); }
  await check('control reverted', `curl -s ${attacker}`, /"access-denied":.*allowedHosts/);
  await apply(config, `steps/${steps[2]}/workloaddeployment.yaml`);
  await check('SQLite reads talks', "sqlite select title, speaker from talks where track = 'Security' order by title", /\[tool sqlite\] ok:.*Least Privilege/);
  await check('SQLite lists tables', 'sqlite .tables', /\[tool sqlite\] ok:.*talks/);
  await check('SQLite attaches a granted database', "sqlite attach 'file:/data/rooms.db?immutable=1' as r; select name, seats from r.rooms", /\[tool sqlite\] ok:.*Hall A \| 900/);
  await check('SQLite refuses a host file outside the mount', `sqlite attach 'file:${config.state}/data/private-data/payroll.db?immutable=1' as p; select name, salary from p.salaries`, /"access-denied":/);
  await check('SQLite refuses shell escape', 'sqlite .shell id', /"invalid-argument":"\.shell: there is no shell here/);
  await check('SQLite refuses extensions', "sqlite select load_extension('/tmp/evil')", /"invalid-argument":"no such function: load_extension/);
  await apply(config, `steps/${steps[3]}/workloaddeployment.yaml`);
  await check('GitHub API is not granted to curl', 'curl -s https://api.github.com/repos/wasmCloud/wasmCloud/pulls', /"access-denied":.*allowedHosts/);
  await check('GitHub capability refuses an unlisted path', 'gh api /user', /\[tool gh\] error:.*(?:refused|not allowed)/);
  const status = await request({ hostname: '127.0.0.1', port: config.port, path: '/github/status', headers: { host: 'agent.localhost' } });
  if (status.status !== 200 || JSON.parse(status.text).state !== 'signed-out') throw Error(`Unexpected GitHub status: ${status.text}`);
  console.log('PASS GitHub capability reports signed out');
  if (live) await check('live GitHub pull request read', 'gh pr list -R wasmCloud/wasmCloud', /\[tool gh\] ok:/);
  await fs.mkdir(path.join(root, '.run', 'desktop'), { recursive: true });
  await fs.writeFile(path.join(root, '.run', 'desktop', 'verification.json'), JSON.stringify({ results, live }, null, 2));
  console.log(`Desktop verification passed: ${results.length + 1} checks. ${live ? 'Live GitHub read included.' : 'Live GitHub read skipped; pass --live to include it.'}`);
}

export async function main(args) {
  const config = settings();
  const [command, ...rest] = args;
  if (command === 'up') return up(config);
  await ownedState(config);
  if (command === 'down') { await api(config, '/v1/shutdown', {}); return; }
  if (command === 'ask') { console.log(await ask(config, rest.join(' '))); return; }
  if (command === 'verify') return verify(config, rest.includes('--live'));
  if (command === 'step' && /^[1-4]$/.test(rest[0])) {
    await apply(config, `steps/${steps[Number(rest[0]) - 1]}/workloaddeployment.yaml`);
    console.log(`Step ${rest[0]} ready: http://agent.localhost:${config.port}`); return;
  }
  throw Error('Usage: node scripts/desktop.mjs up | step 1–4 | ask "prompt" | verify [--live] | down');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
