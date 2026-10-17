# moni-rs 独立代码审查

审查日期：2026-10-17。基准：`docs/api.md` 优先于实现；预期负载为单管理员、30 台服务器、5 秒实时帧、每分钟 Rollup、默认 7/90 天保留期。只新增本报告，未修改源代码、生成的 TS 或配置，未提交、推送、部署。

## 1. 总结

系统的主体设计已经实现：Rust 协议共享、Agent 分钟聚合、可休眠 WebSocket、SQLite 历史、告警和静态前端均已连通。正常路径的测试结果良好，完整 e2e 也验证了重启补发、小时汇总、过期清理和半开连接恢复。

**不建议按当前状态正式发布 v1。** 最明显的目标差距是：少量历史告警就能让每帧查询耗尽免费读额度；同站点不同源的页面可利用 Cookie 读取看板、执行管理操作；调度失败可能永久停止 Alarm；单台 Agent 的异常流量值可破坏整个看板的快照查询。此外，持久化失败而连接仍存活时，Agent 不会补发缺失的分钟。

共 **12 项：Critical 0、High 4、Medium 5、Low 3**。建议先解决 R01–R05，再处理显示错误、契约偏差和辅助工具问题。

### 范围与验证

完整阅读了 `AGENTS.md`、`docs/design.md`、`docs/api.md`、`docs/tasks.md`、`docs/adr/*`、`docs/research.md`、`docs/poc-report.md`，并审查三个 crate 的全部源文件和测试、`web/src`（含生成类型）、`scripts/*`、三个 workflow、README 和构建配置。仓库实际的 Wrangler 配置为 `crates/worker/wrangler.jsonc`，根目录没有同名文件。

本地验证通过：

- `cargo test -p moni-protocol -p moni-agent -p moni-worker`：40 项测试通过。
- `cargo clippy -p moni-protocol -p moni-agent -- -D warnings`、Worker wasm32 clippy、`cargo fmt --all --check`。
- `cargo test -p moni-protocol --features ts -- --skip export_typescript`：12 项通过；特意跳过会写生成源文件的导出测试。
- `web/` 下 `npm run build`，包含 TypeScript 检查。
- `sh -n scripts/install-agent.sh`、`python3 scripts/test-install-agent.py`。
- **完整** `scripts/e2e.sh`：273.1 秒通过，未使用 `--quick`。

额外用现有构建产物和本地 Miniflare/workerd 做了隔离实验：外源 Origin、`text/plain` CSRF、SQL 读行计数、超大流量、Alarm 调度失败、Rollup 单次写入失败。故障注入只存在于内存中的测试包装器，未改生产源文件。前端 store/router 用 esbuild 转换后在 Node VM 中验证；没有把这些检查等同于真实浏览器验收。

未执行真实 Cloudflare 部署、GitHub Actions、24 小时计费观测、musl 二进制构建/实机运行、actionlint 或屏幕阅读器验收；这些仍是发布验收缺口，不能据此断言对应功能已在线验证。

## 2. 问题清单

| ID | 严重程度 | 位置 | 问题 |
| --- | --- | --- | --- |
| R01 | High | `crates/worker/src/hub/alerts.rs:427`、`:276`；`crates/worker/src/hub.rs:192` | 每条消息扫描历史通知表，每帧又查询全局告警状态，免费读额度随历史/告警数量放大 |
| R02 | High | `crates/worker/src/runtime.rs:92`、`:95`、`:104` | Cookie 管理接口和 Dashboard WS 缺少 Origin/CSRF 校验 |
| R03 | High | `crates/worker/src/hub.rs:215`、`:244`、`:249` | Alarm 两次调度失败后仍成功返回，失去后续调度和平台重试 |
| R04 | High | `crates/worker/src/hub/data.rs:87`、`:24`、`:157` | 未校验的 u64 流量可使 SQLite SUM 溢出，导致全局快照 500 |
| R05 | Medium | `crates/agent/src/queue.rs:44`；`crates/agent/src/session.rs:177`、`:188`；`crates/worker/src/hub.rs:185` | Rollup 写入失败但 WS 未关闭时，未确认分钟不会在当前连接补发 |
| R06 | Medium | `web/src/lib/live.ts:33`、`:59`；`crates/worker/src/hub.rs:184` | 在线看板的本月流量长期停留在连接/改名时的值 |
| R07 | Medium | `crates/protocol/src/api.rs:173`；`crates/worker/src/hub/alerts.rs:169` | Rust PATCH DTO 把缺省字段序列化成 null，服务端拒绝自身 DTO |
| R08 | Medium | `web/src/pages/ServerDetail.tsx:45`、`:49`、`:56` | 切换历史查询失败后仍展示上一次服务器或 range 的数据 |
| R09 | Medium | `crates/worker/src/hub/servers.rs:173`、`:180` | 删除服务器仍保留其全部告警事件，与“删除所有数据”的契约不符 |
| R10 | Low | `web/src/lib/router.ts:13` | 非法百分号编码的 hash 导致未捕获 URIError、页面无法渲染 |
| R11 | Low | `web/src/components/ui.tsx:153`、`:161` | 所有模态对话框缺少可访问名称 |
| R12 | Low | `scripts/dash-watch.mjs:1`、`:3`、`:12` | 命令行看板仍使用 PoC 鉴权和旧事件字段，无法用于 v1 |

### R01 — High：每条消息执行随历史增长的全表扫描

**位置：** `crates/worker/src/hub/alerts.rs:419`、`:427`、`:270`、`:276`；`crates/worker/src/hub.rs:192`、`:108`、`:238`。

**问题与触发：** `dispatch_notifications()` 每次执行 `WHERE notification_json IS NOT NULL ORDER BY id LIMIT 24`。事件表没有能支持此过滤的索引；已发送事件只把列更新为 NULL，行一直保留。`LIMIT 24` 限制返回数量，无法限制没有待发送通知时的扫描量。每条 Hello/Metrics/Rollup，以及绝大多数 Hub HTTP 请求和每个 Alarm 都会调用它，通知全部关闭时也一样。

本地 workerd 插入 100 条 `notification_json=NULL` 的历史事件后，原查询返回 0 条，但 `cursor.rowsRead=100`。30 台服务器每天的 Agent 消息就产生 **56,160,000 行读取**，尚未计算其他查询，远超 5M/日；甚至 10 条历史事件就产生 5,616,000 行。历史告警来自已经删除的服务器也会继续产生开销。

另外，`evaluate_metrics()` 每帧读取并 JOIN **所有服务器**的 `alert_state`，然后才在 Rust 中寻找当前服务器的状态。若全局有 A 个 firing 状态，仅遍历状态行的下界就是 `17,280 × N × A` 行/日，还不含 JOIN 读取。例如 N=30、A=10，仅这个下界就超过 5M。

**建议：** 把待发送通知放进发送后即删除的小型持久队列表，或为现有待发送集合建立窄的部分索引并计入写预算；明确事件保留/清理策略。指标告警只查询当前 server 的状态，并让查询能使用主键范围（例如主键以 server_id 开头），避免每帧读取全局 ActiveAlert 和规则/服务器展示名称。不能用仅存在实例字段中的通知游标替代持久状态。

### R02 — High：同站点不同源页面可利用管理员 Cookie

**位置：** `crates/worker/src/runtime.rs:92`、`:95`、`:104`；`crates/worker/src/hub.rs:301`；`crates/worker/src/hub/servers.rs:84`。

**问题与触发：** 入口只检查管理员 Cookie/Bearer，没有验证 `Origin`；JSON 处理也不要求 JSON Content-Type。假设看板位于 `https://moni.example.com`，攻击者控制 `https://evil.example.com`，管理员已登录后访问后者。两个地址同站点、不同源，`SameSite=Strict` 并不会阻止浏览器向看板发送 Cookie。攻击页面可以：

- 用 `new WebSocket("wss://moni.example.com/ws/dashboard")` 读取 snapshot、主机信息、实时指标和告警。
- 用带 credentials 的简单 `POST`，Content-Type 为 `text/plain`、body 为 JSON，创建服务器、创建规则或轮换已知服务器 ID 的 token。无需先通过 CORS 预检；即使响应不可读，管理操作也已执行。

本地运行实际 Worker 产物，携带登录 Cookie 和 `Origin: https://evil.example.com` 的 Dashboard 握手返回 **101**，收到 snapshot 和 metrics；同样的 Cookie/Origin 加 `text/plain` 创建服务器返回 **201**。该攻击需要上述同站点页面控制条件；这不是“任意外站都能绕过 Strict”的结论。[Cookie 同站点规则](https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis-16#section-5.2)区分站点与源。

**建议：** Cookie 鉴权的写操作和 Dashboard WS 校验 Origin 与请求目标源精确一致；拒绝不允许的 Origin，包括 `null`。浏览器写操作再要求 JSON Content-Type 或 CSRF token。无 Origin 的 CLI 请求可通过显式 Bearer 身份处理，不必破坏现有 CLI 契约。

### R03 — High：调度失败被吞掉后 Alarm 永久停止

**位置：** `crates/worker/src/hub.rs:212`、`:215`、`:244`、`:249`；`:175`。

**问题与触发：** Alarm 开头调用 `schedule_alarm()`，失败后末尾再试一次，但第二次失败仍只记日志，并返回成功。当前 Alarm 被消费后，没有下一次 Alarm；Metrics/Rollup 路径不会检查或恢复 Alarm。写额度暂时耗尽等错误解除后，如果现有 Agent 一直保持连接，维护也不会自动恢复：半开连接检测、离线告警、小时汇总和保留期清理都停止。

对实际 Hub 注入两次 `setAlarm()` Promise rejection，得到 **`status=200, attempts=2, nextAlarm=null`**。平台只对失败的 Alarm handler 自动重试，成功返回会放弃这次恢复机会。[Cloudflare Alarm 重试规则](https://developers.cloudflare.com/durable-objects/api/alarms/#alarm)。

**建议：** 两次调度均失败时返回 Err，让平台重试；保留“先续约，再执行可失败维护”的结构。为超过平台有限重试次数的故障，再提供带限频的入站恢复检查或明确的运维恢复入口，并纳入读写预算。

### R04 — High：单个 Agent 可用大流量值破坏全局快照

**位置：** `crates/worker/src/hub/data.rs:64`、`:87`、`:24`、`:157`、`:165`；`crates/worker/src/hub/servers.rs:45`、`:63`。

**问题与触发：** Rollup 只验证分钟时间，不验证流量范围。`rx_bytes` 经 f64 绑定写入 INTEGER affinity 列；能放进 i64 的大整数仍以 INTEGER 保存。两条合法 JSON/u64 值的和可能超出 SQLite 有符号整数范围。

本地用一台服务器的有效 token，向当前分钟和前一分钟各发送 `rx_bytes=9000000000000000000`、其余值为正常小数值的 Rollup，两条都收到 Ack。随后 **`GET /api/servers` 返回 `500 {"error":"internal",...}`**。月流量 SUM 溢出；snapshot 会计算每台服务器的流量，因此一台机器的数据会使全部机器的看板快照失败。7d 聚合及小时汇总中的 SUM 也使用同样的整数求和。

**建议：** 在 Agent 数据入口验证业务可接受且可安全聚合的字节范围；如果契约确实要求支持完整 u64，调整存储/聚合方法，在越界时返回受控结果，并避免一个服务器的坏数据阻断全部 snapshot。单纯在最终 Rust 求和上使用 `saturating_add` 无效，SQL 已先失败。

### R05 — Medium：一次持久化失败可留下永远不补发的分钟

**位置：** `crates/agent/src/queue.rs:44`；`crates/agent/src/session.rs:132`、`:177`、`:188`；`crates/worker/src/hub.rs:185`。

**问题与触发：** `take_unsent()` 在发送前把条目标成 sent；只有重连时的 `resend_all()` 会重新清除此标志。正常连接里既没有 Ack 超时重发，也没有因未确认分钟而主动重连。Hub 的 `store_rollup()?` 出错时不会明确关闭该连接。

本地对真实 Hub 的首条 Rollup 注入一次 SQL 错误：连接没有关闭，第一条没有 Ack；同一连接的下一分钟写入成功并收到 Ack，history 只有第二条。结合队列代码可知，真实 Agent 之后只发送新的分钟；如果此后 24 小时连接健康，第一条最终会因队列满被丢弃。

**建议：** 最小修复是在 Rollup 持久化失败时明确关闭 WS（可重试关闭码），让既有重连补发机制工作；或为未确认条目设置有界的重发期限/次数。增加“第一次写失败、连接仍存活、后续写成功”的集成测试。

### R06 — Medium：本月流量在长连接页面里不更新

**位置：** `web/src/lib/live.ts:33`、`:37`、`:59`；`crates/worker/src/hub.rs:184`；`web/src/pages/Overview.tsx:47`；`web/src/pages/ServerDetail.tsx:144`。

**问题与触发：** `traffic_month` 只随 snapshot/server_upsert 进入 store。Metrics 分支只更新 metrics、online 和 last_seen；Rollup 和 Alarm 都不广播更新后的 ServerStatus；前端也没有刷新服务器列表。页面保持连接时，即使已经产生了多个分钟的实际流量，本月数字仍停在建立连接或改名时的值，跨 UTC 月份也可能一直展示上个月的总量。

Node VM 执行原 live store：snapshot 的流量为 100/10，随后投递 60 条有网络速率的 metrics，流量仍为 100/10。没有后续事件/请求负责修正这个值。

**建议：** 以分钟级刷新持久化流量，或由后端在 Rollup 后推送契约允许的更新事件。若使用 server_upsert，避免为每台机器每分钟重新执行整个月的 SUM；先解决 R01 并核算新增读量。不要用实时速率积分代替权威月流量，因为断线补发和历史修正会使它失准。

### R07 — Medium：Rust PATCH 请求类型不能产生合法的部分更新

**位置：** `crates/protocol/src/api.rs:173`；`crates/protocol/src/tests.rs:264`；`crates/worker/src/hub/alerts.rs:169`。

**问题与触发：** `AlertRulePatch` 的 Option 字段只有反序列化方面的缺省处理，没有 `skip_serializing_if`。Rust 客户端使用 `AlertRulePatch { threshold: Some(95.0), ..Default::default() }` 序列化时，会同时发送 `name:null`、`metric:null`、`op:null`、`duration_s:null`、`enabled:null` 等。服务端明确拒绝这些 null，返回 **400 bad_request**。现有单元测试反而断言这份不合法的 wire 结果。

本地将上述序列化形状发送给实际规则 PATCH，返回 `Only server_id and recover_threshold may be null`。前端直接构造 JS 对象时不必经过 Rust Serialize，因此当前网页的简单 PATCH 不受这一特定问题影响。

**建议：** 所有 patch 字段的外层 None 序列化时省略，保留 `Some(None)` 对 nullable 字段的显式清除语义。测试应断言序列化结果是 `{"threshold":95.0}`，并由真实 handler 接受。

### R08 — Medium：历史查询失败后，旧数据被放在新查询标题下

**位置：** `web/src/pages/ServerDetail.tsx:45`、`:49`、`:56`、`:68`、`:182`；`web/src/App.tsx:58`。

**问题与触发：** effect 在 id/range 改变时不会清空 history；失败分支也只更新 error。加载 A 的 24h 成功后，直接通过 hash 切换至 B 的详情（组件没有按 id 设置 key），若 B 的请求失败，页面标题和实时卡片已属于 B，但仍绘制 A 的历史和区间流量。另一种普通触发是从 1h 切到 90d，90d 请求失败后，90d 选中状态下继续绘制 1h 数据。

`alive` 能避免旧 effect 的响应在卸载后写状态，但不能给已经保存的 history 标记服务器归属，也不能在失败时撤销旧数据。错误文字无法说明图表实际属于哪个查询。

**建议：** history 连同请求的 id/range 一起保存，仅在与当前选择匹配时渲染；或切换查询时先清空 history，失败时显示对应查询的错误/重试入口。需要保留旧图作为加载占位时，明确标注其归属，不能让它冒充新结果。

### R09 — Medium：服务器删除后仍能查询其告警记录

**位置：** `crates/worker/src/hub/servers.rs:173`、`:180`；`crates/worker/src/hub/alerts.rs:93`；`scripts/e2e.sh:433` 的 preserved 断言。

**问题与触发：** API 契约要求 DELETE 服务器“删除该服务器的所有数据”。实际删除列表没有 `alert_events`，且删除前 resolve 还会追加 resolved 事件。删除一台产生过告警的服务器后，`GET /api/alerts/events` 仍返回其 ID、名称、规则和值；重复增删也不会回收这些数据。完整 e2e 明确断言删除后记录仍被保留，证明实现与契约的分歧已经固化进测试。

**建议：** 按当前契约清除该服务器的事件，并处理尚待发送的恢复通知；相应修改 e2e 的预期。若产品确实要永久保留审计事件，应先由维护者明确改变删除语义和保留期，不能由实现暗中扩大数据保留范围。

### R10 — Low：非法 hash 编码使整个应用无法渲染

**位置：** `web/src/lib/router.ts:13`、`:43`；`web/src/App.tsx:18`。

**问题与触发：** hash 分段后直接 `map(decodeURIComponent)`。访问 `/#/%` 或 `/#/server/%E0%A4%A` 时抛出 URIError；路由 snapshot 和 App 没有错误边界/回退。Node VM 执行原 router，`location.hash="#/%"`、调用 `useRoute()`，得到 `URIError: URI malformed`。用户需手工改 URL 才能恢复。

**建议：** 解析时捕获非法编码，将其归为总览/未找到路由；为非法百分号和不完整 UTF-8 编码补一项路由测试。

### R11 — Low：模态窗口没有可访问名称

**位置：** `web/src/components/ui.tsx:153`、`:161`。

**问题与触发：** 通用 Modal 的 `<dialog>` 没有 `aria-label`/`aria-labelledby`，可见的 h2 也没有可引用的 id。屏幕阅读器用户进入添加服务器、编辑规则或 token 窗口时，dialog 本身没有可访问名称，无法从对话框名称识别当前操作。使用原生 dialog 提供了模态和焦点行为，但不会自动把内部 h2 变成名称。[W3C 模态对话框模式](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/#wai-ariaroles,states,andproperties)要求提供该名称。

**建议：** 用 React `useId()` 给标题唯一 id，dialog 通过 `aria-labelledby` 引用它；进行一次键盘和屏幕阅读器验收。本项证据是缺失的语义属性，没有声称本次已经运行屏幕阅读器。

### R12 — Low：命令行看板脚本与 v1 契约不兼容

**位置：** `scripts/dash-watch.mjs:1`、`:3`、`:12`；`crates/worker/src/auth.rs:105`。

**问题与触发：** 注释提供 `?token=dev-admin` 的使用示例，脚本只传 URL 创建 WebSocket，没有 Cookie 或 Authorization。v1 入口不接受 query token，因此按示例执行会在升级时被 401 拒绝，即使提供正确的管理员密码也无法通过。即使外部另行注入身份，metrics 日志读取的 `ev.server_id` 也已经改成了 `ev.id`。

**建议：** 接受环境变量形式的管理员 Bearer 凭据，并用 Node WebSocket 的 headers 参数发送；改用 `ev.id`，更新示例。秘密不应继续放进 URL 或日志。

## 3. 额度推导

### 3.1 假设和平台计量口径

以下是稳态、连续在线、每个分钟恰好写入一次、默认保留期已跑满、无告警状态切换、无管理操作的推导；不是线上实测。N 为服务器数，E 为已发送历史事件数，A 为全局 firing 状态数。重连、补发、告警和故障重试另加。

计量依据：[Cloudflare DO pricing](https://developers.cloudflare.com/durable-objects/platform/pricing/)规定 Free 为请求 100k/日、SQL 读 5M/日、写 100k/日；入站 WS 消息按 20:1 计请求、协议 Ping 和出站不计请求，删除计写，**每次 setAlarm 也计一行写入**。SQL cursor 的计费行数可由 [rowsRead/rowsWritten](https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/#exec)观察。

本地 workerd 还验证：当前使用的 WITHOUT ROWID 表，首次 INSERT OR REPLACE 和覆盖同一个主键的 INSERT OR REPLACE 均报告 `rowsWritten=1`。因此下面没有把一次覆盖错误地算成两次逻辑行写入；重复消息本身仍各产生一次写。

### 3.2 SQLite 写行数

| 来源 | 每台每天 | Hub 全局每天 | 代码依据 |
| --- | ---: | ---: | --- |
| 分钟 Rollup 写入 | 1,440 | — | `hub/data.rs:75` |
| 7 天分钟保留期删除 | 1,440 | — | `hub/data.rs:233`；稳态每小时删除约 60 行 |
| 小时汇总写入 | **48** | — | `logic.rs:60`、`hub/data.rs:210`；每小时重算前两个完整小时 |
| 90 天小时保留期删除 | 24 | — | `hub/data.rs:237` |
| last_hourly_ms 更新 | — | 24 | `hub/data.rs:242`；已有 meta 行只改 v |
| 有 Agent 时 setAlarm | — | **2,880** | `hub.rs:326`；每 30 秒一次 |
| dirty_hours | 正常及时上报为 0 | — | 仅回补已经汇总的旧小时 |
| 合计 | **2,952** | **2,904** | 不含初始化、连接、告警、补发和管理 |

故稳态基线为：

```text
W_day ≈ 2,952 × N + 2,904
N=30 → 91,464 行/日
N=32 → 97,368 行/日
N=33 → 100,320 行/日（已经超出 100k）
```

`design §5.4` 的“每台约 2,900、约 30 台”作为保守量级基本合理，但每台 24 次小时写入漏掉了第二小时重算，且未列全局 Alarm 写入。**32 台只是没有任何额外工作时的理论上限，不能作为实际承诺；30 台约只剩 8,536 行余量。** 冷启动保留期未跑满时，删除量较低，不能用首日数据代表稳态。

额外开销：

- 每次普通连接生命周期通常有 accept 的 last_seen 更新、Hello 元数据更新和 disconnect 的 last_seen 更新，合计约 3 行；连接替换、失败握手和额外 Hello 会改变这个数。
- 每个 firing/resolved 状态切换约 3 行：插入/删除 alert_state、插入事件、把通知 JSON 置 NULL。即使通知渠道都关闭，仍执行最后一次 UPDATE。一对 firing/resolved 约 6 行。
- 每条补发/重复 Rollup 仍写一行 metrics_1m；旧小时首次标脏再写 dirty_hours 一行，重建时再写小时行并删除 dirty 行。不是“幂等就不消耗写额度”。
- `servers`、`alert_rules`、`meta` 的 TEXT PRIMARY KEY 是 rowid 表的附加唯一索引。插入/删除相关行应计入索引维护；常规只更新非键字段时没有这个主键写放大。两张 metrics 表、dirty_hours、alert_state 用 WITHOUT ROWID，避免了同一主键的额外唯一索引。
- 无 Agent 时是 288 次 Alarm/日；连接数状态改变时可能多一次重新调度。不能把所有 Alarm 写入平均到“每台”后误认为台数越少全局消耗也越小。

### 3.3 DO 请求和 Worker 请求

```text
每台 Metrics：86,400 / 5 = 17,280 条/日
每台 Rollup：1,440 条/日
每台 WS 请求当量：(17,280 + 1,440) / 20 = 936/日
Hub 请求基线：936 × N + 2,880 + 建连/HTTP/重连等
N=30：30,960/日，另加建连和 API
```

这与 `design §5.4` 一致。ADR-0006 中“Rollup 只增加约 3 次请求/天”的旧数字不对：每分钟一条按 20:1 折算是 **72 次/台/天**。协议 Ping、Ack 和 Dashboard 出站广播不增加请求当量。稳定连接的数据帧由 DO 直接处理，不会每帧再次调用入口 Worker。

Worker 请求主要来自 WS 握手和 `/api/*`；静态资源配置为 assets 优先，只对 `/api/*`、`/ws/*` 先执行 Worker。Hub 的 HTTP 转发另计 DO 请求，不能与入口 Worker 请求混为一项。

### 3.4 SQLite 读取与查询规模

**当前真正的瓶颈是 R01，写预算成立不代表系统能在 Free 内长期运行。**

| 路径 | 正常输入下的读取规模 |
| --- | --- |
| 每条 Agent 消息之后的通知扫描 | 队列已清空时仍扫描 E 行；仅该部分为 `18,720 × N × E` 行/日 |
| 每个 Metrics 的告警评估 | 全局 alert_state 和 JOIN，加当前 server 查询；至少 `17,280 × N × A` 状态行/日，实际还含 JOIN 读取 |
| 每个 Rollup | 另读 last_hourly_ms 一行；旧小时需要 dirty 判断/写入 |
| 每个 Alarm | 全部 servers、全局活动告警、通知队列和 last_hourly_ms；正常有 Agent 时每天 2,880 次 |
| 小时重算 | 每小时每台扫描约 120 个分钟行，即约 2,880 分钟行/台/日，另有清理扫描 |
| history 1h/6h/24h | 单服务器主键范围扫描约 60/360/1,440 行 |
| history 7d | 扫描约 **10,080** 个分钟行，返回约 1,008 个十分钟桶；GROUP BY 没有把计费读取降到返回桶数 |
| history 30d/90d | 单服务器小时范围扫描约 720/2,160 行 |
| snapshot / GET servers | 每台分别计算月流量和告警数；扫描本月已汇总小时（最多约 744）及未汇总分钟（正常约 0–60），并读 meta；firing_count 还逐台扫描按 rule_id 排列的 alert_state |

因此 E=10、N=30 时，通知查询本身就有 5,616,000 行/日；E=100 时为 56,160,000 行/日，接近整份 Free 读额度的 11.2 倍。通知关闭也不降低这一扫描。一个完整 snapshot 正常可能读到每台数百行，频繁给 R06 增加全列表轮询必须重新核算。

### 3.5 子请求、时长和隐藏成本

通知派发每次最多取 24 个事件，每个最多 Telegram/Webhook 两个 fetch，即最多 48 个出站子请求；测试通知最多两个。目前未发现单次正常派发超过 50 的路径。通知用 wait_until，5 秒超时、手动重定向并取消响应流；通知挂起期间仍可能增加 DO 时长，不能从“非阻塞”推导为“不计时长”。

未发现常规 Metrics 无状态切换时写 SQLite 的路径；但告警切换可在 Metrics handler 中写入，这是设计允许的写入。不可忽略 setAlarm、通知 JSON 清空、主键索引维护、小时重算和重复 Rollup 等额外写入。

DO GB-s、唤醒 CPU 和跨洲连接延迟不能仅凭本地测试精确推导。Hibernation API 及 `Duration` 调度使用正确，但 30 台免费目标仍需修复 R01 后做至少 24 小时线上观测。

## 4. 测试缺口

1. **查询成本没有回归门槛。** 为 30 台、10/100 条已发送事件、10 个 firing 状态执行真实 handler，记录 SQL rowsRead/rowsWritten；通知关闭时同样测。现有 e2e 只验证响应内容，没有对读取规模作断言。
2. **Cookie 的浏览器边界。** 需要同源允许、同站点外源拒绝、普通跨站拒绝、`Origin:null`、无 Origin Bearer CLI，以及 `text/plain` 简单请求的写操作用例。现有 header 注入/错误 token 测试不能代替 CSRF 测试。
3. **故障后恢复。** 覆盖 Alarm 两次 setAlarm 失败、维护 SQL 出错后下一次仍执行、首条 Rollup 写入失败后补发成功、通知队列超过 24 条后继续排空。现有通知批次测试已覆盖 26 条，但没有覆盖持久化/续约失败。
4. **数值边界。** 包括两个接近 i64 上限的 u64 流量、SUM 越界、无效/极端 CPU/load 数值和 SQLite→Rust 反序列化失败，验证坏数据不会让其他服务器的 snapshot 失败。协议“u64 往返”只验证 JSON，未覆盖 SQLite 算术。
5. **真实 Rust DTO 到 handler。** 对 `AlertRulePatch` 的缺省、null、具体值同时验证序列化和反序列化；不能只断言内存结构的 roundtrip。
6. **历史/保留期时间组合。** 完整 e2e 已覆盖 dirty hour 和清理，但仍缺月初 UTC、闰年月份、保留期最老小时的迟到数据、连续多小时错过 Alarm、保留天数变更等数据库集成测试。已有纯时间函数测试不能证明所有 SQL 组合都正确；迟到数据在下一次小时维护前暂未反映到小时流量，属于当前设计的刷新时机，应明确预期。
7. **休眠状态与 pending。** 缺少正式 v1 的“pending 中休眠→恢复→持续时间正确”、连接替换后 pending 重置、旧连接 closing 后继续投递消息只处理一次等自动化用例。PoC 记录提供了线索，不能代替发布代码的回归。64 个 pending 条目的上限目前只有日志提示，应明确其产品限制，并测试 attachment 最坏大小。
8. **前端缺少自动化测试。** 应覆盖重连 snapshot 覆盖、月流量/UTC 跨月刷新、历史请求失败后的归属、过期 Cookie 导致 WS 握手失败后的登录恢复、非法 hash、图表挂载/卸载清理、dialog 可访问名称。`web/package.json` 目前只有 build/typecheck，没有 UI/store 测试入口。
9. **CI 没跑 e2e。** `.github/workflows/ci.yml` 有 Rust 单测、wasm clippy、类型生成差异、web/worker 构建和 installer 模拟测试，但没有运行 `scripts/e2e.sh`。此外 auth.rs/hub.rs 在 wasm32 cfg 内，native `cargo test -p moni-worker --lib` 实际只执行 9 个纯逻辑测试，并没有执行其中定义的鉴权和限流测试；这部分当前主要依赖手动 e2e。
10. **发布链路尚缺真实验收。** 需要在干净 fork 上验证首次无 secrets→部署→同步 secrets→AGENT_RELEASE_REPO→发 tag→安装命令，检查 amd64/arm64 静态产物和真实 systemd 加固环境下的采样。本地模拟 installer 不验证用户组现有状态、systemd 生效或 musl DNS/TLS。Actionlint 和线上免费计量也未在本次执行。

## 5. 明确无问题的方面

以下限定为已读代码及本地正常路径范围：

- 协议 v2 的 tag、字段名、bytes/s、Unix 毫秒 `_ms`、六个 history range/step 和主要 HTTP 错误码一致；TS 数值映射与前端使用相符。关闭码 4001/4002/4003 的 Agent 退出/延迟策略和 systemd `RestartPreventExitStatus=2 3` 配套。
- Hub 实例字段只有平台句柄、可丢失的登录限流和可重建规则缓存。latest、last_seen、closing、pending 都在 attachment；主机元数据不进入 attachment，规则 ID 和 pending 数量有界。未发现必须跨休眠保存的业务状态误放实例字段。
- Worker/Hub 自有生产代码未发现 `unwrap()`、`expect()`、`panic!` 或用户输入驱动的越界索引。比例计算处理 total=0，时间判断多处使用饱和运算；R04 是 SQL 算术边界问题，不是声称发现 Rust panic。
- 分钟主键实现值幂等，写入成功后才 Ack；不合理时间直接 Ack；dirty_hours 重建、两个完整小时重算、完整最老小时保留，以及正常 UTC 月流量拼接均有实现。完整 e2e 验证了历史连续性、迟到覆盖、小时聚合、清理和离线恢复。
- 告警 pending、持续时间和滞回的纯逻辑、offline 连接恢复、规则删除/禁用清除 firing 在现有范围内正确，e2e 已覆盖；规则修改清除旧 pending，规则缓存更新后失效。
- Cookie 使用 HMAC-SHA256、校验签名和服务端过期时间；HTTPS 下有 Secure、HttpOnly、SameSite=Strict。密码/token 哈希用固定长度摘要作恒定时间比较，签名用 MAC verify。Agent token 使用 32 字节随机数并只存 HMAC；未发现秘密进入应用日志的路径。该肯定不包含 R02 的 Origin 防护。
- 入口先删除两个内部 header，再由可信代码填写；Agent 原 Authorization 转发前移除，错误 token/header 注入用例通过。前端服务器名、主机字段和规则名以 React 文本渲染，未发现将这些字符串送进 innerHTML 的 XSS 路径。
- Agent 按接口处理计数器重置、消失和重建，有界分钟队列、Skip tick、指数退避和协议 Ping；没有每帧进程输出或秘密日志。队列满时明确丢最旧条目，符合当前范围；进程自身崩溃无磁盘队列属于已声明的 v1 限制。
- useNow 和历史轮询均清理 interval；Chart 清理 ResizeObserver 并调用 uPlot.destroy；Live 断开清理重连 timer 和 WS。未发现这些位置的持续资源泄漏。表单标签、可见焦点和在线状态文字已有可访问性处理，dialog 名称问题见 R11。
- installer 校验 SHA256，只选当前架构 manifest 条目，拒绝重复/路径注入和环境文件换行，使用非 root 服务账户、0640 凭据文件、原子替换二进制及临时文件清理。发布资产 `moni-agent-linux-{amd64,arm64}` 与下载名称一致。
- 部署 workflow 的必填 Secrets 检查、0600 临时文件和 trap 清理合理，`AGENT_RELEASE_REPO` 通过 deploy `--var` 注入。核对本地锁定的 Wrangler 4.151.1 实现，secret bulk 只 PATCH secrets，不会因读取配置里的空字符串而覆盖前一步的 repo 变量。首次同步前 API 的 503 与 README 一致；这些是静态/本地核对，仍需真实首次部署验收。
