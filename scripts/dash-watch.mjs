// 命令行看板（Node 22+）：MONI_ADMIN_PASSWORD=... node scripts/dash-watch.mjs "ws://127.0.0.1:8787/ws/dashboard" 60
const [url, secs] = process.argv.slice(2);
const password = process.env.MONI_ADMIN_PASSWORD;
if (!url || !password) {
  console.error('Usage: MONI_ADMIN_PASSWORD=... node scripts/dash-watch.mjs <ws-url> [seconds]');
  process.exit(2);
}
const ws = new WebSocket(url, { headers: { Authorization: `Bearer ${password}` } });
const t0 = Date.now();
const counts = {};
ws.onopen = () => console.log('dash open');
ws.onmessage = (e) => {
  const ev = JSON.parse(e.data);
  counts[ev.type] = (counts[ev.type] || 0) + 1;
  const t = ((Date.now() - t0) / 1000).toFixed(1);
  if (ev.type === 'metrics') {
    if (counts.metrics <= 4) console.log(t, 'metrics', ev.id, 'seq', ev.frame.seq, 'cpu', ev.frame.cpu_pct.toFixed(1), 'rx', ev.frame.net_rx_bps);
  } else console.log(t, JSON.stringify(ev).slice(0, 300));
};
ws.onclose = (e) => console.log('dash close', e.code);
setTimeout(() => { console.log('counts', counts); ws.close(); process.exit(0); }, (Number(secs) || 60) * 1000);
