import type { ServerStatus } from "../api/generated";
import { Badge, Card, EmptyState, Meter, Spinner, StatusDot } from "../components/ui";
import { ago, bytes, duration, pct, rate } from "../lib/format";
import { useLive } from "../lib/live";
import { href } from "../lib/router";
import { useNow } from "../lib/useNow";

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <Card className="px-4 py-3">
      <div className="text-xs text-zinc-500">{label}</div>
      <div className="tabular mt-1 text-2xl font-semibold">{value}</div>
    </Card>
  );
}

function ServerCard({ s, now }: { s: ServerStatus; now: number }) {
  const m = s.metrics;
  return (
    <a href={href({ page: "server", id: s.id })} className="group block">
      <Card className="h-full p-4 transition-colors group-hover:border-zinc-300 dark:group-hover:border-zinc-700">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <div className="truncate font-medium">{s.name}</div>
            <div className="truncate text-xs text-zinc-500">
              {s.host ? `${s.host.os} · ${s.host.arch}` : s.id}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {s.alerts_firing > 0 && <Badge tone="critical">⚠ {s.alerts_firing}</Badge>}
            <StatusDot online={s.online} />
          </div>
        </div>

        {s.online && m ? (
          <div className="mt-4 space-y-3">
            <Meter label="CPU" value={m.cpu_pct} detail={`load ${m.load1.toFixed(2)}`} />
            <Meter label="内存" value={pct(m.mem_used, m.mem_total)} detail={bytes(m.mem_total, 0)} />
            <Meter label="磁盘" value={pct(m.disk_used, m.disk_total)} detail={bytes(m.disk_total, 0)} />
            <dl className="tabular grid grid-cols-2 gap-x-3 gap-y-1 pt-1 text-xs">
              <dt className="text-zinc-500">↓ 下载</dt>
              <dd className="text-right">{rate(m.net_rx_bps)}</dd>
              <dt className="text-zinc-500">↑ 上传</dt>
              <dd className="text-right">{rate(m.net_tx_bps)}</dd>
              <dt className="text-zinc-500">本月流量</dt>
              <dd className="text-right">
                {bytes(s.traffic_month.rx_bytes)} / {bytes(s.traffic_month.tx_bytes)}
              </dd>
              <dt className="text-zinc-500">运行</dt>
              <dd className="text-right">{duration(m.uptime_s)}</dd>
            </dl>
          </div>
        ) : (
          <div className="mt-4 text-sm text-zinc-500">
            {s.online ? "等待数据…" : s.last_seen_ms ? `最后在线 ${ago(s.last_seen_ms, now)}` : "Agent 尚未连接"}
          </div>
        )}
      </Card>
    </a>
  );
}

export function Overview() {
  const { ready, servers, alerts } = useLive();
  const now = useNow();
  if (!ready)
    return (
      <div className="grid place-items-center py-24">
        <Spinner />
      </div>
    );

  const list = [...servers.values()].sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name));
  const online = list.filter((s) => s.online).length;

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat label="服务器" value={list.length} />
        <Stat label="在线" value={online} />
        <Stat label="离线" value={list.length - online} />
        <Stat label="告警中" value={alerts.length} />
      </div>
      {list.length === 0 ? (
        <EmptyState title="还没有服务器">
          前往 <a className="underline" href={href({ page: "settings", tab: "servers" })}>设置 → 服务器</a> 添加第一台，并在 VPS 上运行安装命令。
        </EmptyState>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {list.map((s) => (
            <ServerCard key={s.id} s={s} now={now} />
          ))}
        </div>
      )}
    </div>
  );
}
