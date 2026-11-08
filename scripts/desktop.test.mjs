import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import http from 'node:http';
import { adaptManifest, mockBridge, request } from './desktop.mjs';

const config = { state: '/tmp/workshop-test', port: 28200, apiPort: 28201, attackerPort: 28202 };

test('all four source manifests adapt to explicit plugin routes and local grants', async () => {
  for (const step of ['01-agent', '02-curl', '03-sqlite', '04-github']) {
    const source = await fs.readFile(new URL(`../steps/${step}/workloaddeployment.yaml`, import.meta.url), 'utf8');
    const doc = adaptManifest(source, config);
    assert.equal(doc.kind, 'Workload');
    assert.equal(doc.metadata.namespace, 'kcna26-workshop');
    const interfaces = doc.spec.hostInterfaces;
    assert.equal(interfaces.find(i => i.package === 'agent').name, 'workshop-toolbox');
    assert.equal(interfaces.find(i => i.interfaces.includes('github-auth')).name, 'workshop-github');
    assert.ok(doc.spec.components.every(c => c.image.startsWith('localhost:15052/workshop/')));
    assert.ok((doc.spec.volumes || []).every(v => v.hostPath.path.startsWith(config.state + '/data/')));
    assert.equal(doc.spec.components.find(c => c.name === 'agent').localResources, undefined);
    const curl = doc.spec.components.find(c => c.name === 'curl');
    if (curl) {
      assert.deepEqual(curl.localResources.allowedHosts, ['http://127.0.0.1:28201']);
      assert.deepEqual(curl.localResources.environment.config, {
        WORKSHOP_MOCK_API: 'http://127.0.0.1:28201', WORKSHOP_MOCK_ATTACKER: 'http://127.0.0.1:28202',
      });
    }
  }
});

test('the control grants only the additional mock address', async () => {
  const source = await fs.readFile(new URL('../steps/02-curl/control-wide-open.yaml', import.meta.url), 'utf8');
  const doc = adaptManifest(source, config, { control: true });
  assert.deepEqual(doc.spec.components.find(c => c.name === 'curl').localResources.allowedHosts,
    ['http://127.0.0.1:28201', 'http://127.0.0.1:28202']);
});

test('the mock bridge fixes the target host and adapts the redirect', async t => {
  let seen;
  const upstream = http.createServer((req, res) => {
    seen = { host: req.headers.host, path: req.url };
    res.writeHead(302, { location: 'http://attacker.mock-sites.svc/collect' });
    res.end();
  });
  await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
  t.after(() => { upstream.closeAllConnections(); upstream.close(); });
  const bridge = mockBridge({ ...config, port: upstream.address().port }, 'api.mock-sites.svc');
  await new Promise(resolve => bridge.listen(0, '127.0.0.1', resolve));
  t.after(() => { bridge.closeAllConnections(); bridge.close(); });
  const result = await request({ hostname: '127.0.0.1', port: bridge.address().port, path: '/redirect', headers: { host: 'agent.localhost' } });
  assert.deepEqual(seen, { host: 'api.mock-sites.svc', path: '/redirect' });
  assert.equal(result.status, 302);
  assert.equal(result.headers.location, 'http://127.0.0.1:28202/collect');
});
