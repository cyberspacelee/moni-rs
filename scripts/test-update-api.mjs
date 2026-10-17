import assert from 'node:assert/strict';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Miniflare, convertV4MiniflareOptions } from 'miniflare';

// Run worker-build --release in crates/worker before this test.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const password = 'update-api-test-admin';
const repo = 'trusted/moni-rs';
const origin = 'https://updates.test';
const admin = { Authorization: `Bearer ${password}` };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function poll(check, label) {
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    const result = await check();
    if (result) return result;
    await sleep(20);
  }
  throw new Error(`Timed out: ${label}`);
}

async function harness(releaseRepo = repo) {
  const directory = await mkdtemp(join(tmpdir(), 'moni-updates-'));
  const peers = [];
  let mf;
  const bindings = {
    ADMIN_PASSWORD: password,
    AUTH_SECRET: 'update-api-test-secret-at-least-32-bytes',
    PUBLIC_DASHBOARD: 'true',
    REPORT_INTERVAL_S: '2',
    REPORT_IDLE_INTERVAL_S: '30',
    RETENTION_MINUTE_DAYS: '7',
    RETENTION_HOUR_DAYS: '90',
    ...(releaseRepo == null ? {} : { AGENT_RELEASE_REPO: releaseRepo }),
    E2E_RUN_ID: 'update-api',
  };
  const options = () => convertV4MiniflareOptions({
    modules: [
      { type: 'ESModule', path: join(directory, 'worker.mjs') },
      { type: 'ESModule', path: join(directory, 'build/index.js') },
      { type: 'CompiledWasm', path: join(directory, 'build/index_bg.wasm') },
    ],
    modulesRoot: directory,
    rootPath: directory,
    compatibilityDate: '2026-10-01',
    durableObjects: { HUB: { className: 'Hub', useSQLite: true } },
    durableObjectsPersist: join(directory, 'state'),
    bindings,
  });
  try {
    await cp(join(root, 'crates/worker/build'), join(directory, 'build'), { recursive: true });
    await writeFile(join(directory, 'worker.mjs'),
      await readFile(join(root, 'scripts/e2e-worker.mjs'), 'utf8'));
    mf = new Miniflare(options());
    await mf.ready;
  } catch (error) {
    await mf?.dispose();
    await rm(directory, { recursive: true, force: true });
    throw error;
  }

  async function api(path, expected = 200, options = {}) {
    const { body, rawBody, headers, ...rest } = options;
    const requestBody = rawBody ?? (body === undefined ? undefined : JSON.stringify(body));
    const response = await mf.dispatchFetch(origin + path, {
      ...rest,
      headers: { ...(requestBody === undefined ? {} : { 'Content-Type': 'application/json' }), ...headers },
      ...(requestBody === undefined ? {} : { body: requestBody }),
    });
    assert.equal(response.status, expected, `${options.method ?? 'GET'} ${path}`);
    return response;
  }
  const test = (name, body = {}) => api('/__test/' + name, 200,
    { method: 'POST', body }).then(response => response.json());
  const sql = (query, args = []) => test('sql', { sql: query, args });
  async function create(id) {
    return (await api('/api/servers', 201,
      { method: 'POST', headers: admin, body: { id, name: id } })).json();
  }
  async function socket(token) {
    const response = await api('/ws/agent', 101,
      { headers: { Upgrade: 'websocket', Authorization: `Bearer ${token}` } });
    const ws = response.webSocket;
    assert.ok(ws);
    const peer = { ws, messages: [], received: [], closed: null };
    ws.addEventListener('message', event => {
      const message = JSON.parse(event.data);
      peer.received.push(message.type);
      peer.messages.push(message);
    });
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
  let sequence = 0;
  async function send(peer, message) {
    const test_sequence = ++sequence;
    peer.ws.send(JSON.stringify({ ...message, test_sequence }));
    return poll(async () => {
      const last = (await test('stats')).last;
      return last.message?.test_sequence === test_sequence && last;
    }, `Message ${test_sequence} processed`);
  }
  async function hello(peer, version = '0.1.0', updateRepo) {
    const stats = await send(peer, { type: 'hello', protocol: 3, agent_version: version,
      host: { hostname: 'fixture', os: 'Linux', kernel: 'test', arch: 'x86_64',
        cpu_model: 'fixture', cpu_cores: 1, boot_time_ms: 0 },
      ...(updateRepo === undefined ? {} : { update_repo: updateRepo }),
    });
    assert.equal((await event(peer, 'welcome')).report_interval_s, 30);
    assert.equal(peer.received.at(0), 'welcome', 'Welcome precedes report/update commands');
    return stats;
  }
  const jobs = () => api('/api/agent-updates', 200, { headers: admin }).then(response => response.json());
  const update = (body, expected = 202) => api('/api/agent-updates', expected,
    { method: 'POST', headers: admin, body }).then(response => response.json());
  const alarm = async () => assert.equal((await test('alarm')).ok, true, 'Alarm succeeds');
  return { api, test, sql, create, socket, event, hello, send, jobs, update, alarm,
    async rotatePassword(value) {
      bindings.ADMIN_PASSWORD = value;
      await mf.setOptions(options());
      await mf.ready;
    },
    async dispose() {
      for (const peer of peers) {
        try { peer.ws.close(1000, 'test cleanup'); } catch {}
      }
      try { await mf.dispose(); }
      finally { await rm(directory, { recursive: true, force: true }); }
    },
  };
}

async function sessionRotation() {
  const h = await harness();
  try {
    const server = await h.create('password-rotation');
    const response = await h.api('/api/auth/login', 204,
      { method: 'POST', headers: { Origin: origin }, body: { password } });
    const cookie = response.headers.get('set-cookie')?.split(';')[0];
    assert.ok(cookie);
    await h.api('/api/auth/me', 200, { headers: { Cookie: cookie } });
    await h.rotatePassword('replacement-test-admin');
    await h.api('/api/auth/me', 401, { headers: { Cookie: cookie } });
    await h.api('/api/auth/me', 401, { headers: admin });
    await h.api('/api/auth/me', 200,
      { headers: { Authorization: 'Bearer replacement-test-admin' } });
    await h.hello(await h.socket(server.token));
    console.log('ok: password rotation revokes old sessions and preserves Agent tokens');
  } finally { await h.dispose(); }
}

async function rollupRetries() {
  const h = await harness();
  try {
    const server = await h.create('rollup-retries');
    const peer = await h.socket(server.token);
    await h.hello(peer);
    const minute = Math.floor(Date.now() / 60_000) * 60_000 - 60_000;
    const base = { type: 'rollup', minute_ms: minute, samples: 60, cpu_avg: 10, cpu_max: 20,
      mem_used_avg: 1, mem_total: 10, swap_used_avg: 1, swap_total: 10,
      disk_used: 1, disk_total: 10, rx_bytes: 100, tx_bytes: 10,
      rx_bps_max: 10, tx_bps_max: 1, load1_avg: 1 };
    async function send(rollup) {
      peer.ws.send(JSON.stringify(rollup));
      await h.event(peer, 'ack', event => event.minute_ms === rollup.minute_ms);
      return (await h.test('stats')).last;
    }
    assert.equal((await send(base)).rowsWritten, 1);
    assert.equal((await send(base)).rowsWritten, 0);
    const changes = { cpu_avg: 11, cpu_max: 21, mem_used_avg: 2, swap_used_avg: 2,
      disk_used: 2, rx_bytes: 250, tx_bytes: 25, rx_bps_max: 11, tx_bps_max: 2, load1_avg: 2 };
    for (const [field, value] of Object.entries(changes)) {
      const changed = { ...base, [field]: value };
      assert.equal((await send(changed)).rowsWritten, 1, `${field} correction is persisted`);
      assert.equal((await send(changed)).rowsWritten, 0, `${field} retry writes nothing`);
      await send(base);
    }
    assert.equal((await send({ ...base, samples: 59 })).rowsWritten, 0,
      'An unstored sample count change does not rewrite identical aggregate columns');
    await send({ ...base, rx_bytes: 250, tx_bytes: 25 });
    const status = (await (await h.api('/api/servers', 200, { headers: admin })).json())[0];
    assert.equal(status.traffic_month.rx_bytes, 250);
    assert.equal(status.traffic_month.tx_bytes, 25);

    const old = { ...base, minute_ms: Math.floor(Date.now() / 3_600_000) * 3_600_000 - 3 * 3_600_000 };
    await send(old);
    await h.sql('DELETE FROM dirty_hours');
    const replay = await send(old);
    assert.equal(replay.rowsWritten, 0);
    assert.ok(!replay.queries.some(query => query.query.includes('INSERT INTO dirty_hours')
      || query.query.includes('INSERT OR IGNORE INTO dirty_hours')));
    assert.equal((await h.sql('SELECT COUNT(*) AS n FROM dirty_hours'))[0].n, 0);
    await send({ ...old, cpu_avg: 12 });
    assert.equal((await h.sql('SELECT COUNT(*) AS n FROM dirty_hours'))[0].n, 1);
    console.log('ok: identical Rollup retries write nothing; all corrections and traffic deltas persist');
  } finally { await h.dispose(); }
}

async function welcomeValidation() {
  const h = await harness();
  try {
    const noHello = await h.create('no-hello');
    const peer = await h.socket(noHello.token);
    await h.test('stats');
    await sleep(50);
    assert.equal(peer.messages.filter(message => message.type === 'welcome').length, 0,
      'A WebSocket upgrade alone must not confirm Agent health');
    const host = { hostname: 'fixture', os: 'Linux', kernel: 'test', arch: 'x86_64',
      cpu_model: 'fixture', cpu_cores: 1, boot_time_ms: 0 };
    for (const [id, fields, code] of [
      ['wrong-protocol', { protocol: 1 }, 4001],
      ['oversized-host', { host: { ...host, hostname: 'x'.repeat(8193) } }, 1009],
      ['invalid-repo', { update_repo: 'https://evil.test/repo' }, 1009],
    ]) {
      const server = await h.create(id);
      const invalid = await h.socket(server.token);
      await h.send(invalid, { type: 'hello', protocol: 3, agent_version: '0.1.0', host, ...fields });
      await poll(() => invalid.closed !== null, `${id} closed`);
      assert.equal(invalid.closed, code, id);
      assert.equal(invalid.messages.filter(message => message.type === 'welcome').length, 0, id);
    }
    const failedStore = await h.create('hello-write-failed');
    const invalid = await h.socket(failedStore.token);
    await h.test('faults', { sql: 'UPDATE servers SET host_json' });
    await h.send(invalid, { type: 'hello', protocol: 3, agent_version: '0.1.0', host });
    assert.equal(invalid.messages.filter(message => message.type === 'welcome').length, 0,
      'A failed Hello database write must not confirm Agent health');
    console.log('ok: Welcome follows a valid, persisted Hello; absent or rejected Hello confirms no health');
  } finally { await h.dispose(); }
}

async function updateAuthorization() {
  const h = await harness();
  try {
    const server = await h.create('authorization');
    const body = { version: 'v0.2.0', server_ids: ['authorization'] };
    for (const headers of [{}, { Authorization: `Bearer ${server.token}` }, { 'x-moni-admin': '1' }]) {
      await h.api('/api/agent-updates', 401, { headers });
      await h.api('/api/agent-updates', 401, { method: 'POST', headers, body });
    }
    const login = await h.api('/api/auth/login', 204,
      { method: 'POST', headers: { Origin: origin }, body: { password } });
    const cookie = login.headers.get('set-cookie')?.split(';')[0];
    assert.ok(cookie);
    await h.api('/api/agent-updates', 200, { headers: { Cookie: cookie } });
    for (const headers of [{ Cookie: cookie }, { Cookie: cookie, Origin: 'https://other.test' }]) {
      await h.api('/api/agent-updates', 403, { method: 'POST', headers, body });
    }
    await h.api('/api/agent-updates', 415, { method: 'POST', headers: {
      ...admin, 'Content-Type': 'text/plain', 'Content-Length': String(JSON.stringify(body).length),
    }, body });
    await h.api('/api/agent-updates', 202,
      { method: 'POST', headers: { Cookie: cookie, Origin: origin }, body });
    console.log('ok: upgrade API requires admin credentials, same-origin Cookie writes, and JSON');
  } finally { await h.dispose(); }

  for (const releaseRepo of [null, 'https://github.com/attacker/repo']) {
    const h = await harness(releaseRepo);
    try {
      await h.create('not-configured');
      await h.update({ version: 'v0.2.0', server_ids: ['not-configured'] }, 503);
      assert.deepEqual(await h.jobs(), []);
    } finally { await h.dispose(); }
  }
  console.log('ok: upgrades require a valid deployment-pinned release repository');
}

async function updateValidation() {
  const h = await harness();
  try {
    await h.create('validation');
    for (const version of ['0.2.0', 'latest', 'v1.2', 'v01.2.3', 'v1.2.3-rc1', 'v1000000000.1.2',
      'v1.2.3/../../evil', 'https://evil.test/agent', 'v1.2.3;id', 'v1.2.3\n', `v${'1'.repeat(65)}.2.3`]) {
      await h.update({ version, server_ids: ['validation'] }, 400);
    }
    for (const server_ids of [[], ['validation', 'validation'], ['../validation'], Array(101).fill('validation')]) {
      await h.update({ version: 'v0.2.0', server_ids }, 400);
    }
    await h.update({ version: 'v0.2.0', server_ids: ['validation', 'unknown'] }, 404);
    assert.deepEqual(await h.jobs(), [], 'All target IDs are validated before inserting any job');
    for (const fields of [{ repo: 'attacker/repo' }, { url: 'https://evil.test/agent' }, { command: 'id' }]) {
      await h.update({ version: 'v0.2.0', server_ids: ['validation'], ...fields }, 400);
    }
    for (const rawBody of ['{',
      '{"version":"v0.2.0","version":"v0.3.0","server_ids":["validation"]}']) {
      await h.api('/api/agent-updates', 400, { method: 'POST', headers: admin, rawBody });
    }
    await h.api('/api/agent-updates', 413, { method: 'POST', headers: admin,
      rawBody: JSON.stringify({ version: 'v0.2.0', server_ids: ['validation'] }) + ' '.repeat(16_384) });
    assert.deepEqual(await h.jobs(), [], 'Client-supplied sources and commands are rejected');
    const [job] = await h.update({ version: 'v0.2.0', server_ids: ['validation'] });
    assert.equal(job.repo, repo, 'Updates use the trusted deployment repository');
    assert.match(job.job_id, /^[0-9a-f]{32}$/);
    assert.equal(job.state, 'queued');
    assert.equal((await h.update({ version: 'v0.2.0' }))[0].job_id, job.job_id,
      'A repeated active request and omitted server_ids preserve the same job');
    await h.update({ version: 'v0.3.0', server_ids: ['validation'] }, 409);
    assert.equal((await h.jobs())[0].job_id, job.job_id);
    console.log('ok: release tags and targets are bounded, source is pinned, requests are idempotent');
  } finally { await h.dispose(); }
}

async function updateLifecycle() {
  const h = await harness();
  try {
    const alpha = await h.create('lifecycle-alpha');
    const beta = await h.create('lifecycle-beta');
    const [job] = await h.update({ version: 'v0.2.0', server_ids: ['lifecycle-alpha'] });
    assert.equal(job.state, 'queued');
    assert.ok((await h.test('stats')).nextAlarm, 'Offline jobs retain an expiry alarm');
    await h.test('recreate');
    assert.equal((await h.jobs())[0].job_id, job.job_id, 'Queued job survives Hub recreation');
    const a = await h.socket(alpha.token);
    await h.hello(a, '0.1.0', repo);
    assert.deepEqual(await h.event(a, 'update'),
      { type: 'update', job_id: job.job_id, version: job.version, repo });
    await h.test('recreate');
    await h.alarm();
    assert.equal(a.messages.filter(message => message.type === 'update').length, 0,
      'Hibernation preserves the delivery timestamp and does not immediately resend');

    const b = await h.socket(beta.token);
    await h.hello(b, '0.1.0', repo);
    const [other] = await h.update({ version: 'v0.3.0', server_ids: ['lifecycle-beta'] });
    await h.event(b, 'update');
    const status = (peer, fields = {}) => h.send(peer, { type: 'update_status', job_id: job.job_id,
      version: job.version, state: 'accepted', error: null, ...fields });
    assert.equal((await status(b)).rowsWritten, 0, 'Another server cannot accept this job');
    assert.equal((await status(a, { version: 'v0.3.0' })).rowsWritten, 0, 'Mismatched versions are ignored');
    assert.equal((await status(a, { state: 'succeeded' })).rowsWritten, 0,
      'Agent status cannot claim a successful upgrade');
    assert.equal((await h.jobs()).find(row => row.server_id === 'lifecycle-alpha').state, 'queued');
    assert.equal((await status(a)).rowsWritten, 1);
    assert.equal((await status(a)).rowsWritten, 0, 'Repeated acceptance does not write SQLite');
    assert.equal((await h.jobs()).find(row => row.server_id === 'lifecycle-beta').job_id, other.job_id);
    await h.test('recreate');
    assert.equal((await h.jobs()).find(row => row.server_id === 'lifecycle-alpha').state, 'accepted');
    const upgraded = await h.socket(alpha.token);
    await h.hello(upgraded, '0.2.0', repo);
    assert.equal((await h.jobs()).find(row => row.server_id === 'lifecycle-alpha').state, 'succeeded',
      'Only a new Hello with the target version confirms success');
    assert.equal(upgraded.messages.filter(message => message.type === 'update').length, 0);
    const later = await h.socket(alpha.token);
    await h.hello(later, '0.3.0', repo);
    assert.equal((await h.jobs()).find(row => row.server_id === 'lifecycle-alpha').state, 'succeeded',
      'A newer version keeps the preceding upgrade successful');
    const rolledBack = await h.socket(alpha.token);
    await h.hello(rolledBack, '0.1.0', repo);
    await h.test('recreate');
    const rollback = (await h.jobs()).find(row => row.server_id === 'lifecycle-alpha');
    assert.equal(rollback.job_id, job.job_id);
    assert.equal(rollback.state, 'failed', 'A lower-version Hello retracts the successful upgrade');
    assert.equal(rollback.error, 'request_failed');
    await h.api('/api/servers/lifecycle-alpha', 204, { method: 'DELETE', headers: admin });
    assert.ok((await h.jobs()).every(row => row.server_id !== 'lifecycle-alpha'), 'Deleting a server removes its job');
    console.log('ok: offline upgrade jobs survive recreation; status stays bound to server, job, and version');
  } finally { await h.dispose(); }
}

async function updateCapabilities() {
  const h = await harness();
  try {
    for (const [id, version, capability, expectedState, expectedError] of [
      ['updates-disabled', '0.1.0', undefined, 'unsupported', 'unsupported'],
      ['different-repo', '0.1.0', 'other/moni-rs', 'failed', 'repo_mismatch'],
      ['downgrade', '0.3.0', repo, 'failed', 'downgrade'],
      ['already-current', '0.2.0', repo, 'succeeded', null],
    ]) {
      const server = await h.create(id);
      const peer = await h.socket(server.token);
      await h.hello(peer, version, capability);
      const [job] = await h.update({ version: 'v0.2.0', server_ids: [id] });
      assert.equal(job.state, expectedState, id);
      assert.equal(job.error, expectedError, id);
      assert.equal(peer.messages.filter(message => message.type === 'update').length, 0, id);
    }
    console.log('ok: Agents without local update permission receive no update command; repository mismatch and downgrade fail closed');
  } finally { await h.dispose(); }
}

async function updateTimeouts() {
  const h = await harness();
  try {
    const server = await h.create('retry-limit');
    const peer = await h.socket(server.token);
    await h.hello(peer, '0.1.0', repo);
    const [job] = await h.update({ version: 'v0.2.0', server_ids: ['retry-limit'] });
    await h.event(peer, 'update');
    for (let delivery = 2; delivery <= 3; delivery++) {
      await h.test('attachment', { id: 'retry-limit', patch: {
        last_seen_ms: Date.now(), update_sent_ms: Date.now() - 61_000,
      } });
      await h.test('recreate');
      await h.alarm();
      assert.equal((await h.event(peer, 'update')).job_id, job.job_id, `Delivery ${delivery}`);
    }
    await h.test('attachment', { id: 'retry-limit', patch: {
      last_seen_ms: Date.now(), update_sent_ms: Date.now() - 61_000,
    } });
    await h.alarm();
    assert.equal((await h.jobs())[0].state, 'failed');
    assert.equal((await h.jobs())[0].error, 'timeout');
    assert.equal(peer.messages.filter(message => message.type === 'update').length, 0);

    const [accepted] = await h.update({ version: 'v0.3.0', server_ids: ['retry-limit'] });
    await h.event(peer, 'update');
    await h.send(peer, { type: 'update_status', job_id: accepted.job_id,
      version: accepted.version, state: 'accepted', error: null });
    await h.sql('UPDATE agent_updates SET updated_ms=? WHERE server_id=?',
      [Date.now() - 601_000, 'retry-limit']);
    await h.alarm();
    assert.equal((await h.jobs())[0].error, 'timeout', 'Accepted upgrade expires after ten minutes');

    await h.create('offline-timeout');
    await h.update({ version: 'v0.2.0', server_ids: ['offline-timeout'] });
    await h.sql('UPDATE agent_updates SET requested_ms=? WHERE server_id=?',
      [Date.now() - 86_401_000, 'offline-timeout']);
    await h.test('recreate');
    await h.alarm();
    const expired = (await h.jobs()).find(job => job.server_id === 'offline-timeout');
    assert.equal(expired.state, 'failed');
    assert.equal(expired.error, 'timeout');
    console.log('ok: retry state survives recreation; deliveries and queued/accepted lifetimes are bounded');
  } finally { await h.dispose(); }
}

const scenarios = [sessionRotation, rollupRetries, welcomeValidation, updateAuthorization, updateValidation,
  updateLifecycle, updateCapabilities, updateTimeouts];
const selected = new Set(process.argv.slice(2));
for (const name of selected) assert.ok(scenarios.some(scenario => scenario.name === name), `Unknown scenario: ${name}`);
for (const scenario of scenarios) {
  if (selected.size === 0 || selected.has(scenario.name)) await scenario();
}
