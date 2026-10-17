# AGENTS.md — moni-rs 开发约定（给 Codex 等编码代理）

## 项目
运行在 Cloudflare 免费额度内的 Rust VPS 监控系统。必读：`docs/design.md`（设计）、`docs/api.md`（前后端契约，**以它为准**）、`docs/tasks.md`（任务与完成标准）、`docs/adr/`。

## 分工
- 你只负责后端：`crates/**`、`scripts/**`、`.github/**`、`Cargo.toml`、`docs/`（仅限补充实现说明）。
- **不要修改 `web/`**，`web/src/api/generated/`（由 ts-rs 生成）和 `web/dist/.gitkeep` 除外。
- 不要执行 `git commit` / `git push`，提交由维护者完成。

## 命令
```sh
cargo test -p moni-protocol -p moni-agent
cargo test -p moni-protocol --features ts           # 生成 web/src/api/generated
cargo clippy -p moni-protocol -p moni-agent -- -D warnings
cargo clippy -p moni-worker --target wasm32-unknown-unknown -- -D warnings
cargo fmt --all
(cd crates/worker && ../../node_modules/.bin/wrangler dev --ip 127.0.0.1 --port 8787)   # 本地运行；需要 crates/worker/.dev.vars
```
`worker-build` 0.8.7 已安装在 `~/.cargo/bin`；wrangler 4.151.1 安装在根目录 `node_modules`。

## 硬性规则
1. **Hub DO 不得在实例字段中保存需要正确性的状态**。实例可能在任意两条消息之间被回收；连接级状态放 WS attachment（≤16 KiB），持久状态放 SQLite（见 design §5.1）。
2. Worker/Hub 代码禁止使用 `unwrap`/`expect`/`panic!`/会越界的索引：panic 会终止 DO 实例。
3. 免费额度：每帧都写 SQLite 是禁止的。只有 Rollup、状态切换、管理操作才写入。尽量不建索引（每个索引行都计入写入行数）。
4. workers-rs 坑：`Storage::set_alarm(i64)` 的参数是**相对偏移**，请统一使用 `Duration`。workspace release profile 不要对 worker 开启 `strip`。
5. 所有 JSON 时间戳都是 Unix 毫秒，字段名以 `_ms` 结尾。
6. 秘密值（token、密码、Cookie 签名）比较时必须与输入无关、时间恒定；永远不要记录日志。
7. 与周边代码风格保持一致；注释只写「为什么」。
8. 依赖版本：sysinfo 锁定 0.38（rustc 1.94）；新增依赖时优先纯 Rust，且要能在 wasm32 下编译（worker 侧）。
