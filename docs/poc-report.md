# PoC 报告（阶段一：本地）

日期：2026-10-17。环境：`wrangler dev` 4.151.1（本地 workerd）、`worker` 0.8.7、rustc 1.94.1。
**尚未在 Cloudflare 线上验证**，线上清单见 §4。

## 1. 结论

| 问题 | 结果 |
|---|---|
| workers-rs 能否实现 Hibernatable WS DO | ✅ `accept_websocket_with_tags`、`websocket_message/close/error`、attachment 均可用 |
| 休眠后连接与身份能否恢复 | ✅ 14s 间隔下 DO **每帧之间都被回收重建**（`constructs_total` 持续递增），WS 不断，attachment 恢复 server_id |
| 内存状态能否依赖 | ❌ 不能。已改为把 latest 帧和分钟聚合存入 attachment（上限 16 KiB，单帧 JSON 约 400 B） |
| SQLite（DO 内置） | ✅ 建表、UPSERT、WITHOUT ROWID、范围删除均正常 |
| Alarm 离线检测 | ✅ SIGSTOP 冻结 Agent（半开连接）后约 20s 判定离线；恢复后积压帧被丢弃并自动重连 |
| 连接替换 | ✅ 同 server_id 新连接替换旧连接（关闭码 4002） |
| 鉴权 | ✅ 错误 token → 401，Agent 指数退避 |
| Worker 包体积 | 589.72 KiB（gzip 229 KiB），远低于 64 MiB |
| Agent 资源 | release：RSS 3.8 MB，CPU ≈ 0% |
| Agent 静态编译 | ✅ cargo-zigbuild：x86_64 musl 3.3 MB、aarch64 musl 2.8 MB；musl 版 rustls wss 握手正常 |

## 2. 发现的坑（已修复 / 已记录）

1. **`Storage::set_alarm(i64)` 在 workers-rs 中是“相对偏移毫秒”**，而 JS API 的数字是绝对时间戳。误传 `now + 30000` 会把 alarm 设到约 2083 年。统一使用 `Duration`。
2. **`[profile.release] strip = true` 会破坏 wasm-bindgen**（报错 `externref table required for catch wrappers`）。只对 agent 开启 strip。
3. **wrangler 自定义 build 命令的工作目录**取决于 wrangler 的启动目录，必须在 `crates/worker` 内运行 wrangler（`npm run dev` 已处理）。
4. **sysinfo 0.39 需要 rustc 1.95**，暂时锁定 0.38。
5. **服务端 close 之后、对端确认之前，socket 仍在 `get_websockets()` 中**，并且可能继续投递消息 → attachment 中加 `closing` 标记，保证离线广播的幂等。
6. Agent 被挂起后 tokio interval 默认会补发错过的 tick → 设置为 `MissedTickBehavior::Skip`。

## 3. 架构修订

> 后续调研（ADR-0006）：attachment 只能扛住**休眠**。DO **关停**（部署、运行时更新、迁移、panic）时 WS 会被终止，attachment 随之丢失。建议把分钟聚合移到 Agent 端，DO 只做幂等写入。

- ADR-0002 中「DO 内存保存最新指标」改为：**连接级状态全部放在 attachment**，DO 字段只保存调试计数。
- 时长计费：5 秒一帧时，DO 永远等不到 10 秒的空闲期，**实际上不会休眠**。但官方定义「空闲且可休眠」的状态不计时长，所以计费仍然只覆盖每帧的处理时间。这一点需要线上确认。PoC 用 14 秒间隔才触发了休眠。

## 4. 线上验证清单（需要 Cloudflare 账号）

- [ ] GitHub Actions `deploy.yml` 首次部署成功（零预置资源）
- [ ] wss 经 Cloudflare 边缘连接，Agent 跑满 24h，在 Dashboard 中核对：
  - Workers 请求数（预期：只有连接/重连与 API 请求）
  - DO 请求数（预期：约每台 864/天）
  - DO 时长 GB-s
  - SQLite 写入行数（预期：约每台 1,440/天，外加清理）
- [ ] 线上的休眠行为与本地一致（`/api/debug` 中 `constructs_total` 的变化）
- [ ] 重新部署时所有 WS 断开，Agent 在 60s 内重连
- [ ] 来自 HK/SG/US VPS 的连接稳定性
