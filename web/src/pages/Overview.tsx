import { SearchIcon, ServerIcon, TriangleAlertIcon } from "lucide-react";
import { type ReactNode, useState } from "react";
import type { ServerStatus } from "@/api/generated";
import { EmptyState, PageSpinner, Segmented, StatusDot, UsageMeter, levelColor } from "@/components/common";
import { Flag, countryName } from "@/components/Flag";
import { Sparkline } from "@/components/Sparkline";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { ago, bytes, clockTime, duration, pct, rate } from "@/lib/format";
import { type RecentPoint, useLive } from "@/lib/live";
import { href } from "@/lib/router";
import { useNow } from "@/lib/useNow";

/** One reading in the summary strip: hairline-separated, no box, the numeral carries it. */
function Reading({ label, children, sub }: { label: string; children: ReactNode; sub?: ReactNode }) {
  return (
    <div className="min-w-0 bg-card px-5 py-4">
      <div className="text-xs leading-4 text-muted-foreground">{label}</div>
      <div className="hero-num tabular mt-1 truncate text-xl leading-7 font-semibold tracking-tight sm:text-2xl sm:leading-8">{children}</div>
      {sub && <div className="tabular mt-0.5 truncate text-xs leading-4 text-muted-foreground">{sub}</div>}
    </div>
  );
}

/** A label / value line in the card's detail list, aligned with the meter rows above it. */
function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <dt className="shrink-0 text-muted-foreground">{label}</dt>
      <dd className="truncate text-ink-2">{children}</dd>
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
  return (
    <a href={href({ page: "server", id: s.id })} data-server-card className="group block min-w-0 rounded-2xl">
      <Card className="h-full gap-0 py-0 transition-[border-color,box-shadow] duration-150 group-hover:border-line-strong group-hover:shadow-card-hover">
        <div className="flex items-start gap-3 px-5 pt-5">
          <div className="min-w-0 flex-1">
            <div className="flex h-5 items-center gap-2">
              <span className="truncate leading-5 font-semibold">{s.name}</span>
              {s.alerts_firing > 0 && (
                <Badge variant="destructive">
                  <TriangleAlertIcon data-icon="inline-start" />
                  {s.alerts_firing}
                </Badge>
              )}
            </div>
            <div className="mt-1 flex h-4 items-center gap-1.5 text-xs leading-4 text-muted-foreground">
              {s.country && <Flag code={s.country} className="text-[13px]" />}
              <span className="truncate">{hostLine(s)}</span>
            </div>
          </div>
          <StatusDot online={s.online} />
        </div>

        {live ? (
          <>
            <div className="mt-4 flex items-end gap-4 px-5">
              <div className="shrink-0">
                <div className="hero-num tabular text-3xl leading-8 font-semibold tracking-tight">
                  {cpu == null ? "–" : cpu.toFixed(0)}
                  <span className="ml-0.5 text-base font-normal text-muted-foreground">%</span>
                </div>
                <div className="mt-1 text-xs leading-4 text-muted-foreground">CPU · 负载 {m.load1.toFixed(2)}</div>
              </div>
              <div className="h-12 min-w-0 flex-1">
                <Sparkline values={recent.map((p) => p.cpu)} color={levelColor(cpu)} />
              </div>
            </div>
            <div className="mt-4 space-y-3 border-t border-border px-5 pt-4 pb-5">
              <UsageMeter label="内存" value={pct(m.mem_used, m.mem_total)} detail={`${bytes(m.mem_used)} / ${bytes(m.mem_total, 0)}`} />
              <UsageMeter label="磁盘" value={pct(m.disk_used, m.disk_total)} detail={`${bytes(m.disk_used, 0)} / ${bytes(m.disk_total, 0)}`} />
              <dl className="tabular space-y-1.5 border-t border-border pt-3 text-xs leading-4">
                <Row label="网络">
                  ↓ {rate(m.net_rx_bps)}
                  <span className="mx-1.5 text-line-strong">/</span>↑ {rate(m.net_tx_bps)}
                </Row>
                <Row label="本月流量">
                  ↓ {bytes(s.traffic_month.rx_bytes)}
                  <span className="mx-1.5 text-line-strong">/</span>↑ {bytes(s.traffic_month.tx_bytes)}
                </Row>
                <Row label="运行时间">{duration(m.uptime_s)}</Row>
              </dl>
            </div>
          </>
        ) : (
          <div className="mt-4 border-t border-border px-5 pt-4 pb-5 text-sm leading-5 text-muted-foreground">
            {s.online
              ? "等待首帧数据…"
              : s.last_seen_ms
                ? `最后在线 ${clockTime(s.last_seen_ms, now)}（${ago(s.last_seen_ms, now)}）`
                : "尚未接入，在 VPS 上运行安装命令后会出现数据"}
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
        className="grid grid-cols-2 gap-px overflow-hidden card rounded-2xl border border-border bg-border shadow-card lg:grid-cols-4">
        <Reading label="在线" sub={list.length - online.length ? `${list.length - online.length} 台离线` : "全部在线"}>
          {online.length}
          <span className="text-base font-normal text-muted-foreground"> / {list.length}</span>
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
        <EmptyState title="还没有服务器" icon={<ServerIcon />}>
          {admin ? (
            <>
              前往{" "}
              <a className="font-medium text-primary underline-offset-4 hover:underline" href={href({ page: "settings", tab: "servers" })}>
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
            <InputGroup className="h-9 bg-card sm:w-72">
              <InputGroupInput type="search" aria-label="搜索服务器" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索名称或地区" />
              <InputGroupAddon>
                <SearchIcon />
              </InputGroupAddon>
            </InputGroup>
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
            <EmptyState title="没有匹配的服务器" icon={<SearchIcon />} />
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
