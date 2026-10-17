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
   | `CLOUDFLARE_ACCOUNT_ID` | Cloudflare 控制台中的账户 ID |
   | `ADMIN_PASSWORD` | 必填，看板登录密码 |
   | `AUTH_SECRET` | 必填，至少 32 字节；用 `openssl rand -hex 32` 生成，用于签名 Cookie 和对 token 做哈希 |
   | `TG_BOT_TOKEN` / `TG_CHAT_ID` | 可选，Telegram 告警 |
   | `WEBHOOK_URL` | 可选，Webhook 告警 |

3. **Actions → Deploy → Run workflow**。流程构建前端（含 TypeScript 检查），然后由 Wrangler 执行 `worker-build --release` 并部署 Worker，最后用 `wrangler secret bulk` 同步 Secrets。完成后的 **Actions Summary** 会显示从 Wrangler 输出解析出的 Worker URL。
4. 推送 tag（如 `v0.1.0`）触发 **Release agent**，用 cargo-zigbuild 构建静态 musl Agent，发布 `moni-agent-linux-amd64`、`moni-agent-linux-arm64`、`install-agent.sh` 和 `SHA256SUMS` 到 GitHub Release。
5. 在看板 **设置 → 服务器** 中添加服务器，复制生成的安装命令，到 VPS 上执行。

`main` 上的 Worker、协议、前端、Cargo / npm 清单与锁文件、部署 workflow 变更也会自动部署；其他路径的推送不会触发部署。部署时会把当前 Fork 的仓库名注入 `AGENT_RELEASE_REPO`，供看板生成安装命令。

未配置 `CLOUDFLARE_API_TOKEN` 或 `CLOUDFLARE_ACCOUNT_ID` 时，流程会给出 warning 并跳过；Cloudflare 凭据齐全后，缺少 `ADMIN_PASSWORD`、`AUTH_SECRET` 或后者不足 32 字节会在部署前报错。Secret 同步失败会使 workflow 失败，需修复后重新运行；首次部署在同步完成前 API 返回 `503 not_configured`。Secret 临时文件权限为 0600，命令成功或失败后都会删除。

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
npm ci
(cd web && npm ci && npm run build)
npm run deploy -- --var AGENT_RELEASE_REPO:owner/moni-rs  # 替换为实际发布 Agent 的仓库

# 首次部署或更新凭据时，在交互提示中输入值（不要使用开发环境示例值）。
(cd crates/worker && ../../node_modules/.bin/wrangler secret put ADMIN_PASSWORD)
(cd crates/worker && ../../node_modules/.bin/wrangler secret put AUTH_SECRET)
# 可选：同样用 secret put 设置 TG_BOT_TOKEN、TG_CHAT_ID、WEBHOOK_URL
```

## 运维

Agent 安装需要 Linux、systemd、root 权限，以及 `curl`、`sha256sum` 和用户管理工具。脚本按架构下载二进制及 `SHA256SUMS`，校验通过后安装；服务以非 root 的 `moni` 用户运行，并启用 systemd 安全加固。token 只写入 `root:moni` 所有、权限为 0640 的 `/etc/moni/agent.env`。

重复执行看板提供的安装命令会更新二进制、连接配置并重启服务，可用于升级。默认下载最新 Release，也可指定版本和首选上报间隔（实际间隔以 Hub Welcome 为准）：

```sh
curl -fsSL https://github.com/owner/moni-rs/releases/latest/download/install-agent.sh -o install-agent.sh
sudo sh install-agent.sh --server wss://moni.example.com/ws/agent \
  --token mn_xxx_yyy --repo owner/moni-rs --interval 5 --version v0.1.0
systemctl status moni-agent
journalctl -u moni-agent --since '10 minutes ago'
sudo sh install-agent.sh --uninstall
```

卸载会停止并禁用服务，删除二进制、单元文件和 `agent.env`；保留 `moni` 系统账户。无服务时再次卸载也能成功。卸载不会删除看板中的服务器和历史数据，需在看板手动删除。

修改 GitHub Secrets 后需重新运行 **Deploy** 才会同步。未填写的可选通知 Secret 会保留 Worker 上的原值；如需关闭通知，请在 Cloudflare 控制台删除对应 Secret，或在 `crates/worker` 下执行 `../../node_modules/.bin/wrangler secret delete WEBHOOK_URL`（Telegram 同理）。更换 `AUTH_SECRET` 会使所有登录会话和 Agent token 失效，需要重新生成 token 并更新各台 Agent。

本地回归执行 `scripts/e2e.sh`，需要本地开发工具链及 Python 3。脚本使用独立临时状态，覆盖鉴权、CRUD、告警、通知、分钟历史、Worker 重启后的补发及 Agent 冻结后的离线恢复，约需 4–5 分钟；失败时可用 `E2E_KEEP=1 scripts/e2e.sh` 保留日志。安装脚本可用 `sh -n scripts/install-agent.sh` 和 `python3 scripts/test-install-agent.py` 检查，后者使用模拟下载和 systemd，不修改系统配置。

## License

MIT
