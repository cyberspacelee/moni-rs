import { type ReactNode, useState } from "react";
import type { ServerStatus } from "../api/generated";
import { Flag, countryName } from "../components/Flag";
import { IconAlert, IconDown, IconSearch, IconServer, IconUp } from "../components/icons";
import { Sparkline } from "../components/Sparkline";
import { StationClock } from "../components/StationClock";
import { Badge, Card, EmptyState, Meter, PageSpinner, Segmented, StatusDot, levelColor } from "../components/ui";
import { ago, bytes, clockTime, duration, pct, rate } from "../lib/format";
import { type RecentPoint, useLive } from "../lib/live";
import { href } from "../lib/router";
import { useNow } from "../lib/useNow";

/** One reading in the summary strip: hairline-separated, no box, the numeral carries it. */
function Reading({ label, children, sub }: { label: string; children: ReactNode; sub?: ReactNode }) {
  return (
    <div className="min-w-0 bg-surface px-4 py-3 sm:px-5">
      <div className="text-xs leading-4 text-ink-3">{label}</div>
      <div className="tabular mt-1 truncate text-xl leading-7 font-semibold tracking-tight sm:text-2xl sm:leading-8">{children}</div>
      {sub && <div className="tabular mt-0.5 truncate text-xs leading-4 text-ink-3">{sub}</div>}
    </div>
  );
}

function hostLine(s: ServerStatus): string {
  // The OS string ("Linux (Debian GNU/Linux 13)") would truncate everything after it; detail shows it.
  return [s.country && countryName(s.country), s.host && `${s.host.cpu_cores} 核`, s.host?.arch].filter(Boolean).join(" · ") || s.id;
}

function ServerCard({ s, now, recent }: { s: ServerStatus; now: number; recent: readonly RecentPoint[] }) {
  const m = s.metrics;
  const cpu = m?.cpu_pct ?? null;
  const live = s.online && m != null;
  const clockLabel = live ? "运行中" : s.last_seen_ms ? `停于 ${clockTime(s.last_seen_ms, now)}` : "从未连接";
  return (
    <a href={href({ page: "server", id: s.id })} data-server-card className="group block min-w-0 rounded-[10px]">
      <Card className="flex h-full flex-col overflow-hidden transition-[border-color,box-shadow] duration-150 group-hover:border-line-strong group-hover:shadow-[0_6px_20px_-10px_rgb(0_0_0/0.25)]">
        <div className="flex items-center gap-3 px-4 pt-4 sm:px-5">
          <StationClock ms={live ? m.ts_ms : s.last_seen_ms} running={live} size={36} label={clockLabel} />
          <div className="min-w-0 flex-1">
            <div className="flex h-5 items-center gap-2">
              <span className="truncate leading-5 font-semibold">{s.name}</span>
              {s.alerts_firing > 0 && (
                <Badge tone="critical">
                  <IconAlert size={12} />
                  {s.alerts_firing}
                </Badge>
              )}
            </div>
            <div className="mt-0.5 flex h-4 items-center gap-1.5 text-xs leading-4 text-ink-3">
              {s.country && <Flag code={s.country} className="text-[13px]" />}
              <span className="truncate">{hostLine(s)}</span>
            </div>
          </div>
          <StatusDot online={s.online} />
        </div>

        {live ? (
          <>
            <div className="mt-3 flex items-end gap-3 px-4 sm:px-5">
              <div className="shrink-0">
                <div className="tabular text-3xl leading-8 font-semibold tracking-tight">
                  {cpu == null ? "–" : cpu.toFixed(0)}
                  <span className="ml-0.5 text-base font-normal text-ink-3">%</span>
                </div>
                <div className="mt-1 text-xs leading-4 text-ink-3">CPU · 负载 {m.load1.toFixed(2)}</div>
              </div>
              <div className="h-12 min-w-0 flex-1">
                <Sparkline values={recent.map((p) => p.cpu)} color={levelColor(cpu)} />
              </div>
            </div>
            <div className="mt-4 space-y-3 border-t border-line px-4 pt-3.5 pb-4 sm:px-5">
              <Meter label="内存" value={pct(m.mem_used, m.mem_total)} detail={`${bytes(m.mem_used)} / ${bytes(m.mem_total, 0)}`} />
              <Meter label="磁盘" value={pct(m.disk_used, m.disk_total)} detail={`${bytes(m.disk_used, 0)} / ${bytes(m.disk_total, 0)}`} />
              <div className="tabular grid grid-cols-2 gap-x-4 border-t border-line pt-3 text-xs leading-4">
                <div className="flex items-center gap-1.5">
                  <IconDown size={14} className="shrink-0 text-[var(--series-1)]" />
                  <span className="text-ink-3">下行</span>
                  <span className="ml-auto truncate font-medium">{rate(m.net_rx_bps)}</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <IconUp size={14} className="shrink-0 text-[var(--series-2)]" />
                  <span className="text-ink-3">上行</span>
                  <span className="ml-auto truncate font-medium">{rate(m.net_tx_bps)}</span>
                </div>
              </div>
              <div className="tabular flex justify-between gap-2 text-xs leading-4 text-ink-3">
                <span className="truncate">
                  本月 ↓ {bytes(s.traffic_month.rx_bytes)} · ↑ {bytes(s.traffic_month.tx_bytes)}
                </span>
                <span className="shrink-0">运行 {duration(m.uptime_s)}</span>
              </div>
            </div>
          </>
        ) : (
          // A stopped clock keeps its place on the board: the card stays, stamped with when it stopped.
          <div className="mt-4 border-t border-dashed border-line-strong px-4 pt-3.5 pb-4 sm:px-5">
            <div className="tabular text-xl leading-7 font-semibold tracking-tight text-ink-2">
              {s.online ? "等待数据…" : s.last_seen_ms ? `停于 ${clockTime(s.last_seen_ms, now)}` : "尚未接入"}
            </div>
            <div className="mt-1 text-xs leading-4 text-ink-3">
              {s.online ? "Agent 已连接，首帧即将到达" : s.last_seen_ms ? `${ago(s.last_seen_ms, now)}失去心跳` : "在 VPS 上运行安装命令后，这只钟会开始走"}
            </div>
          </div>
        )}
      </Card>
    </a>
  );
}

type Filter = "all" | "online" | "offline";

export function Overview({ admin }: { admin: boolean }) {
  const { ready, servers, alerts, recent } = useLive();
  const now = useNow();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  if (!ready) return <PageSpinner />;

  const list = [...servers.values()].sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name));
  const online = list.filter((s) => s.online);
  const rx = online.reduce((a, s) => a + (s.metrics?.net_rx_bps ?? 0), 0);
  const tx = online.reduce((a, s) => a + (s.metrics?.net_tx_bps ?? 0), 0);
  const month = list.reduce((a, s) => a + s.traffic_month.rx_bytes + s.traffic_month.tx_bytes, 0);
  const regions = new Set(list.map((s) => s.country).filter(Boolean)).size;

  const q = query.trim().toLowerCase();
  const shown = list.filter(
    (s) =>
      (filter === "all" || s.online === (filter === "online")) &&
      (!q ||
        s.name.toLowerCase().includes(q) ||
        s.id.includes(q) ||
        (s.country != null && (s.country.toLowerCase() === q || countryName(s.country).includes(query.trim())))),
  );

  return (
    <div className="space-y-5 sm:space-y-6">
      {/* 1px gaps over a line-coloured ground draw the hairlines between readings at any column count. */}
      <section
        aria-label="汇总"
        className="grid grid-cols-2 gap-px overflow-hidden rounded-[10px] border border-line bg-line lg:grid-cols-4">
        <Reading label="在线" sub={list.length - online.length ? `${list.length - online.length} 台停走` : "全部在走"}>
          {online.length}
          <span className="text-base font-normal text-ink-3"> / {list.length}</span>
        </Reading>
        <Reading label="实时带宽 ↓" sub={`↑ ${rate(tx)}`}>
          {rate(rx)}
        </Reading>
        <Reading label="本月流量" sub="入站 + 出站">
          {bytes(month)}
        </Reading>
        {admin ? (
          <Reading label="告警" sub={alerts.length ? "管理 → 告警事件" : "一切正常"}>
            <span className={alerts.length ? "text-crit" : undefined}>{alerts.length}</span>
          </Reading>
        ) : (
          <Reading label="地区" sub={`${list.length} 台服务器`}>
            {regions}
          </Reading>
        )}
      </section>

      {list.length === 0 ? (
        <EmptyState title="还没有服务器" icon={<IconServer size={18} />}>
          {admin ? (
            <>
              前往{" "}
              <a className="font-medium text-ink underline underline-offset-2" href={href({ page: "settings", tab: "servers" })}>
                管理 → 服务器
              </a>{" "}
              添加第一台，并在 VPS 上运行安装命令。
            </>
          ) : (
            "管理员添加服务器后会显示在这里。"
          )}
        </EmptyState>
      ) : (
        <>
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
            <label className="relative block sm:w-72">
              <IconSearch size={16} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink-3" />
              <span className="sr-only">搜索服务器</span>
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="搜索名称或地区"
                className="h-10 w-full rounded-md border border-line bg-surface pr-3 pl-9 text-sm placeholder:text-ink-3 focus:border-ink-3 focus:ring-3 focus:ring-ink/8 focus:outline-none sm:h-9"
              />
            </label>
            <Segmented
              label="按状态筛选"
              value={filter}
              onChange={setFilter}
              options={[
                { id: "all", label: `全部 ${list.length}` },
                { id: "online", label: `在线 ${online.length}` },
                { id: "offline", label: `离线 ${list.length - online.length}` },
              ]}
            />
          </div>
          {shown.length === 0 ? (
            <EmptyState title="没有匹配的服务器" icon={<IconSearch size={18} />} />
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 sm:gap-4 xl:grid-cols-3">
              {shown.map((s) => (
                <ServerCard key={s.id} s={s} now={now} recent={recent.get(s.id) ?? []} />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
