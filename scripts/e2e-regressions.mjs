import assert from 'node:assert/strict';

export async function regressions({ api, admin, socket, event, poll, base, patches, cpuFrame }) {
  const test = (action, body = {}) => api('/__test/' + action, 200,
    { method: 'POST', body }).then(r => r.json());
  const sql = (query, args = []) => test('sql', { sql: query, args });
  const stats = () => test('stats');
  const create = id => api('/api/servers', 201, { method: 'POST', headers: admin,
    body: { id, name: id } }).then(r => r.json());
  const current = await create('regression');
  const other = await create('other-regression');
  const dash = socket('/ws/dashboard', admin);
  await event(dash, 'snapshot');
  let agent = socket('/ws/agent', { Authorization: `Bearer ${current.token}` });
  await event(agent, 'welcome');
  let seq = 100;
  async function metrics() {
    const message = { ...cpuFrame, seq: seq++ };
    agent.ws.send(JSON.stringify(message));
    await event(dash, 'metrics', e => e.id === 'regression' && e.frame.seq === message.seq);
    return poll(async () => {
      const result = await stats();
      return result.last.message?.seq === message.seq && result;
    }, 'Metrics SQL counters');
  }
  const ruleInput = { name: 'Rust PATCH', metric: 'cpu', server_id: 'regression', op: 'gt',
    threshold: 100, recover_threshold: 20, duration_s: 0, enabled: true };
  const rule = await (await api('/api/alerts/rules', 201, { method: 'POST', headers: admin, body: ruleInput })).json();
  for (const patch of patches) {
    const result = await (await api('/api/alerts/rules/' + rule.id, 200,
      { method: 'PATCH', headers: admin, body: patch })).json();
    for (const [key, value] of Object.entries(patch)) assert.equal(result[key], value);
  }
  await api('/api/alerts/rules/' + rule.id, 204, { method: 'DELETE', headers: admin });
  await metrics(); // Load the rule cache after the admin edit.
  const baseline = (await metrics()).last.rowsRead;
  const now = Date.now();
  for (let i = 0; i < 100; i++) {
    await sql("INSERT INTO alert_events(rule_id,server_id,kind,ts_ms) VALUES ('fixture','regression','firing',?)", [now]);
  }
  for (let i = 0; i < 10; i++) {
    await sql('INSERT INTO alert_state(server_id,rule_id,since_ms,value) VALUES (?,?,?,50)',
      ['other-regression', `fixture-${i}`, now]);
  }
  let measured = (await metrics()).last;
  assert.ok(measured.rowsRead <= 1, JSON.stringify(measured));
  assert.ok(measured.rowsRead <= baseline + 1, 'A primary-key range may visit one boundary row');
  const otherRows = measured.rowsRead;
  await sql("UPDATE alert_state SET server_id='regression' WHERE server_id='other-regression'");
  measured = (await metrics()).last;
  assert.ok(measured.rowsRead <= 11, JSON.stringify(measured));
  assert.equal(measured.rowsWritten, 0);
  console.log(`RowsRead passed: E=100, A_other=10 -> ${otherRows} (empty baseline ${baseline}); A_current=10 -> ${measured.rowsRead}, no writes; queries=${JSON.stringify(measured.queries)}`);
  // Reconstruct the deployed v2 layout, then exercise its real constructor migration with data.
  await sql("ALTER TABLE alert_state RENAME TO saved_state; CREATE TABLE alert_state(rule_id TEXT,server_id TEXT,since_ms INTEGER,value REAL,PRIMARY KEY(rule_id,server_id)) WITHOUT ROWID; INSERT INTO alert_state SELECT rule_id,server_id,since_ms,value FROM saved_state; DROP TABLE saved_state; ALTER TABLE alert_events ADD COLUMN notification_json TEXT; ALTER TABLE servers DROP COLUMN country");
  const migratedNotice = JSON.stringify({ event: 'firing', rule: { id: 'fixture' },
    server: { id: 'regression' }, value: 50, ts_ms: now });
  await sql("UPDATE alert_events SET notification_json=? WHERE id=(SELECT max(id) FROM alert_events)", [migratedNotice]);
  await sql("UPDATE meta SET v='2' WHERE k='schema_version'");
  await test('recreate');
  assert.equal((await sql("SELECT count(*) AS n FROM alert_state WHERE server_id='regression'"))[0].n, 10);
  assert.equal((await sql("SELECT v FROM meta WHERE k='schema_version'"))[0].v, '4');
  assert.ok((await sql('PRAGMA table_info(servers)')).some(c => c.name === 'country'));
  assert.equal((await sql('PRAGMA table_info(alert_state)')).find(c => c.pk === 1).name, 'server_id');
  assert.equal((await sql('SELECT count(*) AS n FROM notification_queue'))[0].n, 1);
  await metrics();
  assert.equal((await sql('SELECT count(*) AS n FROM notification_queue'))[0].n, 0);
  await sql("DELETE FROM alert_state WHERE rule_id LIKE 'fixture-%'");

  const notice = JSON.stringify({ event: 'firing', rule: { id: 'fixture' },
    server: { id: 'regression' }, value: 50, ts_ms: now });
  await sql("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<26) INSERT INTO notification_queue(server_id,notification_json) SELECT 'regression',? FROM n", [notice]);
  const batch = (await metrics()).last;
  assert.ok(batch.rowsRead <= 50, JSON.stringify(batch));
  assert.equal((await sql('SELECT count(*) AS n FROM notification_queue'))[0].n, 2);
  await test('alarm');
  assert.equal((await sql('SELECT count(*) AS n FROM notification_queue'))[0].n, 0);
  console.log(`Notification queue bound: 26 pending, Metrics rowsRead=${batch.rowsRead}; Alarm drained the rest`);

  // Two scheduling failures must reject; maintenance failure must still leave the next alarm set.
  await test('delete-alarm');
  const before = await stats();
  await test('faults', { alarmSets: 2 });
  assert.equal((await test('alarm')).ok, false);
  let after = await stats();
  assert.equal(after.alarmSets - before.alarmSets, 2);
  assert.equal(after.nextAlarm, null);
  await test('faults');
  await test('recreate');
  await metrics();
  after = await stats();
  assert.ok(after.nextAlarm !== null);
  const checks = after.alarmGets;
  await metrics();
  await metrics();
  assert.equal((await stats()).alarmGets, checks, 'Alarm healing must be throttled');
  await test('faults', { sql: "SELECT v FROM meta WHERE k = 'last_hourly_ms'" });
  assert.equal((await test('alarm')).ok, true);
  assert.ok((await stats()).nextAlarm !== null);

  const minute = Math.floor(Date.now() / 60_000) * 60_000 - 60_000;
  const rollup = { type: 'rollup', minute_ms: minute, samples: 60, cpu_avg: 10, cpu_max: 20,
    mem_used_avg: 1, mem_total: 10, swap_used_avg: 0, swap_total: 0, disk_used: 1, disk_total: 10,
    rx_bytes: 100, tx_bytes: 10, rx_bps_max: 10, tx_bps_max: 1, load1_avg: 1 };
  async function send(r) {
    agent.events.length = 0;
    agent.ws.send(JSON.stringify(r));
    await event(agent, 'ack', e => e.minute_ms === r.minute_ms);
  }
  await test('faults', { sql: 'INSERT OR REPLACE INTO metrics_1m' });
  agent.ws.send(JSON.stringify(rollup));
  assert.equal(await agent.closed, 1011);
  assert.equal((await sql("SELECT * FROM metrics_1m WHERE server_id='regression'")).length, 0);
  agent = socket('/ws/agent', { Authorization: `Bearer ${current.token}` });
  await event(agent, 'welcome');
  await send(rollup);
  const traffic = await event(dash, 'traffic', e => e.id === 'regression' && e.traffic.rx_bytes === 100);
  await send(rollup); // Replay must not count twice.
  await send({ ...rollup, rx_bytes: 250, tx_bytes: 25 }); // Replacement applies only the delta.
  const status = async () => (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === 'regression');
  assert.equal((await status()).traffic_month.rx_bytes, 250);
  // Simulate waking with the previous month's attachment: next frame must recompute and publish it.
  await test('attachment', { id: 'regression', patch: { traffic_month: { ...traffic.traffic, since_ms: 0 } } });
  await metrics();
  await event(dash, 'traffic', e => e.id === 'regression' && e.traffic.rx_bytes === 250);
  assert.equal((await status()).traffic_month.rx_bytes, 250);

  // A dirty-hour failure must not persist half of a replacement and corrupt the attachment total.
  const oldMinute = Math.floor(Date.now() / 3_600_000) * 3_600_000 - 3 * 3_600_000;
  await test('faults', { sql: 'INSERT OR IGNORE INTO dirty_hours' });
  agent.ws.send(JSON.stringify({ ...rollup, minute_ms: oldMinute, rx_bytes: 10, tx_bytes: 1 }));
  assert.equal(await agent.closed, 1011);
  assert.equal((await sql("SELECT * FROM metrics_1m WHERE server_id='regression' AND ts_ms=?", [oldMinute])).length, 0);
  agent = socket('/ws/agent', { Authorization: `Bearer ${current.token}` });
  await event(agent, 'welcome');
  await send({ ...rollup, minute_ms: oldMinute, rx_bytes: 0, tx_bytes: 0 });
  assert.equal((await status()).traffic_month.rx_bytes, 250);

  const numeric = await create('numeric-regression');
  const numericAgent = socket('/ws/agent', { Authorization: `Bearer ${numeric.token}` });
  await event(numericAgent, 'welcome');
  const numericMinute = Math.floor(Date.now() / 60_000) * 60_000;
  const monthStart = new Date();
  monthStart.setUTCDate(1);
  monthStart.setUTCHours(0, 0, 0, 0);
  const numericCount = Math.min(16, (numericMinute - monthStart.getTime()) / 60_000 + 1);
  for (let i = 0; i < numericCount; i++) {
    const minute_ms = numericMinute - i * 60_000;
    numericAgent.ws.send(JSON.stringify({ ...rollup, minute_ms, rx_bytes: 2 ** 50 }));
    await event(numericAgent, 'ack', e => e.minute_ms === minute_ms);
  }
  const numericStatus = (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === 'numeric-regression');
  assert.equal(numericStatus.traffic_month.rx_bytes, numericCount * 2 ** 50, 'Legal monthly totals must survive the attachment');
  await api('/api/servers/numeric-regression', 204, { method: 'DELETE', headers: admin });
  assert.equal(await numericAgent.closed, 4003);

  // Bad input is acknowledged, but must never reach storage or alter traffic.
  for (const bad of [{ rx_bytes: 9_000_000_000_000_000_000 }, { tx_bytes: 9_000_000_000_000_000_000 }, { tx_bytes: 2 ** 50 + 1 },
    { rx_bps_max: 2 ** 47 + 1 }, { cpu_avg: -1 }, { cpu_max: 101 },
    { load1_avg: 1e100 }, { cpu_avg: 1e100 }, { mem_used_avg: 11 }, { swap_used_avg: 1 }, { disk_used: 11 }]) {
    await send({ ...rollup, minute_ms: minute - 60_000, ...bad });
  }
  assert.equal((await sql("SELECT * FROM metrics_1m WHERE server_id='regression'")).length, 2);
  assert.equal((await status()).traffic_month.rx_bytes, 250);
  // A legacy integer SUM overflow cannot break another server's snapshot.
  for (const ts of [minute, minute - 60_000]) {
    await sql("INSERT INTO metrics_1m(server_id,ts_ms,rx_bytes,tx_bytes) VALUES ('other-regression',?,9000000000000000000,0)", [ts]);
  }
  await api('/api/servers', 200, { headers: admin });
  // Invalid legacy values cause per-server fallback, never a global 500.
  await sql("UPDATE metrics_1m SET rx_bytes=-1 WHERE server_id='other-regression'");
  const otherStatus = (await (await api('/api/servers', 200, { headers: admin })).json()).find(s => s.id === 'other-regression');
  assert.equal(otherStatus.traffic_month.rx_bytes, 0);

  // Hourly event retention keeps only the newest 1000, with a separate 90-day ceiling.
  await sql("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1005) INSERT INTO alert_events(rule_id,server_id,kind,ts_ms) SELECT 'fixture','regression','firing',? FROM n", [now]);
  await sql("INSERT INTO alert_events(rule_id,server_id,kind,ts_ms) VALUES ('fixture','regression','firing',1)");
  await sql("INSERT INTO meta(k,v) VALUES('last_hourly_ms','0') ON CONFLICT(k) DO UPDATE SET v='0'");
  await test('alarm');
  const retained = await sql("SELECT count(*) AS n, min(ts_ms) AS oldest FROM alert_events");
  assert.ok(retained[0].n <= 1000);
  assert.ok(retained[0].oldest > 1);

  // DELETE removes firing state, history and queued notifications without producing resolved events.
  await sql("INSERT INTO alert_state(server_id,rule_id,since_ms) VALUES ('regression','deleted',?)", [now]);
  await sql("INSERT INTO notification_queue(server_id,notification_json) VALUES ('regression','{}')");
  await api('/api/servers/regression', 204, { method: 'DELETE', headers: admin });
  assert.equal(await agent.closed, 4003);
  for (const table of ['alert_state', 'alert_events', 'notification_queue', 'metrics_1m', 'metrics_1h', 'dirty_hours']) {
    assert.equal((await sql(`SELECT count(*) AS n FROM ${table} WHERE server_id='regression'`))[0].n, 0);
  }
  await api('/api/servers/other-regression', 204, { method: 'DELETE', headers: admin });
  dash.ws.close();
  console.log('Recovery/data passed: Rust DTO -> handler, Alarm rejection/healing/throttle, SQL failure/replay, traffic, numeric bounds, retention and DELETE');
}
