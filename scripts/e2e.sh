#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
worker_dir="$repo_dir/crates/worker"
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/moni-e2e.XXXXXXXX")
quick=false
case "${1:-}" in
  --quick) quick=true ;;
  "") ;;
  *) echo "Usage: $0 [--quick]" >&2; exit 2 ;;
esac
cleanup() {
  if [[ "${E2E_KEEP:-0}" == 1 ]]; then
    echo "e2e logs/state: $test_dir"
  else
    rm -rf -- "$test_dir"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

cd "$repo_dir"
# Wrangler 4.151.1 requires Node 22+, which also provides WebSocket without dependencies.
node -e 'if (Number(process.versions.node.split(".")[0]) < 22 || !globalThis.WebSocket) { console.error("Node 22+ is required by Wrangler and the built-in WebSocket client"); process.exit(1); }'
[[ -f web/dist/.gitkeep || -f web/dist/index.html ]] || {
  echo 'web/dist needs .gitkeep or a frontend build' >&2
  exit 1
}

cargo build -p moni-agent >"$test_dir/agent-build.log" 2>&1 || {
  cat "$test_dir/agent-build.log" >&2; exit 1;
}
(cd "$worker_dir" && worker-build --release) >"$test_dir/worker-build.log" 2>&1 || {
  cat "$test_dir/worker-build.log" >&2; exit 1;
}

port=${E2E_PORT:-$(node -e 'const s=require("node:net").createServer();s.listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')}
node --input-type=module - "$worker_dir" "$test_dir/config.json" <<'JS'
import fs from 'node:fs';
import path from 'node:path';
const [worker, target] = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(path.join(worker, 'wrangler.jsonc'), 'utf8').replace(/^\s*\/\/.*$/gm, ''));
delete config.$schema;
config.main = path.join(worker, config.main);
config.assets.directory = path.resolve(worker, config.assets.directory);
delete config.build;
config.vars.ADMIN_PASSWORD = 'e2e-admin-password';
config.vars.AUTH_SECRET = 'e2e-signing-secret-at-least-32-bytes-long';
fs.writeFileSync(target, JSON.stringify(config));
JS

node --input-type=module - "$port" "$repo_dir" "$quick" "$test_dir" <<'JS'
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
const [port, repo, quickFlag, temp] = process.argv.slice(2);
const quick = quickFlag === "true";
const e2eStartedMs = Date.now();
const WS = globalThis.WebSocket;
const base = `http://127.0.0.1:${port}`;
const password = 'e2e-admin-password';
const admin = { Authorization: `Bearer ${password}` };
const sockets = [];
let agent, wrangler, webhook;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const timeout = setTimeout(() => { console.error('e2e timed out'); stopChildren(); process.exit(1); }, 360_000);
function stopChildren() {
  webhook?.closeAllConnections();
  webhook?.close();
  agent?.kill('SIGCONT');
  agent?.kill('SIGTERM');
  if (wrangler) { try { process.kill(-wrangler.pid, 'SIGKILL'); } catch {} }
  for (const ws of sockets) if (ws.readyState === 1) ws.close();
}
process.on('SIGINT', () => { stopChildren(); process.exit(130); });
process.on('SIGTERM', () => { stopChildren(); process.exit(143); });
async function startWorker() {
  const log = fs.openSync(path.join(temp, 'wrangler.log'), 'a');
  wrangler = spawn(path.join(repo, 'node_modules/.bin/wrangler'), ['dev',
    '--config', path.join(temp, 'config.json'), '--local', '--ip', '127.0.0.1',
    '--port', port, '--inspector-port', '0', '--persist-to', path.join(temp, 'state')], {
    env: { ...process.env, CLOUDFLARE_SEND_METRICS: 'false' },
    detached: true, stdio: ['ignore', log, log],
  });
  fs.closeSync(log);
  for (let i = 0; i < 200; i++) {
    try { if ((await fetch(base + '/api/health')).ok) return; } catch {}
    assert.equal(wrangler.exitCode, null, 'Wrangler exited during startup');
    await pause(100);
  }
  throw new Error('Wrangler did not become ready');
}
async function stopWorker() {
  const exited = once(wrangler, 'exit');
  process.kill(-wrangler.pid, 'SIGKILL');
  await exited;
  wrangler = undefined;
}
function editStorage(source) {
  const result = spawnSync('python3', ['-c', source, path.join(temp, 'state')], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}
async function api(url, status = 200, { method = 'GET', headers = {}, body } = {}) {
  const response = await fetch(base + url, {
    method, headers: { 'Content-Type': 'application/json', ...headers },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  assert.equal(response.status, status, `${method} ${url}: ${response.status}`);
  return response;
}
function socket(url, headers) {
  const ws = new WS(base.replace('http:', 'ws:') + url, { headers });
  sockets.push(ws);
  const events = [];
  ws.addEventListener('message', event => events.push(JSON.parse(event.data)));
  const closed = new Promise(resolve => ws.addEventListener('close', event => resolve(event.code)));
  return { ws, events, closed };
}
async function event(client, type, matches = () => true, waitMs = 10_000) {
  for (let i = 0; i < waitMs / 50; i++) {
    const found = client.events.find(e => e.type === type && matches(e));
    if (found) return found;
    await pause(50);
  }
  throw new Error(`Missing WebSocket event: ${type}`);
}
// fetch disallows Upgrade; use the Node HTTP handshake so a rejected upgrade is still inspectable.
async function rejectedUpgrade(token, extra = {}) {
  const { request } = await import('node:http');
  await new Promise((resolve, reject) => {
    const req = request(base + '/ws/agent', { headers: {
      ...extra, Authorization: `Bearer ${token}`, Upgrade: 'websocket', Connection: 'Upgrade',
      'Sec-WebSocket-Version': '13', 'Sec-WebSocket-Key': 'MDEyMzQ1Njc4OWFiY2RlZg==',
    }}, response => {
      let body = '';
      response.on('data', chunk => { body += chunk; });
      response.on('end', () => { try { assert.equal(response.statusCode, 401); assert.equal(JSON.parse(body).error, 'unauthorized'); resolve(); } catch (error) { reject(error); } });
    });
    req.on('upgrade', (_response, connection) => { connection.destroy(); reject(new Error('Invalid token was accepted')); });
    req.on('error', reject);
    req.end();
  });
}
try {
  await startWorker();
  assert.equal(await (await api('/api/health')).json(), 'ok');
  assert.equal((await (await api('/api/auth/me', 401)).json()).error, 'unauthorized');
  await api('/api/auth/me', 401, { headers: { Authorization: `Bearer ${password}x` } });
  await api('/api/auth/login', 400, { method: 'POST', body: {} });
  await api('/api/auth/login', 401, { method: 'POST', body: { password: 'wrong' } });
  const login = await api('/api/auth/login', 204, { method: 'POST', body: { password } });
  const cookieHeader = login.headers.get('set-cookie');
  assert.match(cookieHeader, /HttpOnly/);
  assert.match(cookieHeader, /SameSite=Strict/);
  assert.match(cookieHeader, /Max-Age=2592000/);
  assert.ok(!cookieHeader.includes('; Secure'));
  const Cookie = cookieHeader.split(';')[0];
  assert.equal((await (await api('/api/auth/me', 200, { headers: { Cookie } })).json()).authenticated, true);
  await api('/api/auth/me', 401, { headers: { Cookie: Cookie + 'x' } });
  const config = await (await api('/api/config', 200, { headers: admin })).json();
  assert.deepEqual(config.retention, { minute_days: 7, hour_days: 90 });
  let dash = socket('/ws/dashboard', { Cookie });
  assert.deepEqual((await event(dash, 'snapshot')).alerts, []);
  assert.deepEqual(await (await api('/api/alerts/test', 200, { method: 'POST', headers: admin })).json(),
    { telegram: 'disabled', webhook: 'disabled' });
  const defaultRules = await (await api('/api/alerts/rules', 200, { headers: admin })).json();
  assert.equal(defaultRules.length, 4);
  for (const [metric, duration] of [['offline', 60], ['cpu', 300], ['mem', 180], ['disk', 0]]) {
    const rule = defaultRules.find(r => r.metric === metric);
    assert.equal(rule.duration_s, duration);
    assert.equal(rule.enabled, true);
    assert.equal(rule.server_id, null);
    if (metric !== 'offline') assert.equal(rule.threshold, 90);
  }
  const removedDefault = defaultRules.find(r => r.metric === 'disk').id;
  await api(`/api/alerts/rules/${removedDefault}`, 204, { method: 'DELETE', headers: admin });
  const created = await (await api('/api/servers', 201, { method: 'POST', headers: admin, body: { name: 'E2E VPS' } })).json();
  const { server, token } = created;
  assert.equal(server.id, 'e2e-vps');
  assert.match(token, /^mn_e2e-vps_[A-Za-z0-9_-]{43}$/);
  assert.equal(server.traffic_month.rx_bytes, 0);
  assert.equal(server.alerts_firing, 0);
  assert.equal(new Date(server.traffic_month.since_ms).getUTCDate(), 1);
  await event(dash, 'server_upsert', e => e.server.id === server.id);
  await api('/api/servers', 409, { method: 'POST', headers: admin, body: { id: server.id, name: 'Conflict' } });
  await api('/api/servers', 400, { method: 'POST', headers: admin, body: { id: 'INVALID', name: 'Bad id' } });
  const collision = await (await api('/api/servers', 201, { method: 'POST', headers: admin, body: { name: 'E2E VPS' } })).json();
  assert.equal(collision.server.id, 'e2e-vps-2');
  // Deterministic history fixtures exercise aggregation/percentages independently of the host load.
  const fixture = socket('/ws/agent', { Authorization: `Bearer ${collision.token}` });
  await event(fixture, 'welcome');
  const ruleInput = { name: 'CPU E2E', server_id: collision.server.id, metric: 'cpu', op: 'gt',
    threshold: 0, recover_threshold: null, duration_s: 0, enabled: true };
  const ruleUrl = '/api/alerts/rules';
  const patchRule = (id, body) => api(`${ruleUrl}/${id}`, 200, { method: 'PATCH', headers: admin, body }).then(r => r.json());
  for (const invalid of [{ metric: 'invalid' }, { op: 'eq' }, { threshold: 'NaN' },
    { duration_s: -1 }, { duration_s: 86401 }, { duration_s: 0.5 }, { server_id: 'missing' }]) {
    await api(ruleUrl, 400, { method: 'POST', headers: admin, body: { ...ruleInput, ...invalid } });
  }
  const rule = await (await api(ruleUrl, 201, { method: 'POST', headers: admin, body: ruleInput })).json();
  const cpuFrame = { type: 'metrics', seq: 1, ts_ms: Date.now(), cpu_pct: 50,
    mem_used: 1, mem_total: 10, swap_used: 0, swap_total: 0, disk_used: 4, disk_total: 8,
    net_rx_bps: 0, net_tx_bps: 0, load1: 1, load5: 1, load15: 1, uptime_s: 1, process_count: 1 };
  const alertEvent = (kind, id = rule.id) => event(dash, 'alert', e => e.kind === kind && e.alert.rule_id === id);
  fixture.ws.send(JSON.stringify(cpuFrame));
  const firing = await alertEvent('firing');
  assert.equal(firing.alert.value, 50);
  assert.equal(firing.alert.server_name, 'E2E VPS');
  let active = await (await api('/api/alerts/active', 200, { headers: admin })).json();
  assert.equal(active.find(a => a.rule_id === rule.id).value, 50);
  assert.equal((await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === collision.server.id).alerts_firing, 1);
  const alertDash = socket('/ws/dashboard', { Cookie });
  const alertSnapshot = await event(alertDash, 'snapshot');
  assert.equal(alertSnapshot.alerts.find(a => a.rule_id === rule.id).value, 50);
  assert.equal(alertSnapshot.servers.find(s => s.id === collision.server.id).alerts_firing, 1);
  await patchRule(rule.id, { threshold: 1000 });
  fixture.ws.send(JSON.stringify(cpuFrame));
  await alertEvent('resolved');
  assert.deepEqual(await (await api('/api/alerts/active', 200, { headers: admin })).json(), []);
  // Nullable PATCH fields distinguish omission from an explicit clear.
  assert.equal((await patchRule(rule.id, { recover_threshold: 25 })).server_id, collision.server.id);
  let patched = await patchRule(rule.id, { server_id: null, recover_threshold: null });
  assert.equal(patched.server_id, null);
  assert.equal(patched.recover_threshold, null);
  await api(`${ruleUrl}/${rule.id}`, 400, { method: 'PATCH', headers: admin, body: { threshold: null } });
  await api(`${ruleUrl}/${rule.id}`, 400, { method: 'PATCH', headers: admin, body: { duration_s: 86401 } });
  await api(`${ruleUrl}/${rule.id}`, 400, { method: 'PATCH', headers: admin, body: { server_id: 'missing' } });
  await patchRule(rule.id, { threshold: 0, server_id: collision.server.id });
  dash.events.length = 0;
  fixture.ws.send(JSON.stringify(cpuFrame));
  await alertEvent('firing');
  await patchRule(rule.id, { enabled: false });
  await alertEvent('resolved');
  dash.events.length = 0;
  await patchRule(rule.id, { enabled: true });
  fixture.ws.send(JSON.stringify(cpuFrame));
  await alertEvent('firing');
  await api(`${ruleUrl}/${rule.id}`, 204, { method: 'DELETE', headers: admin });
  await alertEvent('resolved');
  await api(`${ruleUrl}/${rule.id}`, 404, { method: 'DELETE', headers: admin });
  await api(`${ruleUrl}/missing`, 404, { method: 'PATCH', headers: admin, body: {} });
  const alertHistory = await (await api('/api/alerts/events?limit=2', 200, { headers: admin })).json();
  assert.equal(alertHistory.length, 2);
  assert.equal(alertHistory[0].rule_name, rule.name);
  assert.equal(alertHistory[0].kind, 'resolved');
  const olderAlerts = await (await api(`/api/alerts/events?limit=200&before_id=${alertHistory[0].id}`, 200, { headers: admin })).json();
  assert.ok(olderAlerts.every(e => e.id < alertHistory[0].id));
  assert.equal(olderAlerts[0].id, alertHistory[1].id);
  await api('/api/alerts/events?limit=0', 400, { headers: admin });
  await api('/api/alerts/events?before_id=bad', 400, { headers: admin });
  console.log('Alerts passed: defaults, validation, nullable PATCH, firing/recovery, snapshot/counts, disable/delete and event pagination');
  const fixtureHour = Math.floor(Date.now() / 3_600_000) * 3_600_000 - 3 * 3_600_000;
  const rollup = (minute_ms, cpu_avg, rx_bytes, peak) => ({ type: 'rollup', minute_ms,
    samples: 60, cpu_avg, cpu_max: cpu_avg + 5, mem_used_avg: cpu_avg / 10,
    mem_total: 10, swap_used_avg: 0, swap_total: 0, disk_used: 4, disk_total: 8,
    rx_bytes, tx_bytes: rx_bytes / 10, rx_bps_max: peak, tx_bps_max: peak + 1,
    load1_avg: cpu_avg / 10 });
  for (const message of [rollup(fixtureHour + 60_000, 10, 100, 5),
    rollup(fixtureHour + 120_000, 30, 200, 15), rollup(fixtureHour + 120_000, 30, 200, 15),
    rollup(fixtureHour + 1_200_000, 50, 300, 10), rollup(60_000, 10, 1000, 1),
    rollup(Math.floor(Date.now() / 60_000) * 60_000 + 180_000, 10, 1000, 1)]) {
    fixture.events.length = 0;
    fixture.ws.send(JSON.stringify(message));
    assert.equal((await event(fixture, 'ack')).minute_ms, message.minute_ms);
  }
  const fixturePath = `/api/servers/${collision.server.id}/history`;
  await api(fixturePath + '?range=bad', 400, { headers: admin });
  await api('/api/servers/missing/history?range=1h', 404, { headers: admin });
  for (const [range, step_ms, count] of [['1h', 60_000, 0], ['6h', 60_000, 3],
    ['24h', 60_000, 3], ['7d', 600_000, 2], ['30d', 3_600_000, 0], ['90d', 3_600_000, 0]]) {
    const history = await (await api(fixturePath + '?range=' + range, 200, { headers: admin })).json();
    assert.equal(history.range, range);
    assert.equal(history.step_ms, step_ms);
    assert.equal(history.points.length, count);
    assert.deepEqual(history.points.map(p => p.ts_ms), history.points.map(p => p.ts_ms).sort((a,b) => a-b));
    if (range === '7d') assert.deepEqual(history.points[0], { ts_ms: fixtureHour,
      cpu_avg: 20, cpu_max: 35, mem_pct: 20, swap_pct: 0, disk_pct: 50,
      rx_bytes: 300, tx_bytes: 30, rx_bps_max: 15, tx_bps_max: 16, load1: 2 });
  }
  assert.equal((await (await api(fixturePath, 200, { headers: admin })).json()).range, '24h');
  // Neither message nor credential may be echoed, and a burst produces just one warning.
  for (let i = 0; i < 10; i++) fixture.ws.send(JSON.stringify({ type: 'mn_DO_NOT_LOG_THIS_TOKEN' }));
  await pause(100);
  const warnings = fs.readFileSync(path.join(temp, 'wrangler.log'), 'utf8');
  assert.ok(!warnings.includes('mn_DO_NOT_LOG_THIS_TOKEN'));
  assert.equal(warnings.split(`Invalid Agent message server=${collision.server.id}:`).length - 1, 1);
  fixture.ws.close();
  await rejectedUpgrade(`mn_${server.id}_${Buffer.alloc(32, 1).toString('base64url')}`, { 'x-moni-server-id': server.id, 'x-moni-token-hash': 'forged' });
  await rejectedUpgrade('bad-token', { 'x-moni-server-id': server.id, 'x-moni-token-hash': 'forged' });
  const agentLog = fs.openSync(path.join(temp, 'agent.log'), 'w');
  const startedMs = Date.now();
  agent = spawn(path.join(repo, 'target/debug/moni-agent'), [], {
    env: { ...process.env, MONI_SERVER: base.replace('http:', 'ws:') + '/ws/agent',
      MONI_TOKEN: token, MONI_IFACES: 'lo' },
    stdio: ['ignore', agentLog, agentLog],
  });
  fs.closeSync(agentLog);
  await event(dash, 'online', e => e.id === server.id);
  await event(dash, 'metrics', e => e.id === server.id);
  const listed = await (await api('/api/servers', 200, { headers: admin })).json();
  const live = listed.find(s => s.id === server.id);
  assert.equal(live.online, true);
  assert.ok(live.host);
  assert.ok(live.metrics);
  const renamed = await (await api(`/api/servers/${server.id}`, 200, { method: 'PATCH', headers: admin, body: { name: 'Renamed' } })).json();
  assert.equal(renamed.name, 'Renamed');
  await event(dash, 'server_upsert', e => e.server.name === 'Renamed');
  if (!quick) {
    console.log('Waiting for more than two minutes of native Agent rollups...');
    while (Date.now() - startedMs < 125_000) {
      await api('/api/servers', 200, { headers: admin });
      await pause(1000);
    }
    let history = await (await api(`/api/servers/${server.id}/history?range=1h`, 200, { headers: admin })).json();
    assert.ok(history.points.length >= 2, 'Native Agent must persist at least two minutes');
    const status = (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === server.id);
    assert.ok(status.traffic_month.rx_bytes + status.traffic_month.tx_bytes > 0);
    const freshDash = socket('/ws/dashboard', { Cookie });
    assert.deepEqual((await event(freshDash, 'snapshot')).servers.find(s => s.id === server.id).traffic_month, status.traffic_month);
    const late = socket('/ws/agent', { Authorization: `Bearer ${collision.token}` });
    await event(late, 'welcome');
    late.ws.send(JSON.stringify(rollup(fixtureHour + 120_000, 30, 400, 15)));
    await event(late, 'ack');
    console.log('Killing Wrangler for 62s while Agent keeps sampling and queues rollups...');
    await stopWorker();
    // Force maintenance on restart and add expired rows; SQL edits happen only with workerd stopped.
    editStorage(`
import pathlib, sqlite3, sys, time
for file in pathlib.Path(sys.argv[1]).rglob('*.sqlite'):
    db = sqlite3.connect(file)
    if not db.execute("SELECT name FROM sqlite_master WHERE name='metrics_1m'").fetchone():
        db.close(); continue
    assert db.execute("SELECT COUNT(*) FROM dirty_hours WHERE server_id='e2e-vps-2'").fetchone()[0] == 1
    db.execute("UPDATE meta SET v='0' WHERE k='last_hourly_ms'")
    for table in ('metrics_1m', 'metrics_1h'):
        db.execute(f"INSERT INTO {table} (server_id,ts_ms) VALUES ('e2e-vps',0)")
    db.commit(); db.close()
`);
    await pause(62_000);
    await startWorker();
    const restartedMs = Date.now();
    dash = socket('/ws/dashboard', { Cookie });
    await event(dash, 'snapshot');
    assert.ok(!(await (await api(ruleUrl, 200, { headers: admin })).json()).some(r => r.id === removedDefault), 'Deleted default must stay deleted after restart');
    await event(dash, 'metrics', e => e.id === server.id, 50_000);
    // Wait for the first alarm after restart to aggregate/clean the persisted fixtures.
    await pause(Math.max(0, 31_000 - (Date.now() - restartedMs)));
    history = await (await api(`/api/servers/${server.id}/history?range=1h`, 200, { headers: admin })).json();
    assert.match(fs.readFileSync(path.join(temp, 'agent.log'), 'utf8'), /resent pending rollups count=[1-9]/,
      'The running Agent must replay at least one rollup queued during the outage');
    const minutes = new Set(history.points.map(point => point.ts_ms));
    const firstRequired = Math.floor(startedMs / 60_000) * 60_000 + 60_000;
    const lastRequired = Math.floor(Date.now() / 60_000) * 60_000 - 60_000;
    for (let minute = firstRequired; minute <= lastRequired; minute += 60_000) {
      assert.ok(minutes.has(minute), `Missing minute across restart: ${minute}`);
    }
    assert.ok(history.points.length >= 3);
    for (const range of ['30d', '90d']) {
      const hourly = await (await api(fixturePath + '?range=' + range, 200, { headers: admin })).json();
      assert.equal(hourly.points.length, 1);
      assert.equal(hourly.points[0].rx_bytes, 800, 'Dirty hour must be rebuilt after the late replacement');
      assert.equal(hourly.points[0].cpu_avg, 30);
      assert.equal(hourly.points[0].cpu_max, 55);
      assert.equal(hourly.points[0].rx_bps_max, 15);
    }
    const fixtureStatus = (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === collision.server.id);
    const inMonth = fixtureHour >= fixtureStatus.traffic_month.since_ms;
    assert.equal(fixtureStatus.traffic_month.rx_bytes, inMonth ? 800 : 0);
    const checked = editStorage(`
import pathlib, sqlite3, sys
found = False
for file in pathlib.Path(sys.argv[1]).rglob('*.sqlite'):
    db = sqlite3.connect(f'file:{file}?mode=ro', uri=True)
    if not db.execute("SELECT name FROM sqlite_master WHERE name='metrics_1m'").fetchone():
        db.close(); continue
    found = True
    for table in ('metrics_1m', 'metrics_1h'):
        assert db.execute(f"SELECT COUNT(*) FROM {table} WHERE ts_ms=0").fetchone()[0] == 0
    assert db.execute("SELECT COUNT(*) FROM dirty_hours").fetchone()[0] == 0
    db.close()
assert found
print('hourly aggregation, dirty hours and per-server retention passed')
`);
    console.log(checked);
    console.log(`Restart passed: ${history.points.length} continuous native minute buckets`);
    const offlineRule = defaultRules.find(r => r.metric === 'offline');
    await patchRule(offlineRule.id, { duration_s: 5, server_id: server.id });
    dash.events.length = 0;
    agent.kill('SIGSTOP');
    await event(dash, 'offline', e => e.id === server.id, 46_000);
    const offline = (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === server.id);
    assert.equal(offline.online, false);
    assert.equal(offline.metrics, null);
    await event(dash, 'alert', e => e.kind === 'firing' && e.alert.rule_id === offlineRule.id, 5000);
    agent.kill('SIGCONT');
    await event(dash, 'online', e => e.id === server.id, 15_000);
    await event(dash, 'alert', e => e.kind === 'resolved' && e.alert.rule_id === offlineRule.id);
    console.log('SIGSTOP passed: offline alert fired, Agent reconnected and alert resolved');
  }
  const agentExited = once(agent, 'exit');
  const rotated = await (await api(`/api/servers/${server.id}/token`, 200, { method: 'POST', headers: admin })).json();
  assert.ok(rotated.token !== token, 'Token rotation must issue a new credential');
  assert.equal((await agentExited)[0], 3, 'Agent must exit with code 3 after token revocation (WS 4003)');
  await event(dash, 'offline', e => e.id === server.id);
  await rejectedUpgrade(token);
  const renewed = socket('/ws/agent', { Authorization: `Bearer ${rotated.token}` });
  await event(renewed, 'welcome');
  const replacement = socket('/ws/agent', { Authorization: `Bearer ${rotated.token}` });
  await event(replacement, 'welcome');
  assert.equal(await renewed.closed, 4002);
  replacement.ws.send(JSON.stringify({ type: 'hello', protocol: 0, agent_version: 'old', host: {
    hostname: 'old', os: 'linux', kernel: 'test', arch: 'x86_64', cpu_model: 'test', cpu_cores: 1, boot_time_ms: 0,
  }}));
  assert.equal(await replacement.closed, 4001);
  const finalAgent = socket('/ws/agent', { Authorization: `Bearer ${rotated.token}` });
  await event(finalAgent, 'welcome');
  finalAgent.ws.send(JSON.stringify({ type: 'rollup', minute_ms: 60_000, samples: 1, cpu_avg: 0, cpu_max: 0,
    mem_used_avg: 0, mem_total: 0, swap_used_avg: 0, swap_total: 0, disk_used: 0, disk_total: 0,
    rx_bytes: 0, tx_bytes: 0, rx_bps_max: 0, tx_bps_max: 0, load1_avg: 0 }));
  assert.equal((await event(finalAgent, 'ack')).minute_ms, 60_000);
  await api(`/api/servers/${server.id}`, 204, { method: 'DELETE', headers: admin });
  assert.equal(await finalAgent.closed, 4003);
  await event(dash, 'server_removed', e => e.id === server.id);
  await rejectedUpgrade(rotated.token);
  await api(`/api/servers/${collision.server.id}`, 204, { method: 'DELETE', headers: admin });
  assert.deepEqual(await (await api('/api/servers', 200, { headers: admin })).json(), []);
  const preserved = (await (await api('/api/alerts/events?limit=200', 200, { headers: admin })).json()).find(e => e.rule_id === rule.id);
  assert.equal(preserved.rule_name, rule.name);
  assert.equal(preserved.server_name, 'E2E VPS');
  await api(`/api/servers/${server.id}`, 404, { method: 'DELETE', headers: admin });
  await api('/api/debug', 404, { headers: admin });
  const logout = await api('/api/auth/logout', 204, { method: 'POST', headers: { Cookie } });
  assert.match(logout.headers.get('set-cookie'), /Max-Age=0/);
  // Keep the failures together: hibernation is allowed to discard the in-memory limiter.
  const failures = await Promise.all(Array.from({ length: 11 }, async () => {
    const response = await fetch(base + '/api/auth/login', { method: 'POST',
      headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password: 'wrong' }) });
    assert.ok([401, 429].includes(response.status));
    return response.status;
  }));
  assert.ok(failures.includes(429), 'Consecutive login failures must be rate limited');
  // Exercise outbound delivery locally, without real credentials or external services.
  const notices = [];
  let webhookMode = 'ok';
  webhook = createServer((req, res) => {
    let body = '';
    req.on('data', chunk => { body += chunk; });
    req.on('end', () => {
      notices.push(JSON.parse(body));
      if (webhookMode === 'hang') return;
      res.writeHead(webhookMode === 'error' ? 503 : 200);
      res.end('ok');
    });
  });
  await new Promise(resolve => webhook.listen(0, '127.0.0.1', resolve));
  await stopWorker();
  editStorage(`
import pathlib, sqlite3, sys
for file in pathlib.Path(sys.argv[1]).rglob('*.sqlite'):
    db = sqlite3.connect(file)
    if not db.execute("SELECT name FROM sqlite_master WHERE name='alert_events'").fetchone():
        db.close(); continue
    for column in ('rule_name', 'server_name', 'notification_json'):
        db.execute(f"ALTER TABLE alert_events DROP COLUMN {column}")
    db.execute("UPDATE meta SET v='1' WHERE k='schema_version'")
    db.execute("INSERT INTO alert_events (rule_id,server_id,kind,value,ts_ms) VALUES ('legacy-rule','legacy-server','firing',1,1)")
    db.commit(); db.close()
`);
  const configured = JSON.parse(fs.readFileSync(path.join(temp, 'config.json'), 'utf8'));
  configured.vars.WEBHOOK_URL = `http://127.0.0.1:${webhook.address().port}/notify`;
  fs.writeFileSync(path.join(temp, 'config.json'), JSON.stringify(configured));
  await startWorker();
  assert.ok(!(await (await api(ruleUrl, 200, { headers: admin })).json()).some(r => r.id === removedDefault));
  const migratedEvents = await (await api('/api/alerts/events?limit=1', 200, { headers: admin })).json();
  assert.equal(migratedEvents[0].rule_name, 'legacy-rule');
  assert.equal(migratedEvents[0].server_name, 'legacy-server');
  assert.deepEqual(await (await api('/api/alerts/test', 200, { method: 'POST', headers: admin })).json(),
    { telegram: 'disabled', webhook: 'ok' });
  assert.deepEqual(Object.keys(notices[0]).sort(), ['event', 'rule', 'server', 'ts_ms', 'value']);
  webhookMode = 'error';
  assert.equal((await (await api('/api/alerts/test', 200, { method: 'POST', headers: admin })).json()).webhook, 'HTTP 503');
  webhookMode = 'hang';
  const timeoutStarted = Date.now();
  assert.equal((await (await api('/api/alerts/test', 200, { method: 'POST', headers: admin })).json()).webhook, 'timeout');
  assert.ok(Date.now() - timeoutStarted >= 4800 && Date.now() - timeoutStarted < 7000);
  const notifyServer = await (await api('/api/servers', 201, { method: 'POST', headers: admin, body: { id: 'notify', name: '通知服务器' } })).json();
  const notifyAgent = socket('/ws/agent', { Authorization: `Bearer ${notifyServer.token}` });
  await event(notifyAgent, 'welcome');
  const notifyDash = socket('/ws/dashboard', { Cookie });
  await event(notifyDash, 'snapshot');
  const notifyRules = [];
  for (let i = 0; i < 26; i++) {
    notifyRules.push(await (await api(ruleUrl, 201, { method: 'POST', headers: admin,
      body: { ...ruleInput, server_id: 'notify', name: `通知规则 ${i}` } })).json());
  }
  const messageStarted = Date.now();
  notifyAgent.ws.send(JSON.stringify(cpuFrame));
  await event(notifyDash, 'alert', e => e.kind === 'firing' && e.alert.rule_id === notifyRules[25].id);
  assert.ok(Date.now() - messageStarted < 3000, 'Slow notifications must not block Metrics handling');
  webhookMode = 'ok';
  // A later invocation drains the two transitions beyond the 24-transition notification batch.
  assert.equal((await (await api('/api/alerts/active', 200, { headers: admin })).json()).length, 26);
  const waitNotices = async (kind, count) => {
    for (let i = 0; i < 100; i++) {
      if (notices.filter(n => n.event === kind && n.server.id === 'notify').length === count) return;
      await pause(50);
    }
    throw new Error(`Missing ${kind} webhook notifications`);
  };
  await waitNotices('firing', 26);
  const delivered = notices.find(n => n.event === 'firing');
  assert.equal(delivered.server.name, '通知服务器');
  assert.equal(delivered.value, 50);
  assert.ok(delivered.rule.name.startsWith('通知规则'));
  await api('/api/servers/notify', 204, { method: 'DELETE', headers: admin });
  await api('/api/alerts/events', 200, { headers: admin });
  await waitNotices('resolved', 26);
  assert.equal(new Set(notices.filter(n => n.event === 'firing').map(n => n.rule.id)).size, 26);
  console.log('Notifications passed: disabled/test, Webhook JSON, HTTP failure, 5s timeout, nonblocking handling and 26-transition batches');
  console.log(`e2e passed: auth, config, CRUD, token rejection/revocation, dashboard, alerts, history/rollup (${quick ? "quick; long scenarios skipped" : "full; restart and SIGSTOP"}); ${((Date.now() - e2eStartedMs) / 1000).toFixed(1)}s`);
} finally {
  clearTimeout(timeout);
  stopChildren();
}
JS
