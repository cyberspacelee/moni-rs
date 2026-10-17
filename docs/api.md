# moni-rs API 契约（v1）

前后端之间的唯一契约。Rust 类型定义在 `crates/protocol/src/api.rs`，TS 类型由 ts-rs 生成到 `web/src/api/generated/`。
本文与代码不一致时，**以本文为准**，并修正代码。

约定：
- JSON 字段一律 snake_case；时间戳统一为 Unix 毫秒，字段名以 `_ms` 结尾；字节数用 `u64`，速率单位为 bytes/s。
- `u64`/`i64` 在 TS 中映射为 `number`（ts-rs 通过 `#[ts(type = "number")]` 指定）。
- 错误响应：HTTP 4xx/5xx，响应体 `{"error": "<code>", "message": "<可读说明>"}`。错误码：`unauthorized`、`forbidden`、`not_found`、`bad_request`、`conflict`、`rate_limited`、`not_configured`、`internal`。
- 鉴权：除 `/api/health`、`/api/auth/login` 外，所有 `/api/*` 和 `/ws/dashboard` 都需要管理员会话（Cookie `moni_session`）或 `Authorization: Bearer <ADMIN_PASSWORD>`。
- CSRF：使用 Cookie 鉴权时，`/ws/dashboard` 握手和所有非 GET 的 `/api/*` 请求都必须带 `Origin`，并且与请求目标源完全一致（缺失、`null` 或不一致 → `403 forbidden`）；带请求体的写操作必须使用 `Content-Type: application/json`（否则返回 `415`，错误码 `bad_request`）。Bearer 鉴权（CLI）不校验 Origin。`/api/auth/login` 同样校验 Origin（存在时必须一致）。

## 1. 认证

| 方法 | 路径 | 请求 | 响应 |
|---|---|---|---|
| POST | `/api/auth/login` | `{"password": string}` | `204` + `Set-Cookie`；失败返回 `401 unauthorized`；限流时返回 `429 rate_limited` |
| POST | `/api/auth/logout` | — | `204`，清除 Cookie |
| GET | `/api/auth/me` | — | `200 {"authenticated": true}`，或 `401` |
| GET | `/api/health` | — | `200 "ok"`（无需鉴权） |

## 2. 配置

`GET /api/config` → `AppConfig`
```ts
type AppConfig = {
  version: string;                 // worker 版本（CARGO_PKG_VERSION）
  report_interval_s: number;
  agent_release_repo: string | null;   // 例如 "owner/moni-rs"，用于生成安装命令
  retention: { minute_days: number; hour_days: number };
  notifications: { telegram: boolean; webhook: boolean };
};
```

## 3. 服务器

```ts
type HostInfo = {
  hostname: string; os: string; kernel: string; arch: string;
  cpu_model: string; cpu_cores: number; boot_time_ms: number;
};

type MetricsFrame = {
  seq: number; ts_ms: number; cpu_pct: number;
  mem_used: number; mem_total: number; swap_used: number; swap_total: number;
  disk_used: number; disk_total: number;
  net_rx_bps: number; net_tx_bps: number;
  load1: number; load5: number; load15: number;
  uptime_s: number; process_count: number;
};

type Traffic = { rx_bytes: number; tx_bytes: number; since_ms: number };  // since_ms = 当月 1 日 00:00 UTC

type ServerStatus = {
  id: string; name: string; created_ms: number;
  online: boolean; last_seen_ms: number | null;
  agent_version: string | null;
  host: HostInfo | null;
  metrics: MetricsFrame | null;     // 仅在线时非空
  traffic_month: Traffic;
  alerts_firing: number;            // 当前处于 firing 状态的告警数
};
```

| 方法 | 路径 | 请求 | 响应 |
|---|---|---|---|
| GET | `/api/servers` | — | `ServerStatus[]`（按 name 排序） |
| POST | `/api/servers` | `{"name": string, "id"?: string}` | `201 {"server": ServerStatus, "token": string}`。id 省略时由 name 生成 slug，冲突时追加 `-2`、`-3`…；id 已存在且是显式指定的 → `409 conflict` |
| PATCH | `/api/servers/:id` | `{"name"?: string}` | `200 ServerStatus` |
| DELETE | `/api/servers/:id` | — | `204`。断开 Agent 连接，删除该服务器的所有数据（包括 metrics、alert_state、alert_events），不发送 resolved 通知 |
| POST | `/api/servers/:id/token` | — | `200 {"token": string}`。轮换 token，断开当前 Agent（关闭码 4003） |
| GET | `/api/servers/:id/history?range=` | `range` ∈ `1h,6h,24h,7d,30d,90d`，默认 `24h` | `200 History` |

```ts
type HistoryRange = "1h" | "6h" | "24h" | "7d" | "30d" | "90d";
type HistoryPoint = {
  ts_ms: number;
  cpu_avg: number; cpu_max: number;
  mem_pct: number; swap_pct: number; disk_pct: number;
  rx_bytes: number; tx_bytes: number;       // 该桶的总字节数
  rx_bps_max: number; tx_bps_max: number;
  load1: number;
};
type History = { range: HistoryRange; step_ms: number; points: HistoryPoint[] };  // points 按 ts_ms 升序；缺失的桶不补点
```
`step_ms`：1h/6h/24h = 60000，7d = 600000，30d/90d = 3600000。

## 4. 告警

```ts
type AlertMetric = "cpu" | "mem" | "swap" | "disk" | "load1" | "net_rx" | "net_tx" | "offline";
type AlertOp = "gt" | "lt";
type AlertRule = {
  id: string; name: string;
  server_id: string | null;        // null 表示所有服务器
  metric: AlertMetric; op: AlertOp;
  threshold: number; recover_threshold: number | null;
  duration_s: number; enabled: boolean; created_ms: number;
};
type AlertRuleInput = Omit<AlertRule, "id" | "created_ms">;
type AlertEvent = {
  id: number; rule_id: string; rule_name: string;
  server_id: string; server_name: string;
  kind: "firing" | "resolved"; value: number | null; ts_ms: number;
};
type ActiveAlert = {
  rule_id: string; rule_name: string; server_id: string; server_name: string;
  metric: AlertMetric; value: number | null; since_ms: number;
};
```

| 方法 | 路径 | 请求 | 响应 |
|---|---|---|---|
| GET | `/api/alerts/rules` | — | `AlertRule[]` |
| POST | `/api/alerts/rules` | `AlertRuleInput` | `201 AlertRule` |
| PATCH | `/api/alerts/rules/:id` | `Partial<AlertRuleInput>` | `200 AlertRule` |
| DELETE | `/api/alerts/rules/:id` | — | `204`，同时清除该规则的 firing 状态 |
| GET | `/api/alerts/active` | — | `ActiveAlert[]` |
| GET | `/api/alerts/events?limit=50&before_id=` | — | `AlertEvent[]`（按 id 倒序，limit 最大 200） |
| POST | `/api/alerts/test` | — | `200 {"telegram": "ok" \| "disabled" \| string, "webhook": 同上}`（string 为错误信息） |

单位约定：cpu/mem/swap/disk 用百分比（0–100），load1 为原值，net_rx/net_tx 为 bytes/s，offline 规则只使用 `duration_s`。

## 5. Dashboard WebSocket

`GET /ws/dashboard`（需管理员会话；浏览器会自动带上同源 Cookie）。服务端只推送消息，客户端发送的消息会被忽略。

```ts
type DashboardEvent =
  | { type: "snapshot"; servers: ServerStatus[]; alerts: ActiveAlert[] }
  | { type: "server_upsert"; server: ServerStatus }       // 新增 / 改名 / 上线时推送完整对象
  | { type: "server_removed"; id: string }
  | { type: "online"; id: string; agent_version: string | null; host: HostInfo | null }
  | { type: "offline"; id: string; last_seen_ms: number }
  | { type: "metrics"; id: string; frame: MetricsFrame }
  | { type: "traffic"; id: string; traffic: Traffic }        // 每次写入 Rollup 后推送更新后的本月流量
  | { type: "alert"; kind: "firing" | "resolved"; alert: ActiveAlert };
```
`traffic`：在线服务器的本月流量由 Hub 在连接 attachment 中增量维护（连接建立或跨月时做一次 SUM 重算，之后每次写入新 Rollup 时累加），因此推送不产生额外的 SQLite 读取。
连接建立后会立即收到一条 `snapshot`。断线重连后，同样用 snapshot 整体替换本地状态。

## 6. Agent WebSocket

`GET /ws/agent`，Header 为 `Authorization: Bearer mn_<server_id>_<secret>`。消息格式见 `docs/design.md` §2.1。
关闭码：`4000` 连接失活，`4001` 协议不兼容，`4002` 被新连接替换，`4003` token 被轮换或服务器被删除。
