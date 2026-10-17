# ADR-0003: 存储以 DO 内置 SQLite 为主，Analytics Engine 可选，不使用 D1/R2

- 状态：已接受（本地 PoC 通过）
- 日期：2026-10-17

## 背景
Free 计划只能使用 SQLite 型 DO。D1 需要预先创建并把 `database_id` 写入配置，不利于 fork 后一键部署；R2 需绑定支付方式。
AE 查询需要 API Token 或新的 Analytics SQL binding（workers-rs 无封装）且存在采样。

## 决策
- Hub DO 的 SQLite 存：`servers`、`agent_tokens`（HMAC 哈希）、`alert_rules`、`alert_state`、`metrics_1m`（每台每分钟一行汇总）。
- 最新指标与分钟累加器保存在连接 attachment 中（不计 SQLite 写入，可跨休眠存活），不落盘。
- `metrics_1m` 默认保留 7 天（可配），由 Alarm 定期删除过期数据；超过 7 天可降采样到 `metrics_1h`。
- AE 作为可选的 90 天历史后端，通过 feature flag/配置开启；不是 MVP 必需。

## 后果
- SQLite 行写入是免费额度下的真实瓶颈：每台每天约 1,440 写 + 1,440 删 ≈ 2,880 行 → 约 30 台。超过时调整为 5 分钟粒度或改写 AE。
- 索引更新也计入写入行数，表索引需最少化（建议用 `(server_id, ts)` 作为主键的 WITHOUT ROWID 表）。
- 零预置资源：wrangler 配置中只需 DO 绑定 + `new_sqlite_classes` 迁移。

## 备选方案
- D1 + AE（原报告）：职责清晰但组件多、配置门槛高。
