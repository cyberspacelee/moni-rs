# v1 实施任务

后端任务（B*、R1）由 Codex（gpt-6.1-sol）完成，前端任务（F*）由 Claude 完成，Claude 同时负责跟踪进度和最终验收。
每完成一项任务，必须通过对应的「完成标准」，然后单独提交（commit 由 Claude 在审阅后执行）。

| ID | 负责 | 任务 | 依赖 | 状态 |
|---|---|---|---|---|
| B1 | Codex | 协议与 DTO | — | ☐ |
| B2 | Codex | Agent v1 | B1 | ☐ |
| B3 | Codex | Worker：鉴权、服务器管理、配置 | B1 | ☐ |
| B4 | Codex | Hub：数据路径、历史、流量、维护 | B3 | ☐ |
| B5 | Codex | 告警与通知 | B4 | ☐ |
| B6 | Codex | CI / 部署 / 发布 / 安装脚本 / e2e | B2, B5, F1 | ☐ |
| F1 | Claude | 前端脚手架、API 客户端、登录 | B1 | ☐ |
| F2 | Claude | 总览 + 实时 WS | F1 | ☐ |
| F3 | Claude | 服务器详情 + 历史图表 | F2, B4 | ☐ |
| F4 | Claude | 设置：服务器管理 / 告警 / 通知 | F2, B5 | ☐ |
| R1 | Codex | 完整 code review | 全部 | ☐ |
| R2 | Claude | 目标对照验收 | R1 | ☐ |

## B1 协议与 DTO（`crates/protocol`）
- 按 `docs/design.md` §2 和 `docs/api.md` 实现全部类型，`PROTOCOL_VERSION = 2`。
- 增加 `ts` feature（依赖 `ts-rs`）。`cargo test -p moni-protocol --features ts` 时把 TS 类型导出到 `web/src/api/generated/`，每个类型一个文件，外加 `index.ts` 统一 re-export。
- 补充单元测试：每种消息都做序列化往返测试；对照 api.md 的示例，校验 tag 和字段名。
- **完成标准**：`cargo test -p moni-protocol`、`cargo test -p moni-protocol --features ts` 通过，生成的 TS 与 api.md 一致。

## B2 Agent v1（`crates/agent`）
- 按 design §3 实现：1 秒采样、实时帧、分钟 Rollup、待确认队列与重发、Welcome/Ack 处理、关闭码处理、`MONI_IFACES`/`MONI_DISKS`。
- 把纯逻辑拆成可测试的模块：速率与差值（计数器重置）、Rollup 聚合、待确认队列（有界、按时间顺序、Ack 后删除）。
- **完成标准**：单元测试覆盖上述纯逻辑；`cargo clippy -p moni-agent -- -D warnings` 通过；连接本地 `wrangler dev` 可以正常上报（在 B4 中联调）。

## B3 Worker：鉴权、服务器管理、配置
- 按 design §4 和 api.md §1–§3（不含 history）实现：登录 / 登出 / me、会话 Cookie、Bearer、登录限流、`/api/config`、服务器 CRUD、token 生成与轮换（HMAC 哈希）、Agent WS 鉴权（Hub 比对哈希，不匹配时关闭码 4003 或返回 401）。
- 统一错误响应格式；缺少 Secret 时返回 `503 not_configured`。
- 静态资源：`assets.directory` 改为 `../../web/dist`（目录不存在时 wrangler 会报错，所以先提交一个占位的 `web/dist/.gitkeep`，或在 wrangler.jsonc 中注明需要先构建前端）。
- **完成标准**：clippy（wasm32）零警告；`scripts/e2e.sh` 的鉴权 / CRUD 部分能在本地跑通。

## B4 Hub：数据路径
- 按 design §5 实现：schema 迁移、Metrics 帧路径、Rollup 写入与 Ack、`dirty_hours`、Alarm（失活检测、每小时汇总、按服务器清理）、history 各个 range、本月流量、snapshot、dashboard 事件（api.md §5，不含 alert）。
- 删除 PoC 时期的 `poc_meta`、`/api/debug`，以及服务端分钟累加逻辑（ADR-0006）。
- **完成标准**：e2e 覆盖以下场景：上报 → 历史有数据；**Agent 运行期间重启 `wrangler dev`，重连后所有分钟数据都齐全，没有缺口**；冻结 Agent → 判为离线。

## B5 告警与通知
- 按 design §6 和 api.md §4 实现：规则 CRUD、默认规则、指标告警状态机（pending 计时放在 attachment）、offline 告警、事件列表、active 列表、Telegram/Webhook 通知、`/api/alerts/test`、WS `alert` 事件、snapshot 中的 `alerts`、`ServerStatus.alerts_firing`。
- **完成标准**：对状态机纯逻辑写单元测试（放在 protocol 之外、不依赖 worker 的模块里，或者用 `#[cfg(test)]` 在 native 上运行）；e2e 覆盖「cpu>阈值规则（threshold 设为 0）→ 触发 firing → 修改规则 → resolved」，以及 offline 告警。

## B6 CI / 部署 / 发布
- 按 design §8 更新 `ci.yml`、`deploy.yml`、`release-agent.yml`、`scripts/install-agent.sh`（SHA256 校验、`--uninstall`），新增 `scripts/e2e.sh`（本地全流程，可重复运行）。
- CI 增加：web 构建（`npm ci && npm run build` 在 `web/` 下）、ts 类型生成后检查 `git diff --exit-code web/src/api/generated`。
- **完成标准**：本地执行 `scripts/e2e.sh` 全部通过；workflow 语法通过 `actionlint` 检查（如果能安装）。

## F1–F4 前端（Claude）
见 design §7；交互契约以 api.md 为准。

## R1 完整 Review（Codex）
- 对照 design.md、api.md、ADR 审查全仓库，重点：正确性、免费额度预算、安全（token、Cookie、header 注入）、DO 状态位置规则、panic、错误处理、测试缺口。
- 输出 `docs/review/codex-review.md`，内容包括：问题清单（严重程度、文件:行号、修复建议）。

## R2 目标对照（Claude）
- 对照最初目标（Cloudflare 免费额度、GitHub Actions 一键部署、Rust 后端、TS + Tailwind 前端）逐条验收，输出 `docs/review/acceptance.md`。
