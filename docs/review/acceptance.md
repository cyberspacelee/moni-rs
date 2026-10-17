# v1 目标对照验收（R2）

验收人：Claude（编排 / 前端）。日期：2026-10-17。依据：用户最初目标、`docs/design.md`、`docs/api.md`、Codex 完整审查 `docs/review/codex-review.md`（R1）。

## 1. 结论

**本地范围内达成目标，可以进入线上验证。** 后端（Rust，Codex 开发）、前端（TS + Tailwind，Claude 开发）、文档、CI/CD 均已完成。R1 审查的 12 个问题全部修复并有回归测试。浏览器验收中额外发现了一个回归（无请求体的写操作返回 415），已修复，并新增浏览器冒烟测试防止复发。

**尚未验证的部分**（需要用户提供 Cloudflare 账号和 GitHub 仓库）：线上部署、真实免费额度消耗、GitHub Actions 实际运行、真实 VPS 上的 Agent 和 Telegram 通知。在完成 §5 的线上验收之前，「免费额度内运行」和「一键部署」仍是**基于代码推导和本地验证的结论**，不是实测结论。

## 2. 目标逐条对照

| 目标 | 状态 | 证据 / 说明 |
|---|---|---|
| 部署在 Cloudflare，Rust 后端 | ✅ 本地 | Worker + 单例 Hub DO（workers-rs 0.8.7，Rust → wasm，约 590 KiB 量级）；`wrangler deploy --dry-run` 通过 |
| 监控 VPS | ✅ 本地 | Agent：1 秒采样、5 秒实时帧、分钟 Rollup + ACK 重发；静态 musl 二进制（amd64 / arm64），RSS 约 4 MB |
| 使用免费额度 | ✅ 推导 / ⏳ 线上 | 只用 Workers + DO（SQLite）+ Static Assets，**不需要预先创建任何资源**。写入 `2,952 × N + 2,904` 行/天（30 台约 91.5k，低于 100k 上限）；每帧读取上界为 `F+1` 行（F 为当前服务器 firing 告警数）；DO 请求约 936/台/天 + 2,880（Alarm） |
| GitHub Actions 一键部署 | ✅ 编写并静态检查 / ⏳ 线上 | `deploy.yml`：Secret 守卫 → 构建 web → wrangler deploy（注入 AGENT_RELEASE_REPO）→ secret bulk；`release-agent.yml`：zigbuild 双架构 + SHA256SUMS；actionlint、ShellCheck 通过 |
| 前端：现代 TS、Tailwind、样式简洁 | ✅ | React 19、TS 7（strict + noUncheckedIndexedAccess）、Vite 8、Tailwind v4、uPlot；明暗主题、移动端适配；图表配色经 dataviz 校验器在两种表面色上验证通过 |
| 方案文档：ADR / architecture / design | ✅ | `docs/adr/0001–0007`、`architecture.md`、`design.md`、`api.md`、`research.md`、`poc-report.md` |
| 任务拆分 + 进度跟踪 | ✅ | `docs/tasks.md`（B1–B6、F1–F4、R1–R2） |
| git 仓库、env 示例、README | ✅ | `.env.example`、`crates/worker/.dev.vars.example`、`scripts/agent.env.example`、README（部署 / 本地开发 / 运维） |
| 后端全部交给 Codex（gpt-6.1-sol） | ✅ | B1–B6、FIX1–FIX3 全部由 `codex exec -m gpt-6.1-sol` 完成。Claude 没有编写后端代码，只做审阅、联调和提交 |
| Codex 完整 review | ✅ | R1：12 个问题（0 Critical / 4 High / 5 Medium / 3 Low），全部已修复 |

## 3. 审查问题处理状态

| ID | 级别 | 问题 | 修复 | 负责方 |
|---|---|---|---|---|
| R01 | High | 每帧扫描历史告警，读额度会被打爆 | 独立待发送队列（发送后删除）；alert_state 主键改为 `(server_id, rule_id)`；事件保留 1000 条 / 90 天；读行数回归测试 | Codex FIX1 |
| R02 | High | Cookie 写操作与 WS 缺少 Origin/CSRF 校验 | Origin 必须与目标源一致；有请求体时要求 JSON；Bearer 鉴权豁免 | Codex FIX1 |
| R03 | High | Alarm 两次调度失败后永久停止 | 返回 Err 让平台重试，并在消息路径加入限频自愈检查 | Codex FIX1 |
| R04 | High | 流量值过大导致 SUM 溢出，快照返回 500 | 入口范围校验；SQL 改用 `total()`；按服务器隔离降级 | Codex FIX1 |
| R05 | Medium | 持久化失败后分钟数据永不补发 | 用关闭码 1011 触发重连补发；Agent 端加 90 秒 ACK 超时重发 | Codex FIX1 |
| R06 | Medium | 看板上的本月流量不刷新 | 新增 `traffic` 事件（attachment 增量维护，不产生额外读取）；前端处理该事件 | Codex FIX1 + Claude |
| R07 | Medium | PATCH DTO 序列化出 null | `skip_serializing_if` 跳过外层 None；补 DTO → handler 测试 | Codex FIX1 |
| R08 | Medium | 历史查询失败时仍显示旧查询的数据 | 历史数据与 (id, range) 绑定，错误提示中标注 range | Claude |
| R09 | Medium | 删除服务器后告警事件仍保留 | 一并删除事件、状态和待发送通知，不发 resolved 通知 | Codex FIX1 |
| R10 | Low | 非法 hash 编码导致页面白屏 | 解码失败时回退到总览页 | Claude |
| R11 | Low | 对话框缺少可访问名称 | `aria-labelledby` + `useId` | Claude |
| R12 | Low | dash-watch 脚本已过时 | 改为 Bearer 环境变量 + `ev.id` | Codex FIX1 |

**R1 之外，Claude 在验收中发现并处理的问题：**

| 问题 | 发现方式 | 处理 |
|---|---|---|
| Cookie 会话下**无请求体**的写操作（删除服务器、轮换 token、登出、测试通知、删除规则）全部返回 415，前端这些功能不可用 | 浏览器实测 | Codex FIX2：按 `Content-Length` / `Transfer-Encoding` 判断是否有请求体，并补 e2e |
| 上述问题能逃过 e2e，是因为 e2e 只用 Node 客户端、没有覆盖浏览器真实发出的请求 | 复盘 | 新增 `web/tests/smoke.mjs`，在浏览器中跑完 UI 的全部写操作；FIX3 接入 CI（`e2e.sh --quick --ui`）。Codex 在 FIX3 中反过来指出冒烟测试的通知断言选择器无效、超时被吞，已修正 |
| 路由快照不稳定，导致 React #185 无限渲染（首页白屏） | 浏览器实测 | 修复（`router.ts`） |
| 新的 Origin 规则会让 Vite 开发代理失效 | 代码推演 | 代理改写 Origin（仅开发环境） |
| 会话过期后，WS 无限重连而不回到登录页 | 代码审查（R1 测试缺口 8） | 握手失败时探测 `/api/auth/me`，返回 401 则回到登录页 |
| Hub 静默丢弃无法解析的 Agent 消息 | 联调 | 改为打印限频且脱敏的警告日志（B4） |
| 文档中的过时数字（ADR-0006 的请求增量、design §5.4 的写入构成）、architecture 数据流 | 对照审查 | 已更新 |

## 4. 验证证据（本地）

- `cargo fmt --check`；native + wasm32 `clippy -D warnings`：通过。
- 单元测试：protocol 12、agent 20、worker（native）14，合计 46，全部通过。
- `cargo test -p moni-protocol --features ts` 重新生成后，与仓库中的 TS 类型无差异。
- `scripts/e2e.sh` 完整版：约 260 秒通过，覆盖鉴权 / CSRF、CRUD、token 吊销、Rollup 连续性、**运行中重启 wrangler 后的补发**、冻结判离线、小时汇总、清理、告警、通知、读行数回归。`--quick` 并发连续运行 3 次通过。
- `web/tests/smoke.mjs`：9 个步骤全部通过（登录、创建、改名、轮换 token、详情页、规则增 / 停 / 删、测试通知、删除、登出）；`scripts/e2e.sh --quick --ui` 一并通过（34 秒），CI 中执行同一命令。
- 截图人工检查：总览、详情（24 小时历史曲线、断点显示）、设置各页签、token 弹窗、规则编辑；明暗主题；1366 和 390 两种宽度。
- `traffic` 事件累加得到的值与 SQL SUM 结果逐字节一致。

## 5. 与目标的差距和遗留风险

1. **未做线上验证**（最重要）。需要用户提供：Cloudflare Account ID 和 API Token（Edit Cloudflare Workers），以及 GitHub 仓库或 fork。上线后要核对：首次部署零配置成功；Agent 连续运行 24 小时后，DO 请求数、SQLite 读写行数、GB-s 与 §2 的推导一致；部署新版本时 Agent 能自动重连补发；HK / SG / US 节点的连接稳定性。
2. **容量上限约 30 台**（Free 计划）。超出后需要把历史粒度改为 5 分钟，或改用 Workers Paid。这是设计取舍，README 已写明。
3. Agent 的待确认队列只在内存中：Agent 进程崩溃时，最多丢失尚未 ACK 的分钟数据（v1 已声明的限制）。
4. Telegram 通知没有用真实凭据测试过（Webhook 已在本地验证）。
5. 前端只有一个浏览器冒烟测试，没有组件 / 单元测试；可访问性只做了语义修复，没有经过屏幕阅读器验收。
6. `*.workers.dev` 在大陆无法直接访问，需要绑定自定义域名（README 已说明）。

## 6. 建议的下一步

1. 用户提供凭据 → 跑通 `deploy.yml` → 打 tag 发布 Agent → 在 1～2 台 VPS 上安装，观察 24 小时。
2. 根据线上实测数据修订 design §5.4 的额度预算，然后打 v0.1.0 正式标签。
3. v1.1 候选：Agent 磁盘持久化队列、公开状态页、Cloudflare Access 集成文档、5 分钟粒度的「大规模模式」。
