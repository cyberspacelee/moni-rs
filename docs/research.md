# 调研记录：Cloudflare 平台事实核实

核实日期：2026-10-17。来源均为 Cloudflare 官方文档（`developers.cloudflare.com/**/index.md`）与 crates.io 上 `worker 0.8.7` 源码。
原始调研报告中的结论若与下表冲突，以本表为准。

## 1. 免费额度与限制（Workers Free）

| 项目 | 免费额度 | 来源 |
|---|---|---|
| Worker 请求 | 100,000 次/天（UTC 0 点重置，超出返回 1027） | workers/platform/limits |
| CPU 时间 | 10 ms / 请求，10 ms / Cron | workers/platform/limits |
| 内存 | 128 MB / isolate | workers/platform/limits |
| 子请求 | 50 / 请求 | workers/platform/limits |
| Worker 体积 | **64 MiB 未压缩，无压缩体积限制**（旧的 3MB/1MB 说法已过时） | workers/platform/limits |
| 启动时间 | 1 s | workers/platform/limits |
| Cron Trigger | 每账号 5 个 | workers/platform/limits |
| 静态资源请求 | **免费且不限量**，不计入 Worker 请求（除非 `run_worker_first`） | workers/static-assets/billing-and-limitations |
| DO 请求 | 100,000 次/天；含 HTTP、RPC、WS 消息、alarm 调用 | durable-objects/platform/pricing |
| DO 时长 | 13,000 GB-s/天 | 同上 |
| DO SQLite 行读 / 行写 / 存储 | 5M/天 / 100k/天 / 5 GB | 同上 |
| Analytics Engine 写入 / 查询 | 100k 点/天 / 10k 次/天（**目前尚未开始计费**） | analytics-engine/pricing |
| AE 保留期 | 3 个月 | analytics-engine/limits |
| AE 单点上限 | 20 blobs + 20 doubles + 1 index（≤96B）；blobs ≤16KB；每次调用 ≤250 点 | analytics-engine/limits |

## 2. Durable Object WebSocket 计费关键点（决定架构）

摘自 DO pricing / websockets 最佳实践：

- **入站 WS 消息按 20:1 折算为请求**；**出站消息不收费**。
- **入站协议级 ping 帧不收费**，运行时自动回 pong，且**不打断休眠**。
- 建立一个 WS 连接计 1 次请求。
- 使用 Hibernation API 时，空闲且可休眠的 DO **不计时长**（即使运行时尚未真正休眠）。
- `setWebSocketAutoResponse()` 的自动回复不计时长。
- 存在挂起 I/O 时 DO 可能在内存中停留最多 15 分钟并计费 → 处理函数里不要留悬挂的 fetch/定时器。
- **代码部署会重启所有 DO 并断开所有 WebSocket**，客户端必须自动重连。
- 每个连接可用 `serializeAttachment` 保存少量状态，休眠唤醒后用 `deserializeAttachment` 恢复。

## 3. workers-rs（`worker` crate 0.8.7，2026-09-25）能力核实

源码 `src/durable.rs`、`src/websocket.rs`、`src/sql.rs` 中确认存在：

| 能力 | API |
|---|---|
| DO 定义 | `#[durable_object] struct X` + `impl DurableObject for X` |
| Hibernation 接受连接 | `State::accept_web_socket(&ws)` / `accept_websocket_with_tags` |
| 枚举连接 | `State::get_websockets()` / `get_websockets_with_tag(tag)` |
| 自动回复 | `State::set_websocket_auto_response(&pair)` |
| 休眠回调 | `websocket_message` / `websocket_close` / `websocket_error` |
| 连接附件 | `WebSocket::serialize_attachment` / `deserialize_attachment` |
| Alarm | `Storage::set_alarm` / `get_alarm` / `delete_alarm` + `alarm()` 回调 |
| SQLite | `Storage::sql().exec(query, bindings)` → `SqlCursor::to_array/one/rows_written` |
| Analytics Engine 写 | `AnalyticsEngineDataset` + `AnalyticsEngineDataPointBuilder` |

注意：
- `DurableObject` 方法签名均为 `&self`，可变状态需要 `RefCell`/`Cell`。构造函数是同步的，异步初始化需要 `block_concurrency_while`。
- **`Storage::set_alarm(i64)` 把参数当作相对当前时间的偏移毫秒**（`From<i64> for ScheduledTime` 为 Offset），与 JS API 的绝对时间戳不同。
- `serializeAttachment` 的序列化结果上限为 16,384 字节；任一方关闭连接后 attachment 即丢失。

## 4. 历史数据查询

- AE 专用 SQL API：`POST /client/v4/accounts/{id}/analytics_engine/sql`，需 Bearer Token（Account Analytics: Read）。
- 新的通用 Analytics SQL API 有 **Workers binding**（wrangler.jsonc 中 `"analytics": {"binding": "ANALYTICS_SQL"}`，需 Wrangler ≥ 4.145.0），无需管理 Token；数据集名 `events.analyticsEngine.<DATASET>`。
  workers-rs 暂无该 binding 的类型封装，需通过 `env.get_binding`/`js_sys` 自行包装 —— **待 PoC 验证**。
- AE 会在高流量 index 上采样，查询需考虑 `_sample_interval`。

## 5. CI/CD

- 官方 GitHub Action：`cloudflare/wrangler-action@v4`，需 Secrets `CLOUDFLARE_API_TOKEN`（模板 "Edit Cloudflare Workers"）与 `CLOUDFLARE_ACCOUNT_ID`。
- 当前 Wrangler：4.151.1。

## 6. 已纠正的原报告结论

| 原结论 | 修正 |
|---|---|
| Agent HTTPS 推送为默认 | 免费额度下应 Agent → DO 走 Hibernatable WS（见 ADR-0002） |
| D1 存业务数据 | DO 内置 SQLite 足够，且不需预建资源，利于一键部署（ADR-0003） |
| R2 归档 | MVP 去除 |
| 需要注意包体积限制 | 体积限制已是 64MiB 未压缩，风险降低，但仍关注启动时间 |
