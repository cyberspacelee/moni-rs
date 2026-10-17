# moni-rs

[![CI](https://github.com/cyberspacelee/moni-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/cyberspacelee/moni-rs/actions/workflows/ci.yml)
[![Deploy](https://github.com/cyberspacelee/moni-rs/actions/workflows/deploy.yml/badge.svg)](https://github.com/cyberspacelee/moni-rs/actions/workflows/deploy.yml)
[![Release agent](https://github.com/cyberspacelee/moni-rs/actions/workflows/release-agent.yml/badge.svg)](https://github.com/cyberspacelee/moni-rs/actions/workflows/release-agent.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

一个用 Rust 编写的 VPS 监控系统，完全运行在 **Cloudflare 免费额度**内，不需要自建服务器。Fork 后填好 Secrets，用 **GitHub Actions 一键部署**。

- **Agent**：Rust 静态二进制（amd64 / arm64，musl），内存约 4 MB。每 5 秒上报一次实时数据，每分钟上报一条聚合数据；断线后会自动补发。
- **后端**：Rust 编译为 WebAssembly，运行在 Cloudflare Workers 和一个单例 Durable Object（内置 SQLite）上。Agent 和看板都通过可休眠的 WebSocket 连接。
- **前端**：React + TypeScript + Tailwind CSS，作为 Workers 静态资源托管（静态请求免费）。
- **功能**：实时看板；1 小时到 90 天的历史曲线；本月流量；阈值告警和离线告警（Telegram / Webhook）；每台服务器使用独立 token。

免费额度下大约可以监控 **30 台**服务器。瓶颈是 SQLite 每天的写入行数，详见 [docs/design.md](docs/design.md) §5.4。

## 目录

- [快速开始：部署到你自己的 Cloudflare](#快速开始部署到你自己的-cloudflare)
- [GitHub Actions 说明](#github-actions-说明)
- [Agent 安装与运维](#agent-安装与运维)
- [本地开发](#本地开发)
- [常见问题](#常见问题)
- [文档](#文档)

## 快速开始：部署到你自己的 Cloudflare

整个过程只用到浏览器，本地不需要安装任何工具。

### 1. 准备 Cloudflare 账号

1. 注册或登录 [Cloudflare](https://dash.cloudflare.com/)，进入 **Workers & Pages** 页面一次。**首次进入会让你注册 `*.workers.dev` 子域，请务必完成**，否则 CI 中的首次部署会失败。
2. 记下 **Account ID**：在 Workers & Pages 页面右侧栏，或账户主页 URL 中可以看到。
3. 创建 API Token：头像 → **My Profile → API Tokens → Create Token**，选择 **Edit Cloudflare Workers** 模板。Account Resources 只选你的账户，Zone Resources 可以选 *All zones*（以后绑定自定义域名时需要），然后生成并复制 token。

### 2. Fork 仓库并启用 Actions

Fork 本仓库后，打开 **Actions** 页签，点击 *I understand my workflows, go ahead and enable them*。Fork 的仓库默认禁用 Actions。

### 3. 配置 Secrets

在 **Settings → Secrets and variables → Actions → New repository secret** 中逐个添加：

| Secret | 必填 | 说明 |
|---|:---:|---|
| `CLOUDFLARE_API_TOKEN` | ✅ | 第 1 步创建的 API Token |
| `CLOUDFLARE_ACCOUNT_ID` | ✅ | 第 1 步记下的 Account ID |
| `ADMIN_PASSWORD` | ✅ | 看板登录密码，建议使用 16 位以上的随机字符串 |
| `AUTH_SECRET` | ✅ | **至少 32 字节**的随机串，用于签名 Cookie 和对 Agent token 做哈希。可以用 `openssl rand -hex 32` 生成 |
| `TG_BOT_TOKEN` / `TG_CHAT_ID` | | 可选，Telegram 告警（向 [@BotFather](https://t.me/BotFather) 申请 bot） |
| `WEBHOOK_URL` | | 可选，告警时 POST JSON `{event, rule, server, value, ts_ms}` |

> ⚠️ 更换 `AUTH_SECRET` 会让所有登录会话和**所有 Agent token** 失效。设置后不要随意更改。

### 4. 部署

打开 **Actions → Deploy → Run workflow**，大约需要 5～10 分钟（首次需要编译 Rust）。完成后，运行页面的 **Summary** 会显示 Worker 地址，形如 `https://moni-rs.<你的子域>.workers.dev`。用 `ADMIN_PASSWORD` 登录即可。

### 5. 发布 Agent

在仓库页面 **Releases → Draft a new release** 中新建一个 tag（例如 `v0.1.0`）并发布；也可以在本地执行 `git tag v0.1.0 && git push origin v0.1.0`。推送 tag 后会触发 **Release agent**，构建结果会自动附加到这个 Release 上，包括 `moni-agent-linux-amd64`、`moni-agent-linux-arm64`、`install-agent.sh` 和 `SHA256SUMS`。

> 安装脚本从 GitHub Release 下载二进制，所以发布 Agent 的仓库需要是**公开**的。

### 6. 接入服务器

在看板中打开 **设置 → 服务器 → 添加服务器**，复制弹窗中的安装命令（token 只显示一次），在 VPS 上以 root 执行：

```sh
curl -fsSL https://github.com/<owner>/moni-rs/releases/latest/download/install-agent.sh \
  | sudo sh -s -- --repo <owner>/moni-rs \
      --server wss://moni-rs.<subdomain>.workers.dev/ws/agent \
      --token mn_<server_id>_<secret>
```

几秒后，服务器就会出现在总览中。

## GitHub Actions 说明

| Workflow | 触发方式 | 内容 |
|---|---|---|
| **CI** (`ci.yml`) | push `main`、Pull Request | fmt、clippy（native + wasm32）、单元测试、TS 类型与 Rust 协议一致性检查、前端构建、Worker 构建、安装脚本测试、quick e2e + 浏览器冒烟测试 |
| **Deploy** (`deploy.yml`) | 手动运行；push `main` 且改动涉及 Worker / 协议 / 前端 / 依赖清单 / 本 workflow | 构建前端 → `wrangler deploy`（注入 `AGENT_RELEASE_REPO=<当前仓库>`）→ `wrangler secret bulk` 同步 Secrets → Summary 输出地址 |
| **Release agent** (`release-agent.yml`) | push `v*` tag | cargo-zigbuild 构建 amd64 / arm64 静态 Agent，连同安装脚本和校验和一起发布到 Release |

几点说明：

- 没有配置 `CLOUDFLARE_API_TOKEN` 或 `CLOUDFLARE_ACCOUNT_ID` 时，Deploy 会给出 warning 并**跳过**，不会失败。因此 fork 之后先 push 也没有问题。
- Cloudflare 凭据齐全后，如果缺少 `ADMIN_PASSWORD` 或 `AUTH_SECRET`，或者 `AUTH_SECRET` 不足 32 字节，会在部署前报错。
- 修改 Secrets 后，需要重新运行 Deploy 才会同步到 Worker。没有填写的可选 Secret 会保留 Worker 上的原值。要关闭某个通知渠道，请在 Cloudflare 控制台删除对应的 Secret。
- 首次部署在 Secret 同步完成之前，API 会返回 `503 not_configured`，属于正常现象。
- 同步 Secrets 时使用的临时文件权限为 0600，命令无论成功还是失败都会删除。

## Agent 安装与运维

需要 Linux、systemd 和 root 权限，以及 `curl`、`sha256sum`、`useradd`。安装脚本会：

- 按架构下载二进制，并用 `SHA256SUMS` 校验；
- 以非 root 用户 `moni` 运行服务，并启用 systemd 安全加固；
- 把 token 写入 `/etc/moni/agent.env`（属主 `root:moni`，权限 0640）。

```sh
# 指定版本 / 上报间隔（实际间隔以服务端下发为准）
curl -fsSL https://github.com/<owner>/moni-rs/releases/latest/download/install-agent.sh -o install-agent.sh
sudo sh install-agent.sh --server wss://<host>/ws/agent --token mn_xxx_yyy \
  --repo <owner>/moni-rs --interval 5 --version v0.1.0

systemctl status moni-agent
journalctl -u moni-agent --since '10 minutes ago'

# 升级：重新执行安装命令即可（幂等）
# 卸载：停止服务，删除二进制、单元文件和 agent.env；保留 moni 账户和看板中的历史数据
sudo sh install-agent.sh --uninstall
```

可选配置见 [scripts/agent.env.example](scripts/agent.env.example)：网卡白名单 `MONI_IFACES`、挂载点白名单 `MONI_DISKS`、日志级别 `RUST_LOG`。

## 本地开发

前置条件：Rust stable（≥ 1.94）、`rustup target add wasm32-unknown-unknown`、`cargo install worker-build --version 0.8.7`、Node 22。

```sh
npm ci                                                # 安装 wrangler
cp crates/worker/.dev.vars.example crates/worker/.dev.vars
(cd web && npm ci && npm run build)                   # 构建前端到 web/dist
npm run dev                                           # http://127.0.0.1:8787，密码见 .dev.vars

# 前端热更新：另开一个终端，在 web/ 下执行 npm run dev（会把 /api、/ws 代理到 8787）
# 注意：wrangler dev 只在启动时读取 web/dist，重新构建前端后需要重启 npm run dev

# 在看板中添加服务器拿到 token 后，启动 Agent：
cargo run -p moni-agent -- --server ws://127.0.0.1:8787/ws/agent --token mn_xxx_yyy
```

常用检查：

```sh
cargo test -p moni-protocol -p moni-agent -p moni-worker
cargo clippy -p moni-worker --target wasm32-unknown-unknown -- -D warnings
cargo test -p moni-protocol --features ts     # 重新生成 web/src/api/generated
scripts/e2e.sh --quick --ui                   # 端到端测试 + 浏览器冒烟测试（约 40 秒）
scripts/e2e.sh                                # 完整版，含 Worker 重启补发、冻结判离线（约 4–5 分钟）
```

失败时可以用 `E2E_KEEP=1` 保留日志。安装脚本可以用 `python3 scripts/test-install-agent.py` 测试，它会模拟下载和 systemd，不修改系统配置。

### 不用 GitHub Actions，在本地直接部署

```sh
cp .env.example .env            # 填入 CLOUDFLARE_API_TOKEN / CLOUDFLARE_ACCOUNT_ID（.env 已被 gitignore）
set -a; . ./.env; set +a
npm ci && (cd web && npm ci && npm run build)
npm run deploy -- --var AGENT_RELEASE_REPO:<owner>/moni-rs
(cd crates/worker && ../../node_modules/.bin/wrangler secret put ADMIN_PASSWORD)
(cd crates/worker && ../../node_modules/.bin/wrangler secret put AUTH_SECRET)
```

## 常见问题

**国内大陆打不开看板？** `*.workers.dev` 在大陆无法直接访问。在 Worker 的 **Settings → Domains & Routes** 中绑定一个托管在 Cloudflare 的自定义域名，安装命令中的 `--server` 也要改用这个域名。

**首次 Deploy 报错，提示需要注册 workers.dev 子域？** 到 Cloudflare 控制台的 Workers & Pages 页面完成子域注册后，重新运行即可。

**能监控多少台服务器？** 免费计划约 30 台（5 秒间隔、分钟级历史保留 7 天）。超出后需要升级 Workers Paid，或者调整历史粒度，参见 [docs/design.md](docs/design.md) §5.4。

**Agent 断网会丢数据吗？** 不会。分钟聚合数据会保存在 Agent 内存队列中（默认 24 小时），重连后补发。只有 Agent 进程本身崩溃时，才会丢失尚未确认的数据。

## 文档

| 文档 | 内容 |
|---|---|
| [docs/architecture.md](docs/architecture.md) | 架构总览、数据流、额度预算 |
| [docs/design.md](docs/design.md) | 详细设计（v1） |
| [docs/api.md](docs/api.md) | HTTP / WebSocket 接口约定 |
| [docs/adr/](docs/adr/) | 架构决策记录 |
| [docs/research.md](docs/research.md) | Cloudflare 平台事实核实 |
| [docs/poc-report.md](docs/poc-report.md) | PoC 结论与踩坑 |
| [docs/tasks.md](docs/tasks.md) | v1 实施任务 |
| [docs/review/](docs/review/) | 代码审查与验收报告 |

## License

[MIT](LICENSE)
