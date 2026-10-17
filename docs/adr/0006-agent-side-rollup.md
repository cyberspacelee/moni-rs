# ADR-0006: 分钟聚合移到 Agent 端，DO 只做无状态写入

- 状态：已接受（v1 实现）
- 日期：2026-10-17
- 关联：ADR-0002、ADR-0003、docs/poc-report.md

## 背景

PoC 发现 Hub DO 的实例内存会在两帧之间丢失。官方文档（DO lifecycle）说明了两类情况：

| 场景 | 触发条件 | 实例内存 | WS 连接 / attachment |
|---|---|---|---|
| 休眠 | 无事件 10 秒，并满足可休眠条件 | 丢失，下次事件时重新执行构造函数 | **保留** |
| 关停 / 重启 | 部署新代码、运行时更新、迁移宿主、未捕获异常 | 丢失 | **WS 被终止，attachment 丢失** |

当前修复（latest 和分钟累加器存入 attachment）只覆盖休眠，没有覆盖关停。每次部署或运行时更新，最多会丢失每台服务器正在累积的 1 分钟数据。

其他候选方案：

- 每帧写 SQLite / 同步 KV：两者都按 SQLite 行写入计费。每台每天 17,280 行，免费额度只够约 5 台。否决。
- 内存缓冲，在休眠前 flush：运行时没有「休眠前」回调，关停也不可预知。否决。

## 决策

1. **Agent 本地做分钟聚合**。Agent 按 1 秒采样，比服务端按 5 秒帧聚合更准，max 值不会漏掉尖峰。每到分钟边界，发送一条 `AgentMessage::Rollup { minute, ... }`。
2. Hub 收到 Rollup 后直接执行 `INSERT OR REPLACE INTO metrics_1m`。主键 `(server_id, ts)` 天然幂等，DO 不再保存任何聚合状态。
3. Hub 回复 `HubMessage::Ack { minute }`。出站消息免费。Agent 在本地保留未确认的 Rollup（有界队列，默认 60 条，超出丢弃最旧的），重连后重发。
4. attachment 继续保存：身份、`last_seen_ms`、`closing` 标记、latest 帧。这些都只用于展示和离线判定，丢了也会在下一帧补回。
5. Alarm 不再负责 flush，只做离线判定和清理过期数据。

## 后果

- DO 收到的任何消息都不依赖实例状态，休眠和关停都不会丢历史数据。只有 Agent 进程自身崩溃时，才会丢失未确认的数据。
- SQLite 写入量不变（每台每分钟 1 行），WS 入站消息每分钟多 1 条，按 20:1 折算约为每台每天多 72 次请求当量（R1 审查更正）。
- 协议需要加 `Rollup` / `Ack`，并在 `HubMessage` 中加入服务端到 Agent 的消息类型。
- Rust DO 中的 panic 会导致实例终止（等同关停）。Hub 代码必须禁止 `unwrap`/`expect`，统一返回 `Result`。

## 备选方案

- 保持现状（attachment 方案）：实现已完成。可接受每次部署丢失 ≤1 分钟历史时可以保留。
