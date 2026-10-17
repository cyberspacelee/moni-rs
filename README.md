# moni-rs

一个 Rust 写的 VPS 监控系统，完全跑在 **Cloudflare 免费额度**内，不需要自建服务器。Fork 仓库后，**GitHub Actions 一键部署**。

- **Agent**：Rust 静态二进制（amd64 / arm64 musl），内存约 4 MB。每 5 秒上报实时数据，每分钟上报一条聚合数据（断线自动重发）。
- **后端**：Rust → WebAssembly，跑在 Cloudflare Workers + 单例 Durable Object（内置 SQLite）。Agent 和看板都通过可休眠的 WebSocket 连接。
- **前端**：React + TypeScript + Tailwind CSS，作为 Workers 静态资源托管（静态请求免费）。
- 功能：实时看板、1 小时～90 天历史曲线、本月流量、阈值 / 离线告警（Telegram / Webhook）、按服务器独立的 token。

免费额度下约可监控 **30 台**服务器（瓶颈是 SQLite 写入行数，见 [docs/design.md](docs/design.md) §5.4）。

## 文档

| 文档 | 内容 |
|---|---|
| [docs/architecture.md](docs/architecture.md) | 架构总览 |
| [docs/design.md](docs/design.md) | 详细设计（v1） |
| [docs/api.md](docs/api.md) | HTTP / WebSocket 契约 |
| [docs/adr/](docs/adr/) | 架构决策记录 |
| [docs/research.md](docs/research.md) | Cloudflare 平台事实核实 |
| [docs/poc-report.md](docs/poc-report.md) | PoC 结论与踩坑 |
| [docs/tasks.md](docs/tasks.md) | v1 实施任务 |

## 部署

1. **Fork** 本仓库。
2. 在仓库 **Settings → Secrets and variables → Actions** 中添加：

   | Secret | 说明 |
   |---|---|
   | `CLOUDFLARE_API_TOKEN` | Cloudflare → My Profile → API Tokens → 模板 “Edit Cloudflare Workers” |
   | `CLOUDFLARE_ACCOUNT_ID` | Cloudflare 控制台右侧栏 |
   | `ADMIN_PASSWORD` | 看板登录密码 |
   | `AUTH_SECRET` | `openssl rand -hex 32`，用于签名 Cookie 和对 token 做哈希 |
   | `TG_BOT_TOKEN` / `TG_CHAT_ID` | 可选，Telegram 告警 |
   | `WEBHOOK_URL` | 可选，Webhook 告警 |

3. **Actions → Deploy → Run workflow**。完成后访问 `https://moni-rs.<你的子域>.workers.dev`。
4. 推送 tag（如 `v0.1.0`）触发 **Release agent**，构建 Agent 并发布到 Release。
5. 在看板 **设置 → 服务器** 中添加服务器，复制生成的安装命令，到 VPS 上执行。

> 国内大陆网络无法直接访问 `*.workers.dev`：在 Worker 设置中绑定一个托管在 Cloudflare 的自定义域名即可。

## 本地开发

前置条件：Rust stable（≥1.94）、`rustup target add wasm32-unknown-unknown`、`cargo install worker-build --version 0.8.7`、Node 22。

```sh
npm ci                                              # 安装 wrangler
cp crates/worker/.dev.vars.example crates/worker/.dev.vars
(cd web && npm ci && npm run build)                 # 构建前端到 web/dist
npm run dev                                         # http://127.0.0.1:8787 ，密码见 .dev.vars

# 前端热更新：另开终端，在 web/ 下执行 npm run dev（代理 /api、/ws 到 8787）

# 添加服务器拿到 token 后，启动 Agent：
cargo run -p moni-agent -- --server ws://127.0.0.1:8787/ws/agent --token mn_xxx_yyy
```

常用检查：

```sh
cargo test -p moni-protocol -p moni-agent
cargo clippy -p moni-worker --target wasm32-unknown-unknown -- -D warnings
cargo test -p moni-protocol --features ts            # 重新生成 web/src/api/generated
scripts/e2e.sh                                       # 本地端到端测试
```

## 本地部署（不用 GitHub Actions）

```sh
cp .env.example .env    # 填入 CLOUDFLARE_API_TOKEN / CLOUDFLARE_ACCOUNT_ID
set -a; . ./.env; set +a
(cd web && npm ci && npm run build) && npm run deploy
```

## License

MIT
