# moni-rs 详细设计（v1 完整实现）

> 状态：已定稿，作为 v1 实现依据（2026-10-17）。
> 决策依据见 `docs/adr/`，平台事实见 `docs/research.md`，PoC 结论见 `docs/poc-report.md`，HTTP/WS 契约见 `docs/api.md`。

## 0. v1 范围

| 范围内 | 范围外（后续版本） |
|---|---|
| Agent：采集、实时帧、Agent 端分钟聚合 + ACK 重发、自动重连、systemd 安装 | Agent 本地磁盘持久化队列、自动升级 |
| 每台服务器独立 token（HMAC 存储），服务器增删改、轮换 token | 多用户 / 多租户 |
| 管理员密码登录（签名 Cookie），Bearer 方式供 CLI 使用 | OAuth、Cloudflare Access 集成（文档说明即可） |
| 实时看板（WS）、历史曲线（1h～90d）、本月流量 | 公开状态页 |
| 告警规则（阈值 + 持续时间 + 恢复阈值）、离线告警、Telegram / Webhook 通知 | 外部拨测（HTTP/TCP/Ping） |
| GitHub Actions：CI、一键部署、Agent 发布 | Analytics Engine 后端 |

## 1. 仓库结构

```
moni-rs/
├── Cargo.toml                  # workspace；default-members 不含 worker
├── crates/
│   ├── protocol/               # 线上协议 + HTTP DTO（serde）；feature "ts" 导出 TS 类型
│   ├── agent/                  # native agent
│   └── worker/                 # workers-rs：Worker 入口 + Hub DO
│       ├── wrangler.jsonc      # assets.directory → ../../web/dist
│       └── .dev.vars.example
├── web/                        # React 19 + TS + Tailwind v4 + Vite（前端）
│   └── src/api/generated/      # 由 protocol 的 ts-rs 生成，勿手改
├── scripts/                    # install-agent.sh、e2e 脚本
├── docs/
└── .github/workflows/          # ci.yml / deploy.yml / release-agent.yml
```

## 2. 协议（`crates/protocol`）

所有 JSON 时间戳统一为 **Unix 毫秒（`*_ms`）**。分钟桶、小时桶也用桶起点的毫秒值。

### 2.1 Agent ↔ Hub（WS 文本帧，`#[serde(tag = "type", rename_all = "snake_case")]`）

```rust
pub const PROTOCOL_VERSION: u16 = 2;

// Agent → Hub
enum AgentMessage {
    Hello { protocol: u16, agent_version: String, host: HostInfo },
    Metrics(MetricsFrame),         // 实时帧，默认每 5 秒一帧，只用于展示 / 告警
    Rollup(MinuteRollup),          // 分钟聚合，每分钟一条；Hub 确认前会重发
}

// Hub → Agent
enum HubMessage {
    Welcome { server_id: String, report_interval_s: u32 },
    Ack { minute_ms: i64 },         // 确认该分钟已写入（幂等）
}

struct MetricsFrame {
    seq: u64, ts_ms: i64,
    cpu_pct: f32,
    mem_used: u64, mem_total: u64, swap_used: u64, swap_total: u64,
    disk_used: u64, disk_total: u64,
    net_rx_bps: u64, net_tx_bps: u64,          // 帧窗口内的平均速率
    load1: f32, load5: f32, load15: f32,
    uptime_s: u64, process_count: u32,
}

struct MinuteRollup {
    minute_ms: i64,                // 分钟起点
    samples: u32,                  // 本分钟 1 秒采样数
    cpu_avg: f32, cpu_max: f32,
    mem_used_avg: u64, mem_total: u64,
    swap_used_avg: u64, swap_total: u64,
    disk_used: u64, disk_total: u64,           // 分钟末快照
    rx_bytes: u64, tx_bytes: u64,              // 本分钟实际字节数（计数器差值，已处理回绕/重置）
    rx_bps_max: u64, tx_bps_max: u64,          // 1 秒粒度峰值
    load1_avg: f32,
}
```

`HostInfo { hostname, os, kernel, arch, cpu_model, cpu_cores, boot_time_ms }`。

### 2.2 HTTP DTO 与 Dashboard 事件

详见 `docs/api.md`。所有 DTO 都定义在 `protocol::api` 中；开启 `ts` feature 后，`cargo test -p moni-protocol --features ts` 会生成 `web/src/api/generated/*.ts`。

## 3. Agent

### 3.1 采集
- tokio `current_thread`。用 `sysinfo` 每 **1 秒**采样一次 CPU、内存和网卡计数器；磁盘每 10 秒采一次。
- 网卡过滤：排除 `lo`、`docker*`、`veth*`、`br-*`、`virbr*`、`cni*`、`flannel*`、`tun*`、`wg*`；可通过 `MONI_IFACES` 白名单覆盖。
- 计数器差值：当前值 < 上一次值（重置 / 网卡重建）时，这一次差值记为 0。
- 磁盘：只统计 `/dev/*` 的物理挂载点并按设备去重；可用 `MONI_DISKS` 指定挂载点覆盖。

### 3.2 发送
- 每 `report_interval_s`（默认 5，以 Welcome 下发的值为准）发送一个 `Metrics` 帧，内容取窗口内平均值。
- 分钟边界：把这一分钟的 1 秒样本汇总成 `MinuteRollup`，推入**待确认队列**（有界，默认 1440 条，满了丢弃最旧的），再发送。
- 收到 `Ack{minute_ms}` 后从队列中删除对应条目。重连并完成 Hello/Welcome 后，按时间顺序重发队列中的全部条目。
- 30 秒发一次 WS 协议 Ping（不计费、不唤醒 DO）。重连采用指数退避 1s → 60s，并加抖动。interval 使用 `MissedTickBehavior::Skip`。
- 关闭码：`4001` 协议不兼容 → 退出码 2，不重试；`4003` token 失效 → 退出码 3，不重试；`4002` 被替换 → 延迟 30 秒再重连。

### 3.3 配置
CLI 参数优先，其次环境变量：`MONI_SERVER`（wss URL）、`MONI_TOKEN`、`MONI_INTERVAL`、`MONI_IFACES`、`MONI_DISKS`、`RUST_LOG`。
systemd 单元通过 `EnvironmentFile=/etc/moni/agent.env` 读取配置。

## 4. Worker（无状态入口）

- 处理路由、鉴权、静态资源回退，其余请求都转发给单例 Hub（`get_by_name("hub")`）。
- **管理员会话**：`POST /api/auth/login` 校验 `ADMIN_PASSWORD`，比较时间与输入无关，成功后设置 Cookie：
  `moni_session=<exp_ms>.<base64url(HMAC-SHA256(AUTH_SECRET, "session:" + exp_ms))>`，属性 `HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=30d`。本地 http 开发时不加 `Secure`。
  `Authorization: Bearer <ADMIN_PASSWORD>` 视为同等权限（供 CLI / 脚本使用）。
- **Agent token**：格式 `mn_<server_id>_<43 字符 base64url 随机串>`。Worker 解析出 server_id，计算 `token_hash = hex(HMAC-SHA256(AUTH_SECRET, token))`，通过内部 header `x-moni-server-id` 和 `x-moni-token-hash` 转发给 Hub，由 Hub 比对存储的哈希。客户端带来的这两个 header 一律先删除。
- 登录接口限流：同一 IP 每分钟最多 10 次失败，计数保存在 Hub 内存里（丢失可以接受）。
- 必需 Secret：`ADMIN_PASSWORD`、`AUTH_SECRET`（≥32 字节）。缺少时，`/api/*` 返回 `503 {"error":"not_configured"}`。

## 5. Hub DO

### 5.1 状态位置（ADR-0002 / ADR-0006）
| 状态 | 存放位置 | 原因 |
|---|---|---|
| 服务器、token 哈希、告警规则、告警状态、历史数据 | SQLite | 持久 |
| 连接身份、`last_seen_ms`、`closing`、latest 帧、告警 pending 计时 | WS attachment | 跨休眠保留，不占写入额度；丢失后可以重建 |
| 登录限流计数、调试计数 | 实例字段 | 丢失可以接受 |

**禁止**把需要正确性的状态放在实例字段中。Hub 代码不允许出现 `unwrap`/`expect`/`panic!`，因为 panic 会让 DO 实例终止。

### 5.2 SQLite 表
```sql
CREATE TABLE servers (
  id TEXT PRIMARY KEY,            -- [a-z0-9-]{1,32}
  name TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  created_ms INTEGER NOT NULL,
  last_seen_ms INTEGER,
  host_json TEXT,
  agent_version TEXT
);
CREATE TABLE metrics_1m (
  server_id TEXT NOT NULL, ts_ms INTEGER NOT NULL,
  cpu_avg REAL, cpu_max REAL, mem_pct REAL, swap_pct REAL, disk_pct REAL,
  rx_bytes INTEGER, tx_bytes INTEGER, rx_bps_max INTEGER, tx_bps_max INTEGER, load1 REAL,
  PRIMARY KEY (server_id, ts_ms)
) WITHOUT ROWID;
CREATE TABLE metrics_1h ( ...同列，ts_ms 为小时起点，cpu_max/bps_max 取 max，bytes 取 sum，其他取 avg... ) WITHOUT ROWID;
CREATE TABLE dirty_hours (server_id TEXT, hour_ms INTEGER, PRIMARY KEY (server_id, hour_ms)) WITHOUT ROWID;
CREATE TABLE alert_rules (
  id TEXT PRIMARY KEY, name TEXT NOT NULL,
  server_id TEXT,                 -- NULL 表示所有服务器
  metric TEXT NOT NULL,           -- cpu|mem|swap|disk|load1|net_rx|net_tx|offline
  op TEXT NOT NULL,               -- gt|lt（offline 忽略）
  threshold REAL NOT NULL, recover_threshold REAL,
  duration_s INTEGER NOT NULL, enabled INTEGER NOT NULL, created_ms INTEGER NOT NULL
);
CREATE TABLE alert_state (rule_id TEXT, server_id TEXT, since_ms INTEGER, value REAL,
  PRIMARY KEY (rule_id, server_id)) WITHOUT ROWID;  -- 只保存 firing 状态
CREATE TABLE alert_events (id INTEGER PRIMARY KEY, rule_id TEXT, server_id TEXT,
  kind TEXT, value REAL, ts_ms INTEGER);              -- kind: firing|resolved
CREATE TABLE meta (k TEXT PRIMARY KEY, v TEXT);       -- schema_version、last_hourly_ms 等
```
Schema 通过 `meta.schema_version` 做顺序迁移，在构造函数中执行（同步 SQL）。索引尽量少，因为每次写入都按「行 + 索引行」计费。

### 5.3 数据路径
- **Metrics 帧** → 更新 attachment.latest → 广播 `metrics` → 评估指标类告警（§6）。**不写 SQLite。**
- **Rollup** → `INSERT OR REPLACE INTO metrics_1m` → 回 `Ack`。如果 `minute_ms` 所在小时早于 `meta.last_hourly_ms`，就向 `dirty_hours` 插入一行。
  - 拒绝时间不合理的数据：比当前时间晚超过 2 分钟，或早于 1m 表的保留期。这类数据直接 Ack，不写入，避免 Agent 一直重发。
- **Alarm**（有 Agent 连接时每 30 秒一次；没有连接时每 5 分钟一次，用于离线告警和维护）：
  1. 半开连接检测：超过 3 个上报间隔没有消息 → 走离线流程（幂等）。
  2. 离线告警评估。
  3. 每小时一次：把最近 2 个完整小时和 `dirty_hours` 中的小时汇总进 `metrics_1h`，然后清空 `dirty_hours`；按服务器分别做范围删除，清理过期数据（1m 默认 7 天，1h 默认 90 天）。
- **历史查询**：`1h`/`6h`/`24h` 直接取 1m 数据；`7d` 从 1m 数据按 10 分钟聚合（SQL `GROUP BY ts_ms/600000`）；`30d`/`90d` 取 1h 数据。
- **本月流量**：当月 `metrics_1h` 的 bytes 总和，加上「最后一个已汇总小时」之后的 `metrics_1m` 数据。按 UTC 月份计算。

### 5.4 额度预算（Free，每台服务器每天）
| 维度 | 用量 |
|---|---|
| DO 请求当量 | (17,280 帧 + 1,440 rollup) / 20 ≈ 936 |
| SQLite 写 | 1,440（1m）+ 24（1h）+ 清理 ≈ 2,900 → **约 30 台** |
| Alarm 调用 | 2,880 次/天（与服务器数量无关） |

## 6. 告警

- 指标类规则在每个 Metrics 帧上评估，取值：cpu=`cpu_pct`，mem/swap/disk=百分比，load1，net_rx/net_tx=bps。
- 状态机（每对规则 × 服务器）：`ok → pending → firing → ok`。
  - pending 的开始时间存在 attachment 的 `pending: {rule_id: since_ms}` 中；持续 `duration_s` 后转为 firing：写 `alert_state` 和 `alert_events`，发通知，广播 `alert`。
  - 恢复条件：值越过 `recover_threshold`（默认：gt 规则为 threshold×0.9，lt 规则为 threshold×1.1）→ 删除 `alert_state`，写 `alert_events`，发通知，广播。
- offline 规则：在 Alarm 中评估，服务器离线时长 ≥ `duration_s` 即触发；Agent 重新连上即恢复。
- 只有状态切换时才写 SQLite。告警规则存在 SQLite 中，每次构造实例时加载到内存缓存，规则变更时让缓存失效（缓存丢了重新加载即可）。
- **通知**：`TG_BOT_TOKEN` + `TG_CHAT_ID` → Telegram `sendMessage`；`WEBHOOK_URL` → POST JSON `{event, rule, server, value, ts_ms}`。超时 5 秒，失败只记日志，不重试。
- 默认规则：首次初始化 schema 时创建：offline 60 秒、cpu>90 持续 300 秒、mem>90 持续 180 秒、disk>90 持续 0 秒。

## 7. 前端（web/）

React 19 + TypeScript + Tailwind v4 + Vite，图表用 uPlot。构建产物放在 `web/dist`，作为 Worker 的 Static Assets，`run_worker_first: ["/api/*", "/ws/*"]`。
页面包括：登录、总览（服务器卡片）、服务器详情（实时数据 + 历史曲线 + 主机信息 + 本月流量）、设置（服务器管理 / 安装命令 / 告警规则 / 告警事件 / 通知测试）。
状态：一条 dashboard WS 连接，断线后指数退避重连；重连时用 snapshot 覆盖本地状态。支持明暗主题，移动端自适应。

## 8. 部署

- `deploy.yml`：依次构建 web → worker-build → `wrangler deploy` → `wrangler secret bulk`（`ADMIN_PASSWORD`、`AUTH_SECRET`，可选 `TG_BOT_TOKEN`、`TG_CHAT_ID`、`WEBHOOK_URL`）。部署时通过 `--var AGENT_RELEASE_REPO:${{ github.repository }}` 注入发布仓库。
- `release-agent.yml`：用 cargo-zigbuild 构建 musl amd64/arm64，连同 `install-agent.sh` 和 `SHA256SUMS` 发布到 Release。
- 安装脚本会校验 SHA256；支持 `--uninstall`。
