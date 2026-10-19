// Regenerates docs/screenshots/*.png from the built dashboard with mock data. The browser
// loads web/dist from file-backed routes and gets /api/* and the dashboard socket from mocks,
// so no Worker is needed. Usage (after `pnpm build`): pnpm screenshots [name-filter]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const web = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const dist = path.join(web, "dist");
const out = path.join(web, "..", "docs", "screenshots");
const BASE = "http://moni.local";
const filter = process.argv[2];

const KB = 1024;
const MB = KB * KB;
const GB = MB * KB;
const DAY = 86_400_000;
const now = Date.now();

/** Deterministic noise, so reruns produce the same pictures. */
let seed = 7;
const rand = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646;

// Live values follow the README: six servers, one never connected.
const SPECS = [
  { id: "01", name: "东京 01", country: "JP", cores: 2, cpu: 11, mem: [942.1 * MB, 2 * GB], disk: [15 * GB, 40 * GB], net: [3.3 * MB, 2.0 * MB], month: [507.9 * GB, 304.8 * GB], up: 12, load: 0.21, cpuModel: "AMD EPYC 7B13" },
  { id: "02", name: "法兰克福", country: "DE", cores: 2, cpu: 91, mem: [3.3 * GB, 4 * GB], disk: [53 * GB, 60 * GB], net: [8.7 * MB, 5.2 * MB], month: [760.3 * GB, 456.2 * GB], up: 39, load: 1.82, cpuModel: "Intel Xeon Gold 6230" },
  { id: "03", name: "洛杉矶", country: "US", cores: 1, cpu: 9, mem: [179.2 * MB, 512 * MB], disk: [3 * GB, 10 * GB], net: [575.9 * KB, 345.6 * KB], month: [84.3 * GB, 50.6 * GB], up: 48, load: 0.09, cpuModel: "Intel Xeon E5-2680 v4" },
  { id: "04", name: "香港 02", country: "HK", cores: 1, cpu: 9, mem: [634.9 * MB, 1 * GB], disk: [11 * GB, 20 * GB], net: [3.1 * MB, 1.8 * MB], month: [612.4 * GB, 371.5 * GB], up: 7, load: 0.09, cpuModel: "AMD EPYC 7543" },
  { id: "05", name: "新加坡", country: "SG", cores: 4, cpu: 52, mem: [5.7 * GB, 8 * GB], disk: [38 * GB, 80 * GB], net: [18.6 * MB, 11.1 * MB], month: [1.21 * KB * GB, 0.74 * KB * GB], up: 21, load: 2.07, cpuModel: "AMD EPYC 9654" },
  { id: "06", name: "首尔 (备用)", country: "KR", offline: true },
];

function frame(s, seq, ts, jitter) {
  const j = (v, k) => Math.max(0, v * (1 + (rand() - 0.5) * k * jitter));
  return {
    seq, ts_ms: ts, cpu_pct: Math.min(100, j(s.cpu, 0.5)),
    mem_used: s.mem[0], mem_total: s.mem[1], swap_used: 0.09 * s.mem[1], swap_total: s.mem[1],
    disk_used: s.disk[0], disk_total: s.disk[1],
    net_rx_bps: j(s.net[0], 0.6), net_tx_bps: j(s.net[1], 0.6),
    load1: s.load, load5: s.load * 0.95, load15: s.load * 0.9,
    uptime_s: s.up * 86_400, process_count: 60 + s.cores * 15,
  };
}

function status(s) {
  const month = { rx_bytes: s.month?.[0] ?? 0, tx_bytes: s.month?.[1] ?? 0, since_ms: now - 18 * DAY };
  if (s.offline)
    return { id: s.id, name: s.name, created_ms: now - 60 * DAY, online: false, last_seen_ms: null, agent_version: null, host: null, metrics: null, traffic_month: month, alerts_firing: 0, country: s.country };
  return {
    // Shaped like the public view, which redacts hostname, kernel and Agent version.
    id: s.id, name: s.name, created_ms: now - 60 * DAY, online: true, last_seen_ms: now, agent_version: null, country: s.country,
    host: { hostname: "", os: "Debian GNU/Linux 13", kernel: "", arch: "x86_64", cpu_model: s.cpuModel, cpu_cores: s.cores, boot_time_ms: now - s.up * DAY },
    metrics: frame(s, 0, now, 0), traffic_month: month, alerts_firing: 0,
  };
}

/** A day of per-minute rollups with a diurnal swell, like a busy proxy box. */
function history(s, range) {
  const hours = { "1h": 1, "6h": 6, "24h": 24, "7d": 168, "30d": 720, "90d": 2160 }[range] ?? 24;
  const step = hours <= 24 ? 60_000 : 3_600_000;
  const points = [];
  for (let t = now - hours * 3_600_000 + step; t <= now; t += step) {
    const day = Math.sin(((t / DAY) % 1) * 2 * Math.PI - 1.2);
    const wave = Math.sin(t / 1_800_000);
    const swell = 0.55 + 0.45 * day;
    const cpu = Math.max(1, s.cpu * (0.8 + 0.35 * swell + 0.12 * wave + 0.1 * rand()));
    const rx = s.net[0] * (0.25 + 1.4 * swell) * (0.7 + 0.6 * rand());
    const tx = s.net[1] * (0.25 + 1.4 * swell) * (0.7 + 0.6 * rand());
    points.push({
      ts_ms: t, cpu_avg: cpu, cpu_max: Math.min(100, cpu * (1.3 + 0.2 * rand())),
      mem_pct: (100 * s.mem[0]) / s.mem[1] + 3 * Math.sin(t / 21_600_000), swap_pct: 9.2,
      disk_pct: (100 * s.disk[0]) / s.disk[1],
      rx_bytes: (rx * step) / 1000, tx_bytes: (tx * step) / 1000,
      rx_bps_max: rx * (1.6 + 0.4 * rand()), tx_bps_max: tx * (1.6 + 0.4 * rand()),
      load1: s.load * (0.8 + 0.6 * swell + 0.15 * wave),
    });
  }
  return { range, step_ms: step, points };
}

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml" };

async function page(browser, { theme, mode, width, height, scale }) {
  const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, colorScheme: mode });
  const p = await ctx.newPage();
  await p.addInitScript(([t, m]) => localStorage.setItem("moni-theme", JSON.stringify({ theme: t, mode: m })), [theme, mode]);
  await p.route(`${BASE}/**`, (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === "/api/auth/me") return route.fulfill({ status: 401, json: { error: "unauthorized", message: "" } });
    if (url.pathname === "/api/public") return route.fulfill({ json: { enabled: true, appearance: { theme, mode, locked: false } } });
    const hist = url.pathname.match(/^\/api\/servers\/([^/]+)\/history$/);
    if (hist) return route.fulfill({ json: history(SPECS.find((s) => s.id === hist[1]), url.searchParams.get("range")) });
    const file = path.join(dist, url.pathname === "/" ? "index.html" : decodeURIComponent(url.pathname));
    if (!file.startsWith(dist) || !fs.existsSync(file)) return route.fulfill({ status: 404 });
    return route.fulfill({ body: fs.readFileSync(file), contentType: TYPES[path.extname(file)] ?? "application/octet-stream" });
  });
  await p.routeWebSocket("ws://moni.local/ws/dashboard", (ws) => {
    ws.send(JSON.stringify({ type: "snapshot", servers: SPECS.map(status), alerts: [] }));
    // The last frame per server lands on the README value; the earlier ones fill the sparkline.
    for (const s of SPECS.filter((x) => !x.offline))
      for (let i = 30; i >= 0; i--) ws.send(JSON.stringify({ type: "metrics", id: s.id, frame: frame(s, 30 - i, now - i * 2000, i === 0 ? 0 : 1) }));
  });
  return p;
}

async function shot(browser, name, { theme = "clay", mode = "light", route = "/", width = 1080, height = 675, scale = 2, fullPage = false }) {
  if (filter && !name.includes(filter)) return;
  const p = await page(browser, { theme, mode, width, height, scale });
  await p.goto(`${BASE}/#${route}`);
  await p.waitForSelector(route === "/" ? "[data-server-card]" : "text=主机信息");
  // Let flags, charts and the entrance animations settle.
  await p.waitForLoadState("networkidle");
  await p.waitForTimeout(800);
  await p.screenshot({ path: path.join(out, `${name}.png`), fullPage });
  await p.context().close();
  console.log(`✓ ${name}.png`);
}

const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
try {
  for (const [theme, mode] of [
    ["clay", "light"], ["clay", "dark"], ["cyberpunk", "dark"], ["material", "light"],
    ["cloudflare", "light"], ["terminal", "dark"], ["nord", "dark"], ["geist", "light"],
  ])
    await shot(browser, `theme-${theme}-${mode}`, { theme, mode });
  await shot(browser, "detail", { route: "/server/01", height: 900, fullPage: true });
  await shot(browser, "mobile-overview", { width: 390, height: 844, scale: 3 });
  await shot(browser, "mobile-detail", { mode: "dark", route: "/server/01", width: 390, height: 844, scale: 3 });
} finally {
  await browser.close();
}
