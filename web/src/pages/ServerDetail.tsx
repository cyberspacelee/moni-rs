import { type ReactNode, useEffect, useMemo, useState } from "react";
import type { History, HistoryPoint, HistoryRange } from "../api/generated";
import { Chart, type ChartSeries } from "../components/Chart";
import { Flag, countryName } from "../components/Flag";
import {
  IconAlert,
  IconBack,
  IconClock,
  IconCpu,
  IconDown,
  IconGlobe,
  IconHistory,
  IconServer,
  IconSliders,
  IconUp,
} from "../components/icons";
import { Badge, Card, EmptyState, ErrorText, PageSpinner, Segmented, Spinner, StatusDot, levelColor } from "../components/ui";
import { api, errorText } from "../lib/api";
import { ago, bytes, dateTime, duration, pct, pctText, rate } from "../lib/format";
import { useLive } from "../lib/live";
import { href } from "../lib/router";
import { useNow } from "../lib/useNow";

const RANGES: { id: HistoryRange; label: string; hours: number }[] = [
  { id: "1h", label: "1 小时", hours: 1 },
  { id: "6h", label: "6 小时", hours: 6 },
  { id: "24h", label: "24 小时", hours: 24 },
  { id: "7d", label: "7 天", hours: 168 },
  { id: "30d", label: "30 天", hours: 720 },
  { id: "90d", label: "90 天", hours: 2160 },
];

/** Break lines where buckets are missing instead of drawing across the gap. */
function withGaps(h: History) {
  const xs: number[] = [];
  const rows: (HistoryPoint | null)[] = [];
  let prev: number | null = null;
  for (const p of h.points) {
    if (prev != null && p.ts_ms - prev > h.step_ms * 1.5) {
      xs.push((prev + h.step_ms) / 1000);
      rows.push(null);
    }
    xs.push(p.ts_ms / 1000);
    rows.push(p);
    prev = p.ts_ms;
  }
  return { xs, rows, stepS: h.step_ms / 1000 };
}

/** Axis ticks need to fit 46px: drop the "/s" and decimals the chart legend already carries. */
function shortRate(v: number): string {
  const units = ["B", "K", "M", "G", "T"];
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 10 || i === 0 ? v.toFixed(0) : v.toFixed(1)}${units[i]}`;
}

function Tile({ label, icon, value, sub, level }: { label: string; icon: ReactNode; value: ReactNode; sub?: string; level?: number | null }) {
  return (
    <Card className="relative overflow-hidden px-4 py-3.5">
      <div className="flex items-center gap-1.5 text-xs text-ink-3">
        {icon}
        {label}
      </div>
      <div className="tabular mt-1.5 truncate font-mono text-lg font-semibold tracking-tight">{value}</div>
      {sub && <div className="tabular truncate text-xs text-ink-3">{sub}</div>}
      {level !== undefined && (
        <div aria-hidden className="absolute inset-x-0 bottom-0 h-0.5 bg-surface-2">
          <div
            className="h-full transition-[width] duration-700"
            style={{ width: `${Math.min(100, Math.max(0, level ?? 0))}%`, background: levelColor(level ?? null) }}
          />
        </div>
      )}
    </Card>
  );
}

export function ServerDetail({ id, admin }: { id: string; admin: boolean }) {
  const { ready, servers, alerts } = useLive();
  const now = useNow();
  const s = servers.get(id);
  const [range, setRange] = useState<HistoryRange>("24h");
  // Keep the query a result belongs to, so a failed switch never shows another query's data.
  const [loaded, setLoaded] = useState<{ key: string; history: History } | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const queryKey = `${id}\u0000${range}`;
  const history = loaded?.key === queryKey ? loaded.history : null;

  useEffect(() => {
    let alive = true;
    const key = `${id}\u0000${range}`;
    const load = () => {
      setLoading(true);
      api
        .history(id, range)
        .then((h) => {
          if (!alive) return;
          setLoaded({ key, history: h });
          setError("");
        })
        .catch((e) => alive && setError(`${range} 历史加载失败：${errorText(e)}`))
        .finally(() => alive && setLoading(false));
    };
    setError("");
    load();
    // Minute rollups land once a minute; refresh short ranges accordingly.
    const t = range === "1h" || range === "6h" ? window.setInterval(load, 60_000) : undefined;
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [id, range]);

  const charts = useMemo(() => {
    if (!history) return null;
    const { xs, rows, stepS } = withGaps(history);
    const col = (f: (p: HistoryPoint) => number) => rows.map((p) => (p ? f(p) : null));
    const cpu: ChartSeries[] = [
      { label: "平均", color: "--series-1", values: col((p) => p.cpu_avg) },
      { label: "峰值", color: "--series-2", values: col((p) => p.cpu_max) },
    ];
    const mem: ChartSeries[] = [
      { label: "内存", color: "--series-1", values: col((p) => p.mem_pct) },
      { label: "Swap", color: "--series-2", values: col((p) => p.swap_pct) },
    ];
    const net: ChartSeries[] = [
      { label: "下载", color: "--series-1", values: col((p) => p.rx_bytes / stepS) },
      { label: "上传", color: "--series-2", values: col((p) => p.tx_bytes / stepS) },
    ];
    const netPeak: ChartSeries[] = [
      { label: "下载", color: "--series-1", values: col((p) => p.rx_bps_max) },
      { label: "上传", color: "--series-2", values: col((p) => p.tx_bps_max) },
    ];
    const disk: ChartSeries[] = [{ label: "磁盘", color: "--series-1", values: col((p) => p.disk_pct) }];
    const load: ChartSeries[] = [{ label: "Load 1m", color: "--series-1", values: col((p) => p.load1) }];
    const traffic = history.points.reduce((a, p) => ({ rx: a.rx + p.rx_bytes, tx: a.tx + p.tx_bytes }), { rx: 0, tx: 0 });
    const end = Date.now() / 1000;
    const hours = RANGES.find((r) => r.id === history.range)?.hours ?? 24;
    const window: [number, number] = [end - hours * 3600, end];
    return { xs, cpu, mem, net, netPeak, disk, load, traffic, window };
  }, [history]);

  if (!ready) return <PageSpinner />;
  if (!s)
    return (
      <EmptyState title="服务器不存在" icon={<IconServer size={18} />}>
        <a className="text-brand hover:underline" href="#/">
          返回总览
        </a>
      </EmptyState>
    );

  const m = s.metrics;
  const firing = alerts.filter((a) => a.server_id === id);
  const pctFmt = (v: number) => `${v.toFixed(1)}%`;
  const pctAxis = (v: number) => `${v.toFixed(0)}%`;
  const memPct = m ? pct(m.mem_used, m.mem_total) : null;
  const diskPct = m ? pct(m.disk_used, m.disk_total) : null;

  // Redacted fields arrive empty in the public view; leave them out rather than show blanks.
  const hostRows: [string, string][] = s.host
    ? (
        [
          ["地区", s.country ? `${countryName(s.country)}（${s.country}）` : ""],
          ["主机名", s.host.hostname],
          ["系统", s.host.os],
          ["内核", s.host.kernel],
          ["架构", s.host.arch],
          ["CPU", `${s.host.cpu_model} × ${s.host.cpu_cores}`],
          ["启动时间", dateTime(s.host.boot_time_ms)],
          ["Agent", s.agent_version ?? ""],
          ["ID", s.id],
        ] as [string, string][]
      ).filter(([, v]) => v)
    : [];

  return (
    <div className="space-y-6 sm:space-y-8">
      <div className="space-y-3">
        <a href="#/" className="inline-flex items-center gap-1 text-sm text-ink-3 hover:text-ink">
          <IconBack size={15} />
          总览
        </a>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <Flag code={s.country} className="text-xl" />
          <h1 className="min-w-0 truncate text-xl font-semibold tracking-tight sm:text-2xl">{s.name}</h1>
          <StatusDot online={s.online} />
          {!s.online && (
            <span className="text-xs text-ink-3">{s.last_seen_ms ? `最后在线 ${ago(s.last_seen_ms, now)}` : "Agent 尚未连接"}</span>
          )}
          {admin && (
            <a
              href={href({ page: "settings", tab: "servers" })}
              className="ml-auto inline-flex h-8 items-center gap-1.5 rounded-lg border border-line bg-surface px-3 text-xs font-medium text-ink-2 shadow-xs hover:text-ink"
            >
              <IconSliders size={14} />
              管理
            </a>
          )}
        </div>
        {(s.host || firing.length > 0) && (
          <div className="flex flex-wrap items-center gap-2 text-xs text-ink-3">
            {s.host && (
              <span>
                {s.country && `${countryName(s.country)} · `}
                {s.host.os} · {s.host.arch}
              </span>
            )}
            {firing.map((a) => (
              <Badge key={a.rule_id} tone="critical">
                <IconAlert size={11} />
                {a.rule_name}
              </Badge>
            ))}
          </div>
        )}
      </div>

      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        <Tile label="CPU" icon={<IconCpu size={13} />} value={pctText(m?.cpu_pct)} sub={s.host ? `${s.host.cpu_cores} 核` : undefined} level={m?.cpu_pct ?? null} />
        <Tile
          label="内存"
          icon={<IconServer size={13} />}
          value={pctText(memPct)}
          sub={m ? `${bytes(m.mem_used)} / ${bytes(m.mem_total)}` : undefined}
          level={memPct}
        />
        <Tile
          label="磁盘"
          icon={<IconServer size={13} />}
          value={pctText(diskPct)}
          sub={m ? `${bytes(m.disk_used)} / ${bytes(m.disk_total)}` : undefined}
          level={diskPct}
        />
        <Tile
          label="网络"
          icon={<IconGlobe size={13} />}
          value={
            <span className="flex flex-col text-sm leading-6">
              <span className="inline-flex items-center gap-1">
                <IconDown size={12} className="text-[var(--series-1)]" />
                {rate(m?.net_rx_bps)}
              </span>
              <span className="inline-flex items-center gap-1">
                <IconUp size={12} className="text-[var(--series-2)]" />
                {rate(m?.net_tx_bps)}
              </span>
            </span>
          }
        />
        <Tile
          label="本月流量"
          icon={<IconHistory size={13} />}
          value={bytes(s.traffic_month.rx_bytes + s.traffic_month.tx_bytes)}
          sub={`↓${bytes(s.traffic_month.rx_bytes)} ↑${bytes(s.traffic_month.tx_bytes)}`}
        />
        <Tile
          label="负载 · 运行"
          icon={<IconClock size={13} />}
          value={m ? m.load1.toFixed(2) : "–"}
          sub={m ? `${duration(m.uptime_s)} · ${m.process_count} 进程` : undefined}
        />
      </div>

      <section className="space-y-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
          <h2 className="flex items-center gap-2 text-base font-semibold tracking-tight">
            历史趋势
            {loading && <Spinner className="size-3.5" />}
          </h2>
          <div className="sm:ml-auto">
            <Segmented label="时间范围" value={range} onChange={setRange} options={RANGES} />
          </div>
        </div>
        {charts && (
          <p className="tabular -mt-1 text-xs text-ink-3">
            区间流量 ↓ {bytes(charts.traffic.rx)} · ↑ {bytes(charts.traffic.tx)}
          </p>
        )}
        <ErrorText>{error}</ErrorText>
        {charts && history && history.points.length === 0 && <EmptyState title="该时间范围内暂无数据" icon={<IconHistory size={18} />} />}
        {charts && history && history.points.length > 0 && (
          <div className="grid gap-3 sm:gap-4 lg:grid-cols-2">
            <Chart title="CPU 使用率" xs={charts.xs} window={charts.window} series={charts.cpu} format={pctFmt} axisFormat={pctAxis} yMax={100} />
            <Chart title="内存使用率" xs={charts.xs} window={charts.window} series={charts.mem} format={pctFmt} axisFormat={pctAxis} yMax={100} />
            <Chart title="网络 · 平均速率" xs={charts.xs} window={charts.window} series={charts.net} format={(v) => rate(v)} axisFormat={shortRate} />
            <Chart title="网络 · 峰值速率" xs={charts.xs} window={charts.window} series={charts.netPeak} format={(v) => rate(v)} axisFormat={shortRate} />
            <Chart title="磁盘使用率" xs={charts.xs} window={charts.window} series={charts.disk} format={pctFmt} axisFormat={pctAxis} yMax={100} />
            <Chart title="负载 · 1 分钟" xs={charts.xs} window={charts.window} series={charts.load} format={(v) => v.toFixed(2)} />
          </div>
        )}
      </section>

      {hostRows.length > 0 && (
        <section className="space-y-4">
          <h2 className="text-base font-semibold tracking-tight">主机信息</h2>
          <Card>
            <dl className="grid divide-y divide-line text-sm sm:grid-cols-2 sm:divide-y-0">
              {hostRows.map(([k, v]) => (
                <div key={k} className="flex gap-4 px-4 py-3 sm:px-5">
                  <dt className="w-16 shrink-0 text-ink-3">{k}</dt>
                  <dd className="min-w-0 font-mono text-[13px] break-words">{v}</dd>
                </div>
              ))}
            </dl>
          </Card>
        </section>
      )}
    </div>
  );
}
