# moni-rs 架构

> 状态：本地 PoC 后修订（2026-10-17），见 `docs/poc-report.md`。决策依据见 `docs/adr/`，平台事实见 `docs/research.md`。

## 1. 目标与约束

- 监控个人 VPS（目标 ≤30 台，Free 计划内；≤100 台需调整历史粒度或升级 Paid）。
- **完全运行在 Cloudflare 免费额度内**，无自建中心服务器。
- **fork + 填 Secrets + 跑 workflow** 即可部署。
- 后端与 Agent 均为 Rust；前端 React + TS。

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

## 3. 关键数据流

### 3.1 Agent 上报
1. Agent 启动 → `wss://host/ws/agent`，Header `Authorization: Bearer <token>`。
2. Worker 校验 token → 带上 `X-Moni-Server-Id` 转发至 Hub DO。
3. DO `accept_websocket_with_tags(ws, ["agent", "server:<id>"])`，`serialize_attachment({server_id, connected_at})`，广播 `server.online`。
4. Agent 每 5 秒发 `AgentMessage::Metrics`；DO `websocket_message`：
   - 更新该连接 attachment 中的 `latest` 与分钟累加器（DO 可能在任意两帧间被回收）；
   - 向所有 `dash` 连接广播 `metrics.update`（出站免费）；
   - 分钟边界写一行 `metrics_1m`；
   - 评估告警规则（状态变化时才写 SQLite 与发通知）。
5. Agent 每 30 秒发协议 ping 帧（免费，不唤醒 DO）。

### 3.2 离线检测
- `websocket_close` / `websocket_error` → 立即标记 offline、广播。
- Alarm 每 30 秒（仅在有 agent 连接时设置）检查 `last_seen`，超过 3 × 上报间隔视为离线（覆盖半开连接）。
- 服务端 close 后对端可能长时间不确认，socket 仍会被枚举到 → attachment 中的 `closing` 标记保证离线处理幂等。
- DO 被回收后实例字段丢失；所有连接级状态都从 attachment 恢复，服务器列表来自 SQLite。

### 3.3 Dashboard
- 页面 → `GET /api/servers`（快照）→ 打开 `wss /ws/dashboard` 订阅增量。
- 历史：`GET /api/servers/:id/history?range=24h` → DO 查询 `metrics_1m`。

## 4. 部署单元

| 单元 | 产物 | 位置 |
|---|---|---|
| Worker + Hub DO | `build/worker/shim.mjs` + wasm | Cloudflare |
| Dashboard | `web/dist` → Static Assets | Cloudflare |
| Agent | `moni-agent` musl 静态二进制 | GitHub Release → VPS systemd |

## 5. 额度预算（Free，5 秒间隔）

| 维度 | 每台/天 | 免费额度 | 上限台数 |
|---|---|---|---|
| DO 请求（WS 消息 /20） | 864 | 100k | ~115 |
| Worker 请求 | 重连 + API，≈ 数十 | 100k | 不构成瓶颈 |
| SQLite 写（1m 汇总+清理） | ~2,880 | 100k | **~30** |
| DO 时长 | 每帧 ms 级 | 13k GB-s | 不构成瓶颈（需 PoC 实测） |

## 6. 开放问题

1. ~~Hibernation 回调与 attachment 恢复~~：本地已验证 ✅
2. 线上 Rust WASM DO 唤醒的 CPU 耗时、每帧时长 → 修正额度预算（需线上）
3. ~~`wrangler dev` 对 hibernation / SQLite / alarm 的模拟~~：三者均可在本地复现 ✅
4. 单例 DO 的位置与跨洲 Agent 的延迟（需线上）
