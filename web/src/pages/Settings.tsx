import { type FormEvent, useCallback, useEffect, useState } from "react";
import type {
  AlertEvent,
  AlertMetric,
  AlertOp,
  AlertRule,
  AlertRuleInput,
  AlertTestResponse,
  AppConfig,
  ServerStatus,
} from "../api/generated";
import { Flag } from "../components/Flag";
import { StationClock } from "../components/StationClock";
import {
  IconAlert,
  IconBell,
  IconCheck,
  IconHistory,
  IconKey,
  IconPencil,
  IconPlus,
  IconPower,
  IconSend,
  IconServer,
  IconTrash,
} from "../components/icons";
import {
  Badge,
  Button,
  Card,
  CopyButton,
  EmptyState,
  ErrorText,
  Field,
  Input,
  Modal,
  Select,
  Spinner,
  StatusDot,
  cx,
} from "../components/ui";
import { api, errorText } from "../lib/api";
import { ago, clockTime, dateTime } from "../lib/format";
import { useLive } from "../lib/live";
import { href, type SettingsTab } from "../lib/router";
import { useNow } from "../lib/useNow";

const TABS: { id: SettingsTab; label: string; icon: React.ReactNode }[] = [
  { id: "servers", label: "服务器", icon: <IconServer size={16} /> },
  { id: "alerts", label: "告警规则", icon: <IconBell size={16} /> },
  { id: "events", label: "告警事件", icon: <IconHistory size={16} /> },
  { id: "notify", label: "通知", icon: <IconSend size={16} /> },
];

export function Settings({ tab, config }: { tab: SettingsTab; config: AppConfig | null }) {
  return (
    <div className="space-y-5 sm:space-y-6">
      <h1 className="text-xl leading-7 font-semibold tracking-tight sm:text-2xl sm:leading-8">管理</h1>
      <nav className="no-scrollbar -mx-4 flex gap-1 overflow-x-auto border-b border-line px-4 sm:mx-0 sm:px-0">
        {TABS.map((t) => (
          <a
            key={t.id}
            href={href({ page: "settings", tab: t.id })}
            aria-current={tab === t.id ? "page" : undefined}
            className={cx(
              "-mb-px inline-flex h-10 shrink-0 items-center gap-1.5 border-b-2 px-3 text-sm leading-none transition-colors",
              tab === t.id ? "border-signal font-medium text-ink" : "border-transparent text-ink-3 hover:text-ink",
            )}
          >
            {t.icon}
            {t.label}
          </a>
        ))}
      </nav>
      {tab === "servers" && <ServersTab config={config} />}
      {tab === "alerts" && <AlertsTab />}
      {tab === "events" && <EventsTab />}
      {tab === "notify" && <NotifyTab config={config} />}
    </div>
  );
}

function Toolbar({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <p className="text-sm text-ink-3">{text}</p>
      {children}
    </div>
  );
}

/* ---------------------------------------------------------------- servers */

function installCommand(config: AppConfig | null, token: string): string {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const server = `${proto}://${location.host}/ws/agent`;
  const repo = config?.agent_release_repo ?? "<owner>/moni-rs";
  return [
    `curl -fsSL https://github.com/${repo}/releases/latest/download/install-agent.sh \\`,
    `  | sudo sh -s -- --repo ${repo} \\`,
    `      --server ${server} \\`,
    `      --token ${token}`,
  ].join("\n");
}

function TokenReveal({ config, token }: { config: AppConfig | null; token: string }) {
  const cmd = installCommand(config, token);
  return (
    <div className="space-y-3">
      <p className="flex items-center gap-2 rounded-md bg-[color-mix(in_oklab,var(--status-warning)_14%,transparent)] px-3 py-2 text-sm text-ink">
        <IconAlert size={16} className="shrink-0 text-warn" />
        Token 只显示这一次，请立即保存。
      </p>
      <Field label="Token">
        <div className="flex items-center gap-2">
          <code className="min-w-0 flex-1 truncate rounded-md bg-surface-2 px-2.5 py-2 font-mono text-xs">{token}</code>
          <CopyButton text={token} />
        </div>
      </Field>
      <div className="space-y-1.5">
        <div className="flex items-center justify-between">
          <span className="text-xs font-medium text-ink-2">安装命令（在 VPS 上以 root 运行）</span>
          <CopyButton text={cmd} />
        </div>
        <pre className="overflow-x-auto rounded-md bg-shell p-3.5 font-mono text-xs leading-relaxed text-shell-ink">{cmd}</pre>
      </div>
      {!config?.agent_release_repo && (
        <p className="text-xs text-ink-3">未配置 AGENT_RELEASE_REPO，请把命令中的 &lt;owner&gt; 替换为你的 GitHub 仓库。</p>
      )}
    </div>
  );
}

function ServersTab({ config }: { config: AppConfig | null }) {
  const { servers, ready } = useLive();
  const now = useNow();
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [customId, setCustomId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [token, setToken] = useState<{ server: string; token: string } | null>(null);
  const [editing, setEditing] = useState<ServerStatus | null>(null);
  const [editName, setEditName] = useState("");

  const list = [...servers.values()].sort((a, b) => a.name.localeCompare(b.name));

  const create = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const res = await api.createServer(name.trim(), customId.trim() || undefined);
      setAdding(false);
      setName("");
      setCustomId("");
      setToken({ server: res.server.name, token: res.token });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const rotate = async (s: ServerStatus) => {
    if (!confirm(`为「${s.name}」生成新 token？当前 Agent 将被断开，需要用新 token 重新安装。`)) return;
    try {
      const res = await api.rotateToken(s.id);
      setToken({ server: s.name, token: res.token });
    } catch (err) {
      alert(errorText(err));
    }
  };

  const remove = async (s: ServerStatus) => {
    if (!confirm(`删除「${s.name}」及其全部历史数据？此操作不可撤销。`)) return;
    try {
      await api.deleteServer(s.id);
    } catch (err) {
      alert(errorText(err));
    }
  };

  const rename = async (e: FormEvent) => {
    e.preventDefault();
    if (!editing) return;
    setBusy(true);
    setError("");
    try {
      await api.renameServer(editing.id, editName.trim());
      setEditing(null);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <Toolbar text="每台服务器使用独立 token；轮换或删除会立即断开对应 Agent。">
        <Button variant="primary" onClick={() => (setError(""), setAdding(true))}>
          <IconPlus size={16} />
          添加服务器
        </Button>
      </Toolbar>

      {!ready ? (
        <Spinner />
      ) : list.length === 0 ? (
        <EmptyState title="还没有服务器" icon={<IconServer size={18} />} />
      ) : (
        <Card className="divide-y divide-line">
          {list.map((s) => (
            <div key={s.id} data-server-row className="flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-center sm:px-5">
              <div className="flex min-w-0 flex-1 items-center gap-3">
                <StationClock
                  ms={s.online && s.metrics ? s.metrics.ts_ms : s.last_seen_ms}
                  running={s.online && s.metrics != null}
                  size={32}
                  label={s.online ? "运行中" : s.last_seen_ms ? `停于 ${clockTime(s.last_seen_ms, now)}` : "从未连接"}
                />
                <div className="min-w-0 flex-1">
                  <a href={href({ page: "server", id: s.id })} className="block truncate leading-5 font-medium underline-offset-2 hover:underline">
                    {s.name}
                  </a>
                  <div className="mt-0.5 flex h-4 items-center gap-1.5 text-xs leading-4 text-ink-3">
                    {s.country && <Flag code={s.country} className="text-[13px]" />}
                    <span className="truncate font-mono">{s.id}</span>
                    {s.agent_version && <span className="shrink-0">· v{s.agent_version}</span>}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5 text-xs text-ink-3">
                  <StatusDot online={s.online} />
                  {!s.online && <span className="hidden sm:inline">· {s.last_seen_ms ? ago(s.last_seen_ms, now) : "从未连接"}</span>}
                </div>
              </div>
              <div className="-mx-1.5 flex gap-1 sm:mx-0">
                <Button size="sm" variant="ghost" onClick={() => (setError(""), setEditName(s.name), setEditing(s))}>
                  <IconPencil size={14} />
                  重命名
                </Button>
                <Button size="sm" variant="ghost" onClick={() => void rotate(s)}>
                  <IconKey size={14} />
                  轮换 Token
                </Button>
                <Button size="sm" variant="danger" className="ml-auto sm:ml-0" onClick={() => void remove(s)}>
                  <IconTrash size={14} />
                  删除
                </Button>
              </div>
            </div>
          ))}
        </Card>
      )}

      <Modal open={adding} onClose={() => setAdding(false)} title="添加服务器">
        <form onSubmit={create} className="space-y-4">
          <Field label="名称">
            <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="例如 香港 01" />
          </Field>
          <Field label="ID（可选）" hint="小写字母、数字和 -，最长 32 位；留空则根据名称生成">
            <Input value={customId} onChange={(e) => setCustomId(e.target.value)} placeholder="hk-01" />
          </Field>
          <ErrorText>{error}</ErrorText>
          <div className="flex justify-end gap-2 pt-1">
            <Button type="button" onClick={() => setAdding(false)}>
              取消
            </Button>
            <Button type="submit" variant="primary" disabled={busy || !name.trim()}>
              创建
            </Button>
          </div>
        </form>
      </Modal>

      <Modal open={token != null} onClose={() => setToken(null)} title={`${token?.server ?? ""} 的安装信息`}>
        {token && <TokenReveal config={config} token={token.token} />}
        <div className="flex justify-end">
          <Button variant="primary" onClick={() => setToken(null)}>
            我已保存
          </Button>
        </div>
      </Modal>

      <Modal open={editing != null} onClose={() => setEditing(null)} title="重命名">
        <form onSubmit={rename} className="space-y-4">
          <Field label="名称">
            <Input autoFocus value={editName} onChange={(e) => setEditName(e.target.value)} />
          </Field>
          <ErrorText>{error}</ErrorText>
          <div className="flex justify-end gap-2 pt-1">
            <Button type="button" onClick={() => setEditing(null)}>
              取消
            </Button>
            <Button type="submit" variant="primary" disabled={busy || !editName.trim()}>
              保存
            </Button>
          </div>
        </form>
      </Modal>
    </div>
  );
}

/* ----------------------------------------------------------------- alerts */

const METRICS: { id: AlertMetric; label: string; unit: string }[] = [
  { id: "cpu", label: "CPU 使用率", unit: "%" },
  { id: "mem", label: "内存使用率", unit: "%" },
  { id: "swap", label: "Swap 使用率", unit: "%" },
  { id: "disk", label: "磁盘使用率", unit: "%" },
  { id: "load1", label: "1 分钟负载", unit: "" },
  { id: "net_rx", label: "下载速率", unit: "B/s" },
  { id: "net_tx", label: "上传速率", unit: "B/s" },
  { id: "offline", label: "离线", unit: "" },
];
const metricInfo = (m: AlertMetric) => METRICS.find((x) => x.id === m) ?? { id: m, label: m, unit: "" };

function describeRule(r: AlertRule): string {
  if (r.metric === "offline") return `离线超过 ${r.duration_s} 秒`;
  const { label, unit } = metricInfo(r.metric);
  const op = r.op === "gt" ? ">" : "<";
  const dur = r.duration_s > 0 ? `，持续 ${r.duration_s} 秒` : "";
  return `${label} ${op} ${r.threshold}${unit}${dur}`;
}

const EMPTY_RULE: AlertRuleInput = {
  name: "",
  server_id: null,
  metric: "cpu",
  op: "gt",
  threshold: 90,
  recover_threshold: null,
  duration_s: 300,
  enabled: true,
};

function RuleForm({
  initial,
  servers,
  onSubmit,
  onCancel,
}: {
  initial: AlertRuleInput;
  servers: ServerStatus[];
  onSubmit: (r: AlertRuleInput) => Promise<void>;
  onCancel: () => void;
}) {
  const [r, setR] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const offline = r.metric === "offline";
  const unit = metricInfo(r.metric).unit;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await onSubmit(r);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const num = (v: string) => (v.trim() === "" ? null : Number(v));

  return (
    <form onSubmit={submit} className="space-y-4">
      <Field label="名称">
        <Input autoFocus value={r.name} onChange={(e) => setR({ ...r, name: e.target.value })} placeholder="CPU 过高" />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label="服务器">
          <Select value={r.server_id ?? ""} onChange={(e) => setR({ ...r, server_id: e.target.value || null })}>
            <option value="">全部服务器</option>
            {servers.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="指标">
          <Select value={r.metric} onChange={(e) => setR({ ...r, metric: e.target.value as AlertMetric })}>
            {METRICS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </Select>
        </Field>
      </div>
      {!offline && (
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
          <Field label="条件">
            <Select value={r.op} onChange={(e) => setR({ ...r, op: e.target.value as AlertOp })}>
              <option value="gt">大于</option>
              <option value="lt">小于</option>
            </Select>
          </Field>
          <Field label={`阈值${unit ? `（${unit}）` : ""}`}>
            <Input
              type="number"
              step="any"
              required
              value={r.threshold}
              onChange={(e) => setR({ ...r, threshold: Number(e.target.value) })}
            />
          </Field>
          <Field label="恢复阈值">
            <Input
              type="number"
              step="any"
              placeholder="默认 ±10%"
              value={r.recover_threshold ?? ""}
              onChange={(e) => setR({ ...r, recover_threshold: num(e.target.value) })}
            />
          </Field>
        </div>
      )}
      <Field label={offline ? "离线多久后告警（秒）" : "持续时间（秒）"} hint={offline ? undefined : "超过阈值并持续该时长才触发；0 表示立即触发"}>
        <Input
          type="number"
          min={0}
          required
          value={r.duration_s}
          onChange={(e) => setR({ ...r, duration_s: Math.max(0, Math.round(Number(e.target.value))) })}
        />
      </Field>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" className="size-4 accent-[var(--signal)]" checked={r.enabled} onChange={(e) => setR({ ...r, enabled: e.target.checked })} />
        启用
      </label>
      <ErrorText>{error}</ErrorText>
      <div className="flex justify-end gap-2 pt-1">
        <Button type="button" onClick={onCancel}>
          取消
        </Button>
        <Button type="submit" variant="primary" disabled={busy || !r.name.trim()}>
          保存
        </Button>
      </div>
    </form>
  );
}

function AlertsTab() {
  const { servers } = useLive();
  const [rules, setRules] = useState<AlertRule[] | null>(null);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<AlertRule | "new" | null>(null);
  const serverList = [...servers.values()].sort((a, b) => a.name.localeCompare(b.name));

  const load = useCallback(() => {
    api.rules().then(setRules, (e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);

  const save = async (input: AlertRuleInput) => {
    if (editing === "new") await api.createRule(input);
    else if (editing) await api.updateRule(editing.id, input);
    setEditing(null);
    load();
  };

  const toggle = async (r: AlertRule) => {
    try {
      // Send the whole rule: for nullable fields an explicit null means "clear", not "unchanged".
      const { id, created_ms: _created, ...input } = r;
      await api.updateRule(id, { ...input, enabled: !r.enabled });
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const remove = async (r: AlertRule) => {
    if (!confirm(`删除规则「${r.name}」？`)) return;
    try {
      await api.deleteRule(r.id);
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const serverName = (id: string | null) => (id == null ? "全部服务器" : (servers.get(id)?.name ?? id));

  return (
    <div className="space-y-4">
      <Toolbar text="规则对实时数据求值，状态变化时发送通知。">
        <Button variant="primary" onClick={() => setEditing("new")}>
          <IconPlus size={16} />
          添加规则
        </Button>
      </Toolbar>
      <ErrorText>{error}</ErrorText>
      {rules == null ? (
        !error && <Spinner />
      ) : rules.length === 0 ? (
        <EmptyState title="没有告警规则" icon={<IconBell size={18} />} />
      ) : (
        <Card className="divide-y divide-line">
          {rules.map((r) => (
            <div key={r.id} data-rule-row className="flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-center sm:px-5">
              <div className="flex min-w-0 flex-1 items-center gap-3">
                <span
                  aria-hidden
                  className={cx(
                    "grid size-8 shrink-0 place-items-center rounded-md border",
                    r.enabled ? "border-line bg-surface-2 text-ink" : "border-dashed border-line-strong text-ink-3",
                  )}
                >
                  <IconBell size={16} />
                </span>
                <div className="min-w-0">
                  <div className="flex h-5 flex-wrap items-center gap-2 text-sm leading-5 font-medium">
                    {r.name}
                    {!r.enabled && <Badge>已停用</Badge>}
                  </div>
                  <div className="mt-0.5 text-xs leading-4 text-ink-3">
                    {describeRule(r)} · {serverName(r.server_id)}
                  </div>
                </div>
              </div>
              <div className="-mx-1.5 flex gap-1 sm:mx-0">
                <Button size="sm" variant="ghost" onClick={() => void toggle(r)}>
                  <IconPower size={14} />
                  {r.enabled ? "停用" : "启用"}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setEditing(r)}>
                  <IconPencil size={14} />
                  编辑
                </Button>
                <Button size="sm" variant="danger" className="ml-auto sm:ml-0" onClick={() => void remove(r)}>
                  <IconTrash size={14} />
                  删除
                </Button>
              </div>
            </div>
          ))}
        </Card>
      )}
      <Modal open={editing != null} onClose={() => setEditing(null)} title={editing === "new" ? "添加规则" : "编辑规则"}>
        {editing != null && (
          <RuleForm
            initial={editing === "new" ? EMPTY_RULE : editing}
            servers={serverList}
            onSubmit={save}
            onCancel={() => setEditing(null)}
          />
        )}
      </Modal>
    </div>
  );
}

/* ----------------------------------------------------------------- events */

function EventsTab() {
  const { alerts } = useLive();
  const now = useNow();
  const [events, setEvents] = useState<AlertEvent[] | null>(null);
  const [more, setMore] = useState(true);
  const [error, setError] = useState("");
  const PAGE = 50;

  useEffect(() => {
    api.events(PAGE).then(
      (e) => (setEvents(e), setMore(e.length === PAGE)),
      (e) => setError(errorText(e)),
    );
  }, [alerts.length]);

  const loadMore = async () => {
    const last = events?.at(-1);
    if (!last) return;
    try {
      const next = await api.events(PAGE, last.id);
      setEvents([...(events ?? []), ...next]);
      setMore(next.length === PAGE);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="space-y-6">
      <section className="space-y-2">
        <h2 className="text-sm font-semibold">当前告警</h2>
        {alerts.length === 0 ? (
          <p className="flex items-center gap-2 text-sm text-ink-3">
            <IconCheck size={16} className="text-good" />
            一切正常。
          </p>
        ) : (
          <Card className="divide-y divide-line">
            {alerts.map((a) => (
              <div key={`${a.rule_id}-${a.server_id}`} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3 text-sm sm:px-5">
                <Badge tone="critical">
                  <IconAlert size={12} />
                  告警中
                </Badge>
                <a className="font-medium underline-offset-2 hover:underline" href={href({ page: "server", id: a.server_id })}>
                  {a.server_name}
                </a>
                <span>{a.rule_name}</span>
                {a.value != null && <span className="tabular text-xs text-ink-3">当前 {a.value.toFixed(1)}</span>}
                <span className="text-xs text-ink-3 sm:ml-auto">开始于 {ago(a.since_ms, now)}</span>
              </div>
            ))}
          </Card>
        )}
      </section>
      <section className="space-y-2">
        <h2 className="text-sm font-semibold">历史事件</h2>
        <ErrorText>{error}</ErrorText>
        {events == null ? (
          !error && <Spinner />
        ) : events.length === 0 ? (
          <p className="text-sm text-ink-3">暂无事件。</p>
        ) : (
          <Card className="divide-y divide-line">
            {events.map((e) => (
              <div key={e.id} className="grid grid-cols-[auto_1fr_auto] items-center gap-x-3 gap-y-0.5 px-4 py-2.5 text-sm sm:grid-cols-[10.5rem_5rem_minmax(0,12rem)_1fr_4rem] sm:px-5">
                <span className="tabular col-span-3 text-xs text-ink-3 sm:col-span-1">{dateTime(e.ts_ms)}</span>
                <span>
                  {e.kind === "firing" ? (
                    <Badge tone="critical">
                      <IconAlert size={12} />
                      触发
                    </Badge>
                  ) : (
                    <Badge tone="good">
                      <IconCheck size={12} />
                      恢复
                    </Badge>
                  )}
                </span>
                <span className="min-w-0 truncate">
                  <span className="font-medium">{e.server_name}</span>
                  <span className="text-ink-3 sm:hidden"> · {e.rule_name}</span>
                </span>
                <span className="hidden min-w-0 truncate text-ink-2 sm:block">{e.rule_name}</span>
                <span className="tabular text-right text-xs text-ink-3">{e.value?.toFixed(1) ?? ""}</span>
              </div>
            ))}
          </Card>
        )}
        {events && more && (
          <Button size="sm" onClick={() => void loadMore()}>
            加载更多
          </Button>
        )}
      </section>
    </div>
  );
}

/* ----------------------------------------------------------------- notify */

function NotifyTab({ config }: { config: AppConfig | null }) {
  const [result, setResult] = useState<AlertTestResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const test = async () => {
    setBusy(true);
    setError("");
    try {
      setResult(await api.testNotify());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const channels = [
    { name: "Telegram", enabled: config?.notifications.telegram ?? false, secrets: "TG_BOT_TOKEN、TG_CHAT_ID", res: result?.telegram },
    { name: "Webhook", enabled: config?.notifications.webhook ?? false, secrets: "WEBHOOK_URL", res: result?.webhook },
  ];

  return (
    <div className="max-w-2xl space-y-4">
      <p className="text-sm leading-6 text-ink-3">
        通知渠道通过 Worker Secret 配置（GitHub Secrets → 重新运行 Deploy，或 <code className="rounded bg-surface-2 px-1 py-0.5 font-mono text-xs text-ink-2">wrangler secret put</code>）。
      </p>
      <Card className="divide-y divide-line">
        {channels.map((c) => (
          <div key={c.name} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-4 py-3.5 text-sm sm:px-5">
            <span className="w-24 leading-5 font-medium">{c.name}</span>
            {c.enabled ? (
              <Badge tone="good">
                <IconCheck size={12} />
                已配置
              </Badge>
            ) : (
              <Badge>未配置</Badge>
            )}
            <code className="w-full font-mono text-xs leading-5 text-ink-3 sm:w-auto">{c.secrets}</code>
            {c.res && (
              <span className={cx("text-xs sm:ml-auto", c.res === "ok" ? "text-good" : c.res === "disabled" ? "text-ink-3" : "text-crit")}>
                {c.res === "ok" ? "发送成功" : c.res === "disabled" ? "未启用" : c.res}
              </span>
            )}
          </div>
        ))}
      </Card>
      <ErrorText>{error}</ErrorText>
      <Button onClick={() => void test()} disabled={busy}>
        <IconSend size={16} />
        {busy ? "发送中…" : "发送测试通知"}
      </Button>
      {config && (
        <p className="text-xs text-ink-3">
          上报间隔 {config.report_interval_s} 秒 · 分钟数据保留 {config.retention.minute_days} 天 · 小时数据保留 {config.retention.hour_days} 天
        </p>
      )}
    </div>
  );
}
