import assert from 'node:assert/strict';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Miniflare, convertV4MiniflareOptions } from 'miniflare';

// Run worker-build --release in crates/worker before this test.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const password = 'adaptive-reporting-test-admin';
const admin = { Authorization: `Bearer ${password}` };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function within(promise, label, timeout = 5000) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Timed out: ${label}`)), timeout);
    })]);
  } finally {
    clearTimeout(timer);
  }
}

async function poll(check, label, timeout = 5000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = await check();
    if (value) return value;
    await sleep(20);
  }
  throw new Error(`Timed out: ${label}`);
}

// Extra inspection stays in the temporary test Worker, never in the deployed bundle.
const inspectionWorker = `
import Worker, { Hub as BaseHub } from './base-worker.mjs';
export default Worker;
export class Hub extends BaseHub {
  async fetch(req) {
    const path = new URL(req.url).pathname;
    if (!path.startsWith('/__test/adaptive-')) return super.fetch(req);
    const input = await req.json();
    const dashboards = () => ['dash', 'public'].flatMap(tag => this.state.getWebSockets(tag))
      .filter(ws => !ws.deserializeAttachment().closing);
    if (path === '/__test/adaptive-dashes') return Response.json(dashboards().length);
    if (path === '/__test/adaptive-error') {
      const ws = dashboards()[0];
      if (!ws) return new Response('no dashboard', { status: 404 });
      await this.inner.webSocketError(ws, new Error('adaptive test injected error'));
      return Response.json('ok');
    }
    if (path === '/__test/adaptive-agent') {
      const ws = this.state.getWebSockets('server:' + input.id)
        .find(ws => !ws.deserializeAttachment().closing);
      if (!ws) return Response.json(null);
      const attachment = ws.deserializeAttachment();
      return Response.json(attachment);
    }
    return new Response('not found', { status: 404 });
  }
}
`;

async function harness(idle) {
  const directory = await mkdtemp(join(tmpdir(), 'moni-adaptive-'));
  let mf;
  const peers = [];
  try {
    await cp(join(root, 'crates/worker/build'), join(directory, 'build'), { recursive: true });
    await writeFile(join(directory, 'base-worker.mjs'),
      await readFile(join(root, 'scripts/e2e-worker.mjs'), 'utf8'));
    await writeFile(join(directory, 'worker.mjs'), inspectionWorker);
    mf = new Miniflare(convertV4MiniflareOptions({
      modules: [
        { type: 'ESModule', path: join(directory, 'worker.mjs') },
        { type: 'ESModule', path: join(directory, 'base-worker.mjs') },
        { type: 'ESModule', path: join(directory, 'build/index.js') },
        { type: 'CompiledWasm', path: join(directory, 'build/index_bg.wasm') },
      ],
      modulesRoot: directory,
      rootPath: directory,
      compatibilityDate: '2026-10-01',
      durableObjects: { HUB: { className: 'Hub', useSQLite: true } },
      durableObjectsPersist: join(directory, 'state'),
      bindings: {
        ADMIN_PASSWORD: password,
        AUTH_SECRET: 'adaptive-reporting-test-secret-at-least-32-bytes',
        PUBLIC_DASHBOARD: 'true',
        REPORT_INTERVAL_S: '2',
        REPORT_IDLE_INTERVAL_S: String(idle),
        RETENTION_MINUTE_DAYS: '7',
        RETENTION_HOUR_DAYS: '90',
        E2E_RUN_ID: 'adaptive-reporting',
      },
    }));
    await within(mf.ready, 'Miniflare startup', 20_000);
  } catch (error) {
    await mf?.dispose();
    await rm(directory, { recursive: true, force: true });
    throw error;
  }

  async function api(path, expected = 200, options = {}) {
    const { body, headers, ...rest } = options;
    const response = await within(mf.dispatchFetch('https://adaptive.test' + path, {
      ...rest,
      headers: { ...(body === undefined ? {} : { 'Content-Type': 'application/json' }), ...headers },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    }), `${options.method ?? 'GET'} ${path}`);
    assert.equal(response.status, expected, `${options.method ?? 'GET'} ${path}`);
    return response;
  }
  const test = (path, body = {}) => api('/__test/' + path, 200,
    { method: 'POST', body }).then(response => response.json());
  async function socket(path, headers = {}) {
    const response = await api(path, 101, { headers: { Upgrade: 'websocket', ...headers } });
    const ws = response.webSocket;
    assert.ok(ws, `${path} returned a WebSocket`);
    const peer = { ws, messages: [], closed: null };
    // accept() can immediately deliver Welcome/Snapshot queued by the server.
    ws.addEventListener('message', event => peer.messages.push(JSON.parse(event.data)));
    ws.addEventListener('close', event => { peer.closed = event.code; });
    ws.addEventListener('error', () => {});
    peers.push(peer);
    ws.accept();
    return peer;
  }
  async function event(peer, type, predicate = () => true) {
    return poll(() => {
      const index = peer.messages.findIndex(message => message.type === type && predicate(message));
      if (index >= 0) return peer.messages.splice(index, 1)[0];
      assert.equal(peer.closed, null, `Socket closed before ${type}`);
    }, type);
  }
  async function interval(peer, seconds) {
    const message = await event(peer, 'set_report_interval');
    assert.equal(message.report_interval_s, seconds, 'requested report interval');
  }
  async function noInterval(peer) {
    await test('stats');
    await sleep(60);
    assert.equal(peer.messages.filter(message => message.type === 'set_report_interval').length, 0,
      'No duplicate/unsupported interval command');
  }
  async function dashboard(authenticated = true) {
    const peer = await socket('/ws/dashboard', authenticated ? admin : {});
    await event(peer, 'snapshot');
    return peer;
  }
  async function close(peer) {
    if (peer.closed === null) peer.ws.close(1000, 'test complete');
    await poll(() => peer.closed !== null, 'WebSocket close');
  }
  let version = 0;
  async function hello(peer) {
    const agent_version = `adaptive-test-${++version}`;
    peer.ws.send(JSON.stringify({ type: 'hello', protocol: 3, agent_version,
      host: { hostname: 'fixture', os: 'Linux', kernel: 'test', arch: 'x86_64',
        cpu_model: 'fixture', cpu_cores: 1, boot_time_ms: 0 },
    }));
    await poll(async () => (await test('stats')).last.message?.agent_version === agent_version,
      'Hello processed');
    peer.welcome = await event(peer, 'welcome');
  }
  async function agent(id, sendHello = true) {
    const created = await (await api('/api/servers', 201,
      { method: 'POST', headers: admin, body: { id, name: id } })).json();
    const peer = await socket('/ws/agent', { Authorization: `Bearer ${created.token}` });
    peer.id = id;
    if (sendHello) await hello(peer);
    return peer;
  }
  const attachment = peer => test('adaptive-agent', { id: peer.id });
  const patch = (peer, patch) => test('attachment', { id: peer.id, patch });
  async function alarm() {
    assert.equal((await test('alarm')).ok, true, 'Alarm succeeds');
  }
  async function online(peer, expected) {
    const servers = await (await api('/api/servers', 200, { headers: admin })).json();
    assert.equal(servers.find(server => server.id === peer.id)?.online, expected, `${peer.id} online`);
  }
  return { api, test, agent, hello, event, interval, noInterval, dashboard, close,
    attachment, patch, alarm, online,
    async dispose() {
      for (const peer of peers) {
        if (peer.closed === null) {
          try { peer.ws.close(1000, 'test cleanup'); } catch {}
        }
      }
      try { await mf.dispose(); }
      finally { await rm(directory, { recursive: true, force: true }); }
    },
  };
}

async function defaults() {
  const h = await harness(30);
  try {
    const agent = await h.agent('adaptive');
    assert.equal(agent.welcome.report_interval_s, 30);
    const lastSeen = (await h.attachment(agent)).last_seen_ms;
    const first = await h.dashboard();
    await h.interval(agent, 2);
    const second = await h.dashboard();
    const publicDash = await h.dashboard(false);
    await h.noInterval(agent);
    await h.close(first);
    await h.noInterval(agent);
    await h.close(second);
    await h.noInterval(agent);
    await h.close(publicDash);
    await h.interval(agent, 30);
    assert.equal((await h.attachment(agent)).last_seen_ms, lastSeen,
      'Audience changes must not manufacture an agent heartbeat');
    console.log('PASS active=2s / idle=30s, admin/public audience, multiple windows, last close');

    const publicFirst = await h.dashboard(false);
    await h.interval(agent, 2);
    await h.test('recreate');
    const afterRecreate = await h.dashboard();
    await h.noInterval(agent);
    await h.close(publicFirst);
    await h.noInterval(agent);
    await h.close(afterRecreate);
    await h.interval(agent, 30);
    const errored = await h.dashboard(false);
    await h.interval(agent, 2);
    await h.test('adaptive-error');
    await h.interval(agent, 30);
    // Directly invoking the DO error callback does not complete Miniflare's client handshake.
    errored.ws.close(1000, 'test error cleanup');
    assert.equal(await h.test('adaptive-dashes'), 0);
    console.log('PASS recreation and Dashboard error');

    await h.test('faults', { sql: 'SELECT * FROM servers ORDER BY name, id' });
    await h.api('/ws/dashboard', 500, { headers: { Upgrade: 'websocket', ...admin } });
    assert.equal(await h.test('adaptive-dashes'), 0, 'Failed snapshot leaves no dashboard');
    await h.noInterval(agent);
    console.log('PASS failed snapshot cleanup');

    const lateIdle = await h.agent('late-idle', false);
    const transient = await h.dashboard();
    await h.interval(agent, 2);
    await h.noInterval(lateIdle);
    await h.close(transient);
    await h.interval(agent, 30);
    await h.hello(lateIdle);
    assert.equal(lateIdle.welcome.report_interval_s, 30);
    await h.close(lateIdle);
    const lateActive = await h.agent('late-active', false);
    const current = await h.dashboard(false);
    await h.interval(agent, 2);
    await h.hello(lateActive);
    assert.equal(lateActive.welcome.report_interval_s, 2);
    await h.noInterval(lateActive);
    await h.close(current);
    await h.interval(agent, 30);
    await h.interval(lateActive, 30);
    await h.close(lateActive);
    console.log('PASS late Hello recomputes the current audience');

    const staleAtSwitch = Date.now() - 14_000;
    await h.patch(agent, { last_seen_ms: staleAtSwitch, report_grace_until_ms: null });
    const faster = await h.dashboard();
    await h.interval(agent, 2);
    await h.alarm();
    await h.online(agent, true);
    assert.equal((await h.attachment(agent)).last_seen_ms, staleAtSwitch);
    await h.close(faster);
    await h.interval(agent, 30);
    await h.patch(agent, { last_seen_ms: Date.now() - 30_000, report_grace_until_ms: null });
    await h.alarm();
    await h.online(agent, true);
    const active = await h.dashboard();
    await h.interval(agent, 2);
    await h.patch(agent, { last_seen_ms: Date.now() - 12_000, report_grace_until_ms: null });
    await h.alarm();
    await poll(() => agent.closed === 4000, 'Active agent expires after three actual periods');
    await h.online(agent, false);
    await h.close(active);
    console.log('PASS idle-to-active grace and offline deadlines follow the actual interval');

    const dead = await h.agent('dead');
    assert.equal(dead.welcome.report_interval_s, 30);
    const genuineLastSeen = Date.now() - 120_000;
    await h.patch(dead, { last_seen_ms: genuineLastSeen, report_grace_until_ms: null });
    const churn = await h.dashboard();
    await h.interval(dead, 2);
    await h.close(churn);
    await h.interval(dead, 30);
    assert.equal((await h.attachment(dead)).last_seen_ms, genuineLastSeen);
    await h.alarm();
    await poll(() => dead.closed === 4000, 'Viewer churn cannot revive a dead agent');
    await h.online(dead, false);
    console.log('PASS viewer churn cannot extend a dead agent indefinitely');
  } finally {
    await h.dispose();
  }
}

async function longIdle() {
  const h = await harness(300);
  try {
    const agent = await h.agent('long-idle');
    assert.equal(agent.welcome.report_interval_s, 300);
    await h.patch(agent, { last_seen_ms: Date.now() - 120_000, report_grace_until_ms: null });
    await h.alarm();
    await h.online(agent, true);
    await h.patch(agent, { report_grace_until_ms: Date.now() + 60_000 });
    const minute_ms = Math.floor(Date.now() / 60_000) * 60_000 - 60_000;
    agent.ws.send(JSON.stringify({ type: 'rollup', minute_ms, samples: 60,
      cpu_avg: 10, cpu_max: 20, mem_used_avg: 1, mem_total: 10,
      swap_used_avg: 0, swap_total: 0, disk_used: 1, disk_total: 10,
      rx_bytes: 100, tx_bytes: 10, rx_bps_max: 10, tx_bps_max: 1, load1_avg: 1 }));
    await h.event(agent, 'ack', event => event.minute_ms === minute_ms);
    const refreshed = await h.attachment(agent);
    assert.ok(refreshed.last_seen_ms >= Date.now() - 5000, 'Minute Rollup renews the heartbeat');
    assert.equal(refreshed.report_grace_until_ms ?? null, null, 'A real frame clears transition grace');
    await h.test('recreate');
    await h.alarm();
    await h.online(agent, true);
    await h.patch(agent, { last_seen_ms: Date.now() - 200_000, report_grace_until_ms: null });
    await h.alarm();
    await poll(() => agent.closed === 4000, 'Minute heartbeat bounds long idle offline detection');
    await h.online(agent, false);
    console.log('PASS idle=300s, minute Rollup heartbeat, recreation, 180s offline bound');
  } finally {
    await h.dispose();
  }
}

await defaults();
await longIdle();
console.log('Adaptive reporting integration tests passed.');
