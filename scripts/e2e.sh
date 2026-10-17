#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
worker_dir="$repo_dir/crates/worker"
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/moni-e2e.XXXXXXXX")
wrangler_pid=
cleanup() {
  if [[ -n "$wrangler_pid" ]]; then
    kill "$wrangler_pid" 2>/dev/null || true
    wait "$wrangler_pid" 2>/dev/null || true
  fi
  rm -rf -- "$test_dir"
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

if [[ "${E2E_AGENT:-auto}" != mock ]]; then
  if cargo build -p moni-agent >"$test_dir/agent-build.log" 2>&1; then
    agent_mode=native
  else
    echo 'Agent build failed; using a minimal WebSocket agent for B3.'
    agent_mode=mock
  fi
else
  agent_mode=mock
fi

port=${E2E_PORT:-$(node -e 'const s=require("node:net").createServer();s.listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')}
node --input-type=module - "$worker_dir" "$test_dir/config.json" <<'JS'
import fs from 'node:fs';
import path from 'node:path';
const [worker, target] = process.argv.slice(2);
const config = JSON.parse(fs.readFileSync(path.join(worker, 'wrangler.jsonc'), 'utf8').replace(/^\s*\/\/.*$/gm, ''));
delete config.$schema;
config.main = path.join(worker, config.main);
config.assets.directory = path.resolve(worker, config.assets.directory);
config.build.cwd = worker;
config.vars.ADMIN_PASSWORD = 'e2e-admin-password';
config.vars.AUTH_SECRET = 'e2e-signing-secret-at-least-32-bytes-long';
fs.writeFileSync(target, JSON.stringify(config));
JS

CLOUDFLARE_SEND_METRICS=false "$repo_dir/node_modules/.bin/wrangler" dev \
  --config "$test_dir/config.json" --local --ip 127.0.0.1 --port "$port" \
  --inspector-port 0 --persist-to "$test_dir/state" >"$test_dir/wrangler.log" 2>&1 &
wrangler_pid=$!
ready=false
for ((attempt = 0; attempt < 180; attempt++)); do
  if curl --silent --fail "http://127.0.0.1:$port/api/health" > /dev/null; then
    ready=true
    break
  fi
  if ! kill -0 "$wrangler_pid" 2>/dev/null; then
    cat "$test_dir/wrangler.log" >&2
    exit 1
  fi
  sleep 1
done
if [[ "$ready" != true ]]; then
  cat "$test_dir/wrangler.log" >&2
  echo 'Wrangler did not become ready' >&2
  exit 1
fi

node --input-type=module - "$port" "$repo_dir" "$agent_mode" "$test_dir" <<'JS'
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
const [port, repo, mode, temp] = process.argv.slice(2);
const WS = globalThis.WebSocket;
const base = `http://127.0.0.1:${port}`;
const password = 'e2e-admin-password';
const admin = { Authorization: `Bearer ${password}` };
const sockets = [];
let agent;
const timeout = setTimeout(() => { console.error('e2e timed out'); process.exitCode = 1; agent?.kill(); for (const ws of sockets) ws.close(); }, 60_000);
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
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
async function event(client, type, matches = () => true) {
  for (let i = 0; i < 200; i++) {
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
  const dash = socket('/ws/dashboard', { Cookie });
  assert.deepEqual((await event(dash, 'snapshot')).alerts, []);
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
  await rejectedUpgrade(`mn_${server.id}_${Buffer.alloc(32, 1).toString('base64url')}`, { 'x-moni-server-id': server.id, 'x-moni-token-hash': 'forged' });
  await rejectedUpgrade('bad-token', { 'x-moni-server-id': server.id, 'x-moni-token-hash': 'forged' });
  let mock;
  if (mode === 'native') {
    agent = spawn(path.join(repo, 'target/debug/moni-agent'), [], {
      env: { ...process.env, MONI_SERVER: base.replace('http:', 'ws:') + '/ws/agent', MONI_TOKEN: token },
      stdio: ['ignore', fs.openSync(path.join(temp, 'agent.log'), 'w'), 'pipe'],
    });
  } else {
    mock = socket('/ws/agent', { Authorization: `Bearer ${token}` });
    const welcome = await event(mock, 'welcome');
    assert.equal(welcome.server_id, server.id);
    mock.ws.send(JSON.stringify({ type: 'hello', protocol: 2, agent_version: 'e2e', host: {
      hostname: 'e2e', os: 'linux', kernel: 'test', arch: 'x86_64', cpu_model: 'test', cpu_cores: 1, boot_time_ms: Date.now() - 10_000,
    }}));
    mock.ws.send(JSON.stringify({ type: 'metrics', seq: 1, ts_ms: Date.now(), cpu_pct: 1,
      mem_used: 1, mem_total: 2, swap_used: 0, swap_total: 0, disk_used: 1, disk_total: 2,
      net_rx_bps: 1, net_tx_bps: 1, load1: 0, load5: 0, load15: 0, uptime_s: 10, process_count: 1 }));
  }
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
  const agentExited = agent && new Promise(resolve => agent.once('exit', code => resolve(code)));
  const rotated = await (await api(`/api/servers/${server.id}/token`, 200, { method: 'POST', headers: admin })).json();
  assert.ok(rotated.token !== token, 'Token rotation must issue a new credential');
  if (mock) assert.equal(await mock.closed, 4003);
  else assert.equal(await agentExited, 3, 'Agent must exit with code 3 after token revocation (WS 4003)');
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
  console.log(`e2e passed: auth, config, CRUD, token rejection/revocation, dashboard online/metrics (${mode} agent)`);
} finally {
  clearTimeout(timeout);
  agent?.kill();
  for (const ws of sockets) if (ws.readyState === 1) ws.close();
}
JS
