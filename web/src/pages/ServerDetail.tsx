import { useEffect, useMemo, useState } from "react";
import type { History, HistoryPoint, HistoryRange } from "../api/generated";
import { Chart, type ChartSeries } from "../components/Chart";
import { Badge, Card, EmptyState, ErrorText, Spinner, StatusDot, cx } from "../components/ui";
import { api, errorText } from "../lib/api";
import { ago, bytes, dateTime, duration, pct, pctText, rate } from "../lib/format";
import { useLive } from "../lib/live";
import { href } from "../lib/router";
import { useNow } from "../lib/useNow";

const RANGES: HistoryRange[] = ["1h", "6h", "24h", "7d", "30d", "90d"];

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

function Tile({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <Card className="px-4 py-3">
      <div className="text-xs text-zinc-500">{label}</div>
      <div className="tabular mt-1 text-lg font-semibold">{value}</div>
      {sub && <div className="tabular truncate text-xs text-zinc-500">{sub}</div>}
    </Card>
  );
}

export function ServerDetail({ id }: { id: string }) {
  const { ready, servers, alerts } = useLive();
  const now = useNow();
  const s = servers.get(id);
  const [range, setRange] = useState<HistoryRange>("24h");
  const [history, setHistory] = useState<History | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let alive = true;
    const load = () => {
      setLoading(true);
      api
        .history(id, range)
        .then((h) => alive && (setHistory(h), setError("")))
        .catch((e) => alive && setError(errorText(e)))
        .finally(() => alive && setLoading(false));
    };
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
      { label: "下载峰值", color: "--series-1", values: col((p) => p.rx_bps_max) },
      { label: "上传峰值", color: "--series-2", values: col((p) => p.tx_bps_max) },
    ];
    const disk: ChartSeries[] = [{ label: "磁盘", color: "--series-1", values: col((p) => p.disk_pct) }];
    const load: ChartSeries[] = [{ label: "Load 1m", color: "--series-1", values: col((p) => p.load1) }];
    const traffic = history.points.reduce((a, p) => ({ rx: a.rx + p.rx_bytes, tx: a.tx + p.tx_bytes }), { rx: 0, tx: 0 });
    return { xs, cpu, mem, net, netPeak, disk, load, traffic };
  }, [history]);

  if (!ready)
    return (
      <div className="grid place-items-center py-24">
        <Spinner />
      </div>
    );
  if (!s)
    return (
      <EmptyState title="服务器不存在">
        <a className="underline" href="#/">返回总览</a>
      </EmptyState>
    );

  const m = s.metrics;
  const firing = alerts.filter((a) => a.server_id === id);
  const pctFmt = (v: number) => `${v.toFixed(0)}%`;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center gap-3">
        <a href="#/" className="text-sm text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100">
          ← 总览
        </a>
        <h1 className="text-xl font-semibold">{s.name}</h1>
        <StatusDot online={s.online} />
        {!s.online && (
          <span className="text-xs text-zinc-500">{s.last_seen_ms ? `最后在线 ${ago(s.last_seen_ms, now)}` : "Agent 尚未连接"}</span>
        )}
        {firing.map((a) => (
          <Badge key={a.rule_id} tone="critical">
            ⚠ {a.rule_name}
          </Badge>
        ))}
        <a href={href({ page: "settings", tab: "servers" })} className="ml-auto text-sm text-zinc-500 hover:underline">
          管理
        </a>
      </div>

      <div className="grid grid-cols-2 gap-3 md:grid-cols-4 xl:grid-cols-7">
        <Tile label="CPU" value={pctText(m?.cpu_pct)} sub={s.host ? `${s.host.cpu_cores} 核` : undefined} />
        <Tile
          label="内存"
          value={m ? pctText(pct(m.mem_used, m.mem_total)) : "–"}
          sub={m ? `${bytes(m.mem_used)} / ${bytes(m.mem_total)}` : undefined}
        />
        <Tile
          label="磁盘"
          value={m ? pctText(pct(m.disk_used, m.disk_total)) : "–"}
          sub={m ? `${bytes(m.disk_used)} / ${bytes(m.disk_total)}` : undefined}
        />
        <Tile label="↓ 下载" value={rate(m?.net_rx_bps)} sub={`本月 ${bytes(s.traffic_month.rx_bytes)}`} />
        <Tile label="↑ 上传" value={rate(m?.net_tx_bps)} sub={`本月 ${bytes(s.traffic_month.tx_bytes)}`} />
        <Tile
          label="负载"
          value={m ? m.load1.toFixed(2) : "–"}
          sub={m ? `${m.load5.toFixed(2)} · ${m.load15.toFixed(2)}` : undefined}
        />
        <Tile label="运行时间" value={m ? duration(m.uptime_s) : "–"} sub={m ? `${m.process_count} 个进程` : undefined} />
      </div>

      <section className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="mr-2 text-sm font-medium">历史</h2>
          <div className="inline-flex rounded-md border border-zinc-200 p-0.5 dark:border-zinc-800" role="tablist">
            {RANGES.map((r) => (
              <button
                key={r}
                role="tab"
                aria-selected={range === r}
                onClick={() => setRange(r)}
                className={cx(
                  "rounded px-2.5 py-1 text-xs",
                  range === r ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900" : "text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100",
                )}
              >
                {r}
              </button>
            ))}
          </div>
          {loading && <Spinner className="size-3.5" />}
          {charts && (
            <span className="tabular ml-auto text-xs text-zinc-500">
              区间流量 ↓ {bytes(charts.traffic.rx)} · ↑ {bytes(charts.traffic.tx)}
            </span>
          )}
        </div>
        <ErrorText>{error}</ErrorText>
        {charts && history && history.points.length === 0 && <EmptyState title="该时间范围内暂无数据" />}
        {charts && history && history.points.length > 0 && (
          <div className="grid gap-4 lg:grid-cols-2">
            <Chart title="CPU 使用率" xs={charts.xs} series={charts.cpu} format={pctFmt} yMax={100} />
            <Chart title="内存使用率" xs={charts.xs} series={charts.mem} format={pctFmt} yMax={100} />
            <Chart title="网络（平均速率）" xs={charts.xs} series={charts.net} format={(v) => rate(v)} />
            <Chart title="网络（峰值速率）" xs={charts.xs} series={charts.netPeak} format={(v) => rate(v)} />
            <Chart title="磁盘使用率" xs={charts.xs} series={charts.disk} format={pctFmt} yMax={100} />
            <Chart title="负载（1 分钟）" xs={charts.xs} series={charts.load} format={(v) => v.toFixed(2)} />
          </div>
        )}
      </section>

      {s.host && (
        <section>
          <h2 className="mb-3 text-sm font-medium">主机信息</h2>
          <Card>
            <dl className="grid gap-x-6 gap-y-2 p-4 text-sm sm:grid-cols-2">
              {[
                ["主机名", s.host.hostname],
                ["系统", s.host.os],
                ["内核", s.host.kernel],
                ["架构", s.host.arch],
                ["CPU", `${s.host.cpu_model} × ${s.host.cpu_cores}`],
                ["启动时间", dateTime(s.host.boot_time_ms)],
                ["Agent 版本", s.agent_version ?? "–"],
                ["ID", s.id],
              ].map(([k, v]) => (
                <div key={k} className="flex gap-3">
                  <dt className="w-20 shrink-0 text-zinc-500">{k}</dt>
                  <dd className="min-w-0 break-words">{v}</dd>
                </div>
              ))}
            </dl>
          </Card>
        </section>
      )}
    </div>
  );
}
