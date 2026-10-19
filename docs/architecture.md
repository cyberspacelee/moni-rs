# moni-rs 架构

> 状态：v1（2026-10-17）。PoC 结论见 `docs/poc-report.md`，详细设计见 `docs/design.md`。决策依据见 `docs/adr/`，平台事实见 `docs/research.md`。

## 1. 目标与约束

- 监控个人 VPS（目标 ≤30 台，Free 计划内；≤100 台需调整历史粒度或升级 Paid）。
- **完全运行在 Cloudflare 免费额度内**，无自建中心服务器。
- **fork + 填 Secrets + 跑 workflow** 即可部署。
- 后端与 Agent 均为 Rust；前端 React + TS（shadcn/ui + Base UI）。

## 2. 组件总览

```
             ┌──────────────────────── Cloudflare ────────────────────────┐
             │                                                             │
 Browser ──HTTPS(静态资源，免费)──▶ Static Assets (React Dashboard)         │
    │        │                                                             │
    ├─wss /ws/dashboard──┐                                                 │
    └─https /api/*───────┤                                                 │
                         ▼                                                 │
                ┌─────────────────┐   stub.fetch    ┌──────────────────┐   │
 Agent ─wss /ws/agent──▶│ Worker (Rust/WASM)│───────────────▶│ Hub DO (单例)     │   │
 (每 5s 一帧,          │ 路由 / 鉴权 / API │                │ - Hibernatable WS │   │
  ping 帧保活)          └─────────────────┘                │ - attachment:最新帧 │   │
                                                           │ - SQLite:配置/历史 │   │
                                                           │ - Alarm:离线/清理   │   │
                                                           └───────┬──────────┘   │
                                                                   │(可选)        │
                                                                   ▼              │
                                                         Analytics Engine         │
             └─────────────────────────────────────────────────────┼──────────────┘
                                                                   ▼
                                                        Telegram / Webhook 通知
```

## 3. 关键数据流（v1）

### 3.1 Agent 上报
1. Agent 启动 → `wss://host/ws/agent`，携带 `Authorization: Bearer mn_<server_id>_<secret>`。
2. Worker 先剥离客户端带来的内部 header，计算 `HMAC(AUTH_SECRET, token)`，再带上 `x-moni-server-id` 和 `x-moni-token-hash` 转发给 Hub DO（ADR-0005）。
3. Hub 比对 token 哈希 → `accept_websocket_with_tags(ws, ["agent", "server:<id>"])` → 写入 attachment → 发送 `Welcome{report_interval_s}` → 广播 `online`。
4. Agent 每 5 秒发一个 `Metrics` 帧：Hub 更新 attachment.latest → 广播 `metrics`（出站免费）→ 评估指标类告警。**不写 SQLite。**
5. 每分钟，Agent 发送一条在本地聚合好的 `Rollup`（基于 1 秒采样）。Hub 执行 `INSERT OR REPLACE INTO metrics_1m`，然后回 `Ack`。未确认的 Rollup 留在 Agent 队列中，重连后重发（ADR-0006）。
6. Agent 每 30 秒发一次协议 Ping 帧（免费，不唤醒 DO）。

### 3.2 离线检测与维护
- `websocket_close` / `websocket_error` → 立即标记离线并广播。
- Alarm：有 Agent 连接时每 30 秒一次，没有连接时每 5 分钟一次。检查半开连接（超过 3 个上报间隔没有消息）、评估 offline 告警；每小时把数据汇总到 `metrics_1h`，并清理过期数据。
- 服务端 close 后，对端可能长时间不确认，socket 仍会被枚举到；attachment 中的 `closing` 标记保证离线处理是幂等的。
- DO 被回收后，实例字段会丢失：连接级状态从 attachment 恢复，持久状态从 SQLite 读取。DO 关停（部署、运行时更新）会断开所有 WS：Agent 自动重连并重发 Rollup，所以历史数据不会丢。

### 3.3 Dashboard
- 浏览器访问静态资源（免费）→ `POST /api/auth/login` 获得签名 Cookie → `wss /ws/dashboard`：先收到 `snapshot`，之后接收增量事件（`metrics` / `online` / `offline` / `server_upsert` / `server_removed` / `alert`）。
- 历史：`GET /api/servers/:id/history?range=1h|6h|24h|7d|30d|90d`，24 小时以内的范围读 `metrics_1m`，7d 从 1m 数据按 10 分钟聚合，30d/90d 读 `metrics_1h`。

## 4. 部署单元

| 单元 | 产物 | 位置 |
|---|---|---|
| Worker + Hub DO | `crates/worker/build/index.js` + wasm（worker-build） | Cloudflare（`deploy.yml`） |
| Dashboard | `web/dist` → Static Assets | Cloudflare（同一 Worker） |
| Agent | `moni-agent-linux-{amd64,arm64}` musl 静态二进制 | GitHub Release（`release-agent.yml`）→ `install-agent.sh` → systemd |

## 5. 额度预算（Free，5 秒间隔，每台服务器每天）

| 维度 | 用量 | 免费额度 | 上限台数 |
|---|---|---|---|
| DO 请求（WS 入站 /20） | (17,280 帧 + 1,440 rollup) / 20 ≈ 936 | 100k | ~100（另有 Alarm 2,880/天，与台数无关） |
| Worker 请求 | 重连 + 看板 API，约数十次 | 100k | 不构成瓶颈 |
| SQLite 写 | ≈ 2,950（B4 根据代码推导） | 100k | **~33** |
| DO 时长 | 每帧毫秒级；空闲可休眠的时间不计费 | 13k GB-s | 不构成瓶颈（需线上实测） |

## 6. 开放问题

1. ~~Hibernation 回调与 attachment 恢复~~：本地已验证 ✅
2. 线上 Rust WASM DO 唤醒的 CPU 耗时、每帧时长 → 修正额度预算（需线上）
3. ~~`wrangler dev` 对 hibernation / SQLite / alarm 的模拟~~：三者均可在本地复现 ✅
4. 单例 DO 的位置与跨洲 Agent 的延迟（需线上）
