#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
worker_dir="$repo_dir/crates/worker"
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/moni-e2e.XXXXXXXX")
quick=false
ui=false
for arg in "$@"; do
  case "$arg" in
    --quick) quick=true ;;
    --ui) ui=true ;;
    *) echo "Usage: $0 [--quick] [--ui]" >&2; exit 2 ;;
  esac
done
cleanup() {
  local exit_status=$?
  if [[ "${E2E_KEEP:-0}" == 1 || "$exit_status" != 0 ]]; then
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
# Serialize builds, then snapshot their outputs: concurrent runs must not overwrite live Wasm/assets.
exec 9>"${TMPDIR:-/tmp}/moni-e2e-build-$(id -u).lock"
flock 9
npm --prefix web run build >"$test_dir/web-build.log" 2>&1 || {
  cat "$test_dir/web-build.log" >&2; exit 1;
}
cargo run -q -p moni-protocol --example patch-fixture >"$test_dir/patch.json"
cargo build -p moni-agent >"$test_dir/agent-build.log" 2>&1 || {
  cat "$test_dir/agent-build.log" >&2; exit 1;
}
(cd "$worker_dir" && worker-build --release) >"$test_dir/worker-build.log" 2>&1 || {
  cat "$test_dir/worker-build.log" >&2; exit 1;
}

cp -R "$worker_dir/build" "$test_dir/build"
cp -R web/dist "$test_dir/assets"
cp target/debug/moni-agent "$test_dir/moni-agent"
cp scripts/e2e-worker.mjs "$test_dir/worker.mjs"
flock -u 9
exec 9>&-

port=${E2E_PORT:-0}
node --input-type=module - "$worker_dir" "$test_dir/config.json" <<'JS'
import fs from 'node:fs';
import path from 'node:path';
const [worker, target] = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(path.join(worker, 'wrangler.jsonc'), 'utf8').replace(/^\s*\/\/.*$/gm, ''));
delete config.$schema;
config.name = path.basename(path.dirname(target)).replace(/[^a-z0-9-]/gi, '-').toLowerCase();
config.main = path.join(path.dirname(target), "worker.mjs");
config.assets.directory = path.join(path.dirname(target), "assets");
delete config.build;
config.vars.E2E_RUN_ID = path.basename(path.dirname(target));
config.vars.ADMIN_PASSWORD = 'e2e-admin-password';
config.vars.AUTH_SECRET = 'e2e-signing-secret-at-least-32-bytes-long';
fs.writeFileSync(target, JSON.stringify(config));
JS

node --input-type=module - "$port" "$repo_dir" "$quick" "$test_dir" "$ui" <<'JS'
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createServer } from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
const [initialPort, repo, quickFlag, temp, uiFlag] = process.argv.slice(2);
let port = initialPort;
const quick = quickFlag === "true";
const e2eStartedMs = Date.now();
const WS = globalThis.WebSocket;
let base = `http://127.0.0.1:${port}`;
const password = 'e2e-admin-password';
const admin = { Authorization: `Bearer ${password}` };
const sockets = [];
let agent, wrangler, webhook, uiSmoke;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const timeout = setTimeout(() => { console.error('e2e timed out'); stopChildren(); process.exit(1); }, 600_000);
function stopChildren() {
  webhook?.closeAllConnections();
  webhook?.close();
  agent?.kill('SIGCONT');
  agent?.kill('SIGTERM');
  uiSmoke?.kill('SIGTERM');
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
    env: { ...process.env, CLOUDFLARE_SEND_METRICS: 'false',
      WRANGLER_REGISTRY_PATH: path.join(temp, 'registry') },
    detached: true, stdio: ['ignore', log, log],
  });
  fs.closeSync(log);
  const deadline = Date.now() + 120_000;
  let readySince;
  while (Date.now() < deadline) {
    const ready = fs.readFileSync(path.join(temp, 'wrangler.log'), 'utf8').match(/Ready on http:\/\/127\.0\.0\.1:(\d+)/g)?.at(-1);
    if (port === '0' && ready) {
      port = ready.split(':').at(-1);
      base = `http://127.0.0.1:${port}`;
    }
    try {
      const response = await fetch(base + '/api/health', { signal: AbortSignal.timeout(2000) });
      const healthy = response.ok && response.headers.get('x-e2e-run') === path.basename(temp);
      await response.arrayBuffer();
      const hub = await fetch(base + '/api/servers', { headers: admin, signal: AbortSignal.timeout(2000) });
      const hubReady = hub.ok && Array.isArray(await hub.json());
      // The proxy can serve health before the initial runtime/binding updates have settled.
      if (healthy && hubReady) {
        readySince ??= Date.now();
        if (Date.now() - readySince >= 1000) return;
      } else readySince = undefined;
    } catch { readySince = undefined; }
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
async function poll(check, label, waitMs = 60_000, intervalMs = 100) {
  const deadline = Date.now() + waitMs;
  while (Date.now() < deadline) {
    const result = await check();
    if (result) return result;
    await pause(intervalMs);
  }
  throw new Error(`Timed out waiting for ${label}`);
}
async function api(url, status = 200, { method = 'GET', headers = {}, body } = {}) {
  const response = await fetch(base + url, {
    signal: AbortSignal.timeout(15_000), method,
    headers: { ...(body === undefined ? {} : { 'Content-Type': 'application/json' }), ...headers },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const failure = response.status >= 500 ? await response.clone().text() : '';
  assert.equal(response.status, status, `${method} ${url}: ${response.status} ${failure}`);
  return response;
}
function socket(url, headers) {
  const ws = new WS(base.replace('http:', 'ws:') + url, { headers: { ...(headers?.Cookie ? { Origin: base } : {}), ...headers } });
  sockets.push(ws);
  const events = [];
  ws.addEventListener('message', event => events.push(JSON.parse(event.data)));
  const closed = new Promise(resolve => ws.addEventListener('close', event => resolve(event.code)));
  return { ws, events, closed };
}
async function hello(client, protocol = 3) {
  if (client.ws.readyState === WS.CONNECTING) await once(client.ws, 'open');
  client.ws.send(JSON.stringify({ type: 'hello', protocol, agent_version: '0.1.0', host: {
    hostname: 'fixture', os: 'Linux', kernel: 'test', arch: 'x86_64',
    cpu_model: 'test', cpu_cores: 1, boot_time_ms: 0,
  }}));
}
async function event(client, type, matches = () => true, waitMs = 30_000) {
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
    const req = request(base + '/ws/agent', { agent: false, headers: {
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
async function rejectedDashboard(headers) {
  const { request } = await import('node:http');
  await new Promise((resolve, reject) => {
    const req = request(base + '/ws/dashboard', { agent: false, headers: {
      ...headers, Upgrade: 'websocket', Connection: 'Upgrade',
      'Sec-WebSocket-Version': '13', 'Sec-WebSocket-Key': 'MDEyMzQ1Njc4OWFiY2RlZg==',
    }}, response => {
      response.resume();
      response.on('end', () => {
        try { assert.equal(response.statusCode, 403); resolve(); } catch (error) { reject(error); }
      });
    });
    req.on('upgrade', (_response, connection) => { connection.destroy(); reject(new Error('Foreign dashboard Origin accepted')); });
    req.on('error', reject);
    req.end();
  });
}
try {
  await startWorker();
  if (uiFlag === 'true') {
    uiSmoke = spawn('npm', ['--prefix', 'web', 'run', 'test:smoke'], {
      cwd: repo, env: { ...process.env, MONI_BASE: base, MONI_PASSWORD: password },
      stdio: 'inherit',
    });
    assert.equal((await once(uiSmoke, 'exit'))[0], 0, 'UI smoke failed');
    uiSmoke = undefined;
  }
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
  for (const Origin of [undefined, 'null', 'https://evil.example.com', 'http://localhost:' + port]) {
    const headers = { Cookie, ...(Origin === undefined ? {} : { Origin }) };
    await api('/api/servers', 403, { method: 'POST', headers, body: { name: 'CSRF' } });
    await rejectedDashboard(headers);
  }
  await api('/api/auth/login', 403, { method: 'POST', headers: { Origin: 'null' }, body: { password } });
  await api('/api/auth/login', 403, { method: 'POST', headers: { Origin: 'https://evil.example.com' }, body: { password } });
  await api('/api/auth/login', 204, { method: 'POST', headers: { Origin: base }, body: { password } });
  await api('/api/servers', 415, { method: 'POST', headers: { Cookie, Origin: base, 'Content-Type': 'text/plain' }, body: { name: 'CSRF' } });
  await api('/api/auth/login', 415, { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: { password } });
  await api('/api/servers', 415, { method: 'POST', headers: { ...admin, 'Content-Type': 'text/plain' }, body: { name: 'CLI' } });
  for (const framing of [{}, { 'Content-Length': '0' }]) {
    const headers = { Cookie, Origin: base, ...framing };
    const sameOrigin = await (await api('/api/servers', 201, { method: 'POST',
      headers: { Cookie, Origin: base }, body: { name: 'same-origin' } })).json();
    const rule = await (await api('/api/alerts/rules', 201, { method: 'POST',
      headers: { Cookie, Origin: base }, body: { name: 'CSRF', server_id: sameOrigin.server.id,
        metric: 'cpu', op: 'gt', threshold: 100, recover_threshold: null, duration_s: 0, enabled: false } })).json();
    const writes = [
      ['POST', `/api/servers/${sameOrigin.server.id}/token`, 200],
      ['POST', '/api/alerts/test', 200],
      ['DELETE', `/api/alerts/rules/${rule.id}`, 204],
      ['DELETE', `/api/servers/${sameOrigin.server.id}`, 204],
      ['POST', '/api/auth/logout', 204],
    ];
    for (const [method, url] of writes) {
      for (const mediaType of [{}, { 'Content-Type': 'text/plain' }]) {
        const blocked = await api(url, 403, { method,
          headers: { ...headers, Origin: 'https://evil.example.com', ...mediaType } });
        assert.equal((await blocked.json()).error, 'forbidden');
      }
    }
    for (const [method, url, status] of writes) await api(url, status, { method, headers });
  }
  const cliDash = socket('/ws/dashboard', admin);
  await event(cliDash, 'snapshot');
  cliDash.ws.close();
  console.log('CSRF passed: five bodyless Cookie writes, absent/zero Content-Length, foreign Origin with/without text/plain, JSON media type and Bearer CLI');
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
  let fixture = socket('/ws/agent', { Authorization: `Bearer ${collision.token}` });
  await hello(fixture);
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
  const anonAlerts = socket('/ws/dashboard');
  await event(anonAlerts, 'snapshot');
  fixture.ws.send(JSON.stringify(cpuFrame));
  const firing = await alertEvent('firing');
  assert.equal(firing.alert.value, 50);
  // Alert state is operator configuration: public sockets receive neither events nor counts.
  await event(anonAlerts, 'metrics', e => e.id === collision.server.id);
  assert.ok(anonAlerts.events.every(e => e.type !== 'alert'));
  const anonAlertSnapshot = await event(socket('/ws/dashboard'), 'snapshot');
  assert.deepEqual(anonAlertSnapshot.alerts, []);
  assert.equal(anonAlertSnapshot.servers.find(s => s.id === collision.server.id).alerts_firing, 0);
  anonAlerts.ws.close();
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
  fixture.ws.close();
  const { regressions } = await import(path.join(repo, 'scripts/e2e-regressions.mjs'));
  await regressions({ api, admin, socket, hello, event, poll, base,
    patches: JSON.parse(fs.readFileSync(path.join(temp, 'patch.json'), 'utf8')), cpuFrame });
  fixture = socket('/ws/agent', { Authorization: `Bearer ${collision.token}` });
  await hello(fixture);
  await event(fixture, 'welcome');
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
  await poll(() => fs.readFileSync(path.join(temp, 'wrangler.log'), 'utf8').includes(`Invalid Agent message server=${collision.server.id}:`), 'parse warning');
  const warnings = fs.readFileSync(path.join(temp, 'wrangler.log'), 'utf8');
  assert.ok(!warnings.includes('mn_DO_NOT_LOG_THIS_TOKEN'));
  assert.equal(warnings.split(`Invalid Agent message server=${collision.server.id}:`).length - 1, 1);
  fixture.ws.close();
  await rejectedUpgrade(`mn_${server.id}_${Buffer.alloc(32, 1).toString('base64url')}`, { 'x-moni-server-id': server.id, 'x-moni-token-hash': 'forged' });
  await rejectedUpgrade('bad-token', { 'x-moni-server-id': server.id, 'x-moni-token-hash': 'forged' });
  const agentLog = fs.openSync(path.join(temp, 'agent.log'), 'w');
  const startedMs = Date.now();
  agent = spawn(path.join(temp, 'moni-agent'), [], {
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
  // Anonymous visitors get a redacted, read-only view; everything else stays 401.
  assert.deepEqual(await (await api('/api/public')).json(), { enabled: true });
  for (const url of ['/api/servers', '/api/config', '/api/alerts/rules', '/api/alerts/active', '/api/alerts/events'])
    await api(url, 401);
  await api(`/api/servers/${server.id}`, 401, { method: 'PATCH', headers: { Origin: base }, body: { name: 'anon' } });
  await api(`/api/servers/${server.id}/token`, 401, { method: 'POST', headers: { Origin: base } });
  await api(`/api/servers/${server.id}/history?range=bad`, 400);
  const publicHistory = await api(`/api/servers/${server.id}/history?range=1h`, 200);
  assert.match(publicHistory.headers.get('cache-control') ?? '', /public, max-age=60/);
  assert.equal((await publicHistory.json()).range, '1h');
  for (const headers of [{}, { 'x-moni-admin': '1' }]) {
    const anon = socket('/ws/dashboard', headers);
    const snapshot = await event(anon, 'snapshot');
    assert.deepEqual(snapshot.alerts, []);
    const visible = snapshot.servers.find(s => s.id === server.id);
    assert.equal(visible.agent_version, null);
    assert.equal(visible.host.hostname, '');
    assert.equal(visible.host.kernel, '');
    assert.ok(visible.host.os);
    assert.ok('country' in visible);
    await event(anon, 'metrics', e => e.id === server.id);
    anon.ws.close();
  }
  console.log('Public view passed: redacted snapshot, forged admin header ignored, read-only history, admin APIs 401');
  const renamed = await (await api(`/api/servers/${server.id}`, 200, { method: 'PATCH', headers: admin, body: { name: 'Renamed' } })).json();
  assert.equal(renamed.name, 'Renamed');
  await event(dash, 'server_upsert', e => e.server.name === 'Renamed');
  if (!quick) {
    console.log('Waiting for two persisted native Agent rollups...');
    let history = await poll(async () => {
      const result = await (await api(`/api/servers/${server.id}/history?range=1h`, 200, { headers: admin })).json();
      return result.points.length >= 2 && result;
    }, 'two native minute rollups', 180_000, 1000);
    const status = (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === server.id);
    if (history.points.some(p => p.ts_ms >= status.traffic_month.since_ms && p.rx_bytes + p.tx_bytes > 0)) {
      assert.ok(status.traffic_month.rx_bytes + status.traffic_month.tx_bytes > 0);
    }
    const freshDash = socket('/ws/dashboard', { Cookie });
    const freshTraffic = (await event(freshDash, 'snapshot')).servers.find(s => s.id === server.id).traffic_month;
    // Another native Rollup may arrive between the REST response and the snapshot.
    assert.ok(freshTraffic.since_ms >= status.traffic_month.since_ms);
    if (freshTraffic.since_ms === status.traffic_month.since_ms) {
      assert.ok(freshTraffic.rx_bytes >= status.traffic_month.rx_bytes);
      assert.ok(freshTraffic.tx_bytes >= status.traffic_month.tx_bytes);
    }
    const late = socket('/ws/agent', { Authorization: `Bearer ${collision.token}` });
    await hello(late);
    await event(late, 'welcome');
    late.ws.send(JSON.stringify(rollup(fixtureHour + 120_000, 30, 400, 15)));
    await event(late, 'ack');
    console.log('Killing Wrangler for 62s while Agent keeps sampling and queues rollups...');
    await stopWorker();
    // Force maintenance on restart and add expired rows; SQL edits happen only with workerd stopped.
    editStorage(`
import pathlib, sqlite3, sys
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
    // A full minute of deliberate downtime exercises the Agent buffer.
    await pause(62_000);
    await startWorker();
    dash = socket('/ws/dashboard', { Cookie });
    await event(dash, 'snapshot');
    assert.ok(!(await (await api(ruleUrl, 200, { headers: admin })).json()).some(r => r.id === removedDefault), 'Deleted default must stay deleted after restart');
    await event(dash, 'metrics', e => e.id === server.id, 90_000);
    // Observe the completed maintenance instead of assuming an alarm fires within 31 seconds.
    await poll(async () => {
      const result = await (await api(fixturePath + '?range=30d', 200, { headers: admin })).json();
      return result.points.some(p => p.rx_bytes === 800);
    }, 'hourly rebuild after restart', 90_000, 500);
    await poll(() => /resent pending rollups count=[1-9]/.test(fs.readFileSync(path.join(temp, 'agent.log'), 'utf8')),
      'Agent replay after restart');
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
    await event(dash, 'offline', e => e.id === server.id, 90_000);
    const offline = (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === server.id);
    assert.equal(offline.online, false);
    assert.equal(offline.metrics, null);
    await event(dash, 'alert', e => e.kind === 'firing' && e.alert.rule_id === offlineRule.id, 60_000);
    agent.kill('SIGCONT');
    await event(dash, 'online', e => e.id === server.id, 90_000);
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
  await hello(renewed);
  await event(renewed, 'welcome');
  const replacement = socket('/ws/agent', { Authorization: `Bearer ${rotated.token}` });
  await hello(replacement, 0);
  assert.equal(await renewed.closed, 4002);
  assert.equal(await replacement.closed, 4001);
  const finalAgent = socket('/ws/agent', { Authorization: `Bearer ${rotated.token}` });
  await hello(finalAgent);
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
  assert.deepEqual(await (await api('/api/alerts/events?limit=200', 200, { headers: admin })).json(), []);
  await api(`/api/servers/${server.id}`, 404, { method: 'DELETE', headers: admin });
  await api('/api/debug', 404, { headers: admin });
  const logout = await api('/api/auth/logout', 204, { method: 'POST', headers: { Cookie, Origin: base } });
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
  const configured = JSON.parse(fs.readFileSync(path.join(temp, 'config.json'), 'utf8'));
  configured.vars.WEBHOOK_URL = `http://127.0.0.1:${webhook.address().port}/notify`;
  fs.writeFileSync(path.join(temp, 'config.json'), JSON.stringify(configured));
  await startWorker();
  assert.ok(!(await (await api(ruleUrl, 200, { headers: admin })).json()).some(r => r.id === removedDefault));
  const eventsBeforeNotificationTest = await (await api('/api/alerts/events', 200, { headers: admin })).json();
  assert.deepEqual(await (await api('/api/alerts/test', 200, { method: 'POST', headers: admin })).json(),
    { telegram: 'disabled', webhook: 'ok' });
  assert.deepEqual(Object.keys(notices[0]).sort(), ['event', 'rule', 'server', 'ts_ms', 'value']);
  webhookMode = 'error';
  assert.equal((await (await api('/api/alerts/test', 200, { method: 'POST', headers: admin })).json()).webhook, 'HTTP 503');
  webhookMode = 'hang';
  const timeoutStarted = Date.now();
  assert.equal((await (await api('/api/alerts/test', 200, { method: 'POST', headers: admin })).json()).webhook, 'timeout');
  assert.ok(Date.now() - timeoutStarted >= 4800 && Date.now() - timeoutStarted < 15_000);
  const notifyServer = await (await api('/api/servers', 201, { method: 'POST', headers: admin, body: { id: 'notify', name: '通知服务器' } })).json();
  const notifyAgent = socket('/ws/agent', { Authorization: `Bearer ${notifyServer.token}` });
  await hello(notifyAgent);
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
  assert.ok(Date.now() - messageStarted < 4800, 'Slow notifications must not block Metrics handling');
  webhookMode = 'ok';
  // A later invocation drains the two transitions beyond the 24-transition notification batch.
  assert.equal((await (await api('/api/alerts/active', 200, { headers: admin })).json()).length, 26);
  const waitNotices = async (kind, count) => {
    await poll(() => notices.filter(n => n.event === kind && n.server.id === 'notify').length === count,
      `${kind} webhook notifications`);
  };
  await waitNotices('firing', 26);
  const delivered = notices.find(n => n.event === 'firing');
  assert.equal(delivered.server.name, '通知服务器');
  assert.equal(delivered.value, 50);
  assert.ok(delivered.rule.name.startsWith('通知规则'));
  await api('/api/servers/notify', 204, { method: 'DELETE', headers: admin });
  await api('/api/alerts/events', 200, { headers: admin });
  assert.equal(notices.filter(n => n.event === 'resolved' && n.server.id === 'notify').length, 0);
  assert.deepEqual(await (await api('/api/alerts/events', 200, { headers: admin })).json(), eventsBeforeNotificationTest);
  assert.equal(new Set(notices.filter(n => n.event === 'firing').map(n => n.rule.id)).size, 26);
  console.log('Notifications passed: disabled/test, Webhook JSON, HTTP failure, 5s timeout, nonblocking handling and 26-transition batches');
  console.log(`e2e passed: auth, config, CRUD, token rejection/revocation, dashboard, alerts, history/rollup (${quick ? "quick; long scenarios skipped" : "full; restart and SIGSTOP"}); ${((Date.now() - e2eStartedMs) / 1000).toFixed(1)}s`);
} finally {
  clearTimeout(timeout);
  stopChildren();
}
JS
