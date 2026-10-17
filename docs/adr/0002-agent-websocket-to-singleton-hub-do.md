# ADR-0002: Agent 通过 Hibernatable WebSocket 直连单例 Hub DO

- 状态：已接受（本地 PoC 通过，线上待验证，见 docs/poc-report.md）
- 日期：2026-10-17

## 背景
目标是在 Workers Free 额度内运行。每次 HTTPS 推送 = 1 Worker 请求 + 1 DO 请求，10 秒间隔时 10 万/天的额度只够约 11 台。
DO 计费规则（见 docs/research.md §2）：入站 WS 消息 20:1 折算、出站免费、协议 ping 免费且不打断休眠、可休眠空闲不计时长。

## 决策
- Agent 与浏览器都通过 `wss://<host>/ws/agent`、`/ws/dashboard` 升级，由 Worker 转发至**单例** `Hub` DO（`get_by_name("hub")`）。
- DO 使用 Hibernation API（`accept_websocket_with_tags`，tag 为 `agent` / `dash` / `server:<id>`）。
- 连接身份、最新帧、当前分钟聚合全部存入 `serialize_attachment`（≤16 KiB），休眠唤醒后恢复。PoC 证实 DO 会在两帧之间被回收，**不得依赖实例字段保存状态**。
- Agent 保活使用 WS 协议 ping 帧（免费、不唤醒 DO）。
- Agent 默认每 5 秒发一帧指标；DO 收到后更新 attachment 并向 `dash` 标签连接广播（出站免费）。

## 后果
- 每台 5s 间隔：17,280 条/天 → 864 次请求当量；DO 请求额度可支撑 100+ 台。
- DO 部署时重启并断开所有连接 → Agent 需指数退避重连；Dashboard 需重连并通过 REST 拉快照。
- 单例 DO 是单点：个人规模（≤100 台）吞吐不是问题；位置由首次请求决定，可用 location hint。
- 网络断开检测：`websocket_close` 即时标记离线，Alarm 兜底半开连接（超过 N 秒无消息）。

## 备选方案
- HTTPS 批量推送（原报告方案）：实现简单、无长连接，但额度消耗 ~20 倍，实时性差。
- 每台服务器一个 DO：隔离更好，但概览需跨 DO 聚合，免费额度下无收益。
