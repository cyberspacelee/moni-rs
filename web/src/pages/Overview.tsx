import { type ReactNode, useState } from "react";
import type { ServerStatus } from "../api/generated";
import { Flag, countryName } from "../components/Flag";
import { IconAlert, IconDown, IconGlobe, IconSearch, IconServer, IconUp } from "../components/icons";
import { Sparkline } from "../components/Sparkline";
import { Badge, Card, EmptyState, Meter, PageSpinner, Segmented, StatusDot, cx, levelColor } from "../components/ui";
import { ago, bytes, duration, pct, rate } from "../lib/format";
import { type RecentPoint, useLive } from "../lib/live";
import { href } from "../lib/router";
import { useNow } from "../lib/useNow";

function Stat({ label, icon, children, sub }: { label: string; icon: ReactNode; children: ReactNode; sub?: ReactNode }) {
  return (
    <Card className="px-4 py-3.5 sm:px-5 sm:py-4">
      <div className="flex items-center gap-1.5 text-xs text-ink-3">
        <span className="text-ink-3">{icon}</span>
        {label}
      </div>
      <div className="tabular mt-1.5 truncate font-mono text-xl font-semibold tracking-tight sm:text-2xl">{children}</div>
      {sub && <div className="tabular mt-0.5 truncate text-xs text-ink-3">{sub}</div>}
    </Card>
  );
}

function hostLine(s: ServerStatus): string {
  if (!s.host) return s.id;
  return [s.host.os, s.host.arch, `${s.host.cpu_cores} 核`].filter(Boolean).join(" · ");
}

function ServerCard({ s, now, recent }: { s: ServerStatus; now: number; recent: readonly RecentPoint[] }) {
  const m = s.metrics;
  const cpu = m?.cpu_pct ?? null;
  return (
    <a href={href({ page: "server", id: s.id })} data-server-card className="group block min-w-0 rounded-2xl">
      <Card
        className={cx(
          "h-full overflow-hidden transition-[border-color,box-shadow,transform] duration-200 group-hover:-translate-y-px group-hover:border-line-strong group-hover:shadow-[0_8px_30px_-12px_rgb(0_0_0/0.18)]",
          !s.online && "opacity-80",
        )}
      >
        <div className="flex items-start gap-3 p-4 pb-0 sm:p-5 sm:pb-0">
          <Flag code={s.country} className="mt-0.5 text-base" />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="truncate font-medium">{s.name}</span>
              {s.alerts_firing > 0 && (
                <Badge tone="critical">
                  <IconAlert size={11} />
                  {s.alerts_firing}
                </Badge>
              )}
            </div>
            <div className="truncate text-xs text-ink-3">
              {s.country && <span>{countryName(s.country)} · </span>}
              {hostLine(s)}
            </div>
          </div>
          <StatusDot online={s.online} />
        </div>

        {s.online && m ? (
          <>
            <div className="relative mt-3 h-14 px-0">
              <div className="absolute top-0 left-4 z-10 sm:left-5">
                <div className="tabular font-mono text-2xl leading-none font-semibold tracking-tight">
                  {cpu == null ? "–" : cpu.toFixed(0)}
                  <span className="ml-0.5 text-sm font-normal text-ink-3">%</span>
                </div>
                <div className="mt-1 text-[11px] text-ink-3">CPU · load {m.load1.toFixed(2)}</div>
              </div>
              <div className="absolute inset-x-0 bottom-0 h-11">
                <Sparkline values={recent.map((p) => p.cpu)} color={levelColor(cpu)} />
              </div>
            </div>
            <div className="space-y-3 border-t border-line px-4 pt-3.5 pb-4 sm:px-5">
              <Meter label="内存" value={pct(m.mem_used, m.mem_total)} detail={`${bytes(m.mem_used)} / ${bytes(m.mem_total, 0)}`} />
              <Meter label="磁盘" value={pct(m.disk_used, m.disk_total)} detail={`${bytes(m.disk_used, 0)} / ${bytes(m.disk_total, 0)}`} />
              <div className="tabular grid grid-cols-2 gap-2 pt-0.5 font-mono text-[11px]">
                <div className="flex items-center gap-1.5 rounded-lg bg-surface-2 px-2 py-1.5">
                  <IconDown size={12} className="text-[var(--series-1)]" />
                  <span className="truncate">{rate(m.net_rx_bps)}</span>
                </div>
                <div className="flex items-center gap-1.5 rounded-lg bg-surface-2 px-2 py-1.5">
                  <IconUp size={12} className="text-[var(--series-2)]" />
                  <span className="truncate">{rate(m.net_tx_bps)}</span>
                </div>
              </div>
              <div className="flex justify-between gap-2 text-[11px] text-ink-3">
                <span className="truncate">
                  本月 ↓{bytes(s.traffic_month.rx_bytes)} ↑{bytes(s.traffic_month.tx_bytes)}
                </span>
                <span className="shrink-0">运行 {duration(m.uptime_s)}</span>
              </div>
            </div>
          </>
        ) : (
          <div className="px-4 pt-6 pb-5 text-sm text-ink-3 sm:px-5">
            {s.online ? "等待数据…" : s.last_seen_ms ? `最后在线 ${ago(s.last_seen_ms, now)}` : "Agent 尚未连接"}
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
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4 lg:gap-4">
        <Stat label="在线" icon={<IconServer size={14} />} sub={`共 ${list.length} 台`}>
          {online.length}
          <span className="text-base font-normal text-ink-3">/{list.length}</span>
        </Stat>
        <Stat
          label="实时带宽"
          icon={<IconDown size={14} />}
          sub={
            <span className="inline-flex items-center gap-1">
              <IconUp size={11} /> {rate(tx)}
            </span>
          }
        >
          {rate(rx)}
        </Stat>
        <Stat label="本月流量" icon={<IconGlobe size={14} />} sub="入站 + 出站">
          {bytes(month)}
        </Stat>
        {admin ? (
          <Stat label="告警中" icon={<IconAlert size={14} />} sub={alerts.length ? "查看管理 → 告警事件" : "一切正常"}>
            <span className={alerts.length ? "text-crit" : undefined}>{alerts.length}</span>
          </Stat>
        ) : (
          <Stat label="覆盖地区" icon={<IconGlobe size={14} />} sub={`${list.length} 台服务器`}>
            {regions}
          </Stat>
        )}
      </div>

      {list.length === 0 ? (
        <EmptyState title="还没有服务器" icon={<IconServer size={18} />}>
          {admin ? (
            <>
              前往{" "}
              <a className="text-brand underline-offset-2 hover:underline" href={href({ page: "settings", tab: "servers" })}>
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
              <IconSearch size={15} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-ink-3" />
              <span className="sr-only">搜索服务器</span>
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="搜索名称或地区"
                className="h-10 w-full rounded-xl border border-line bg-surface pr-3 pl-9 text-sm shadow-xs placeholder:text-ink-3 focus:border-brand focus:ring-3 focus:ring-brand/15 focus:outline-none sm:h-9"
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
