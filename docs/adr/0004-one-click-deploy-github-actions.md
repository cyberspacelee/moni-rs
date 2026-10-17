# ADR-0004: 通过 GitHub Actions 实现 fork 后一键部署

- 状态：已接受（workflow 已编写，待在真实仓库运行）
- 日期：2026-10-17

## 背景
目标用户 fork 仓库后，只填 Secrets 即可部署，不需要本地安装 Rust/Node/Wrangler。

## 决策
- `deploy.yml`（`workflow_dispatch` + push main）：
  1. 若未配置 `CLOUDFLARE_API_TOKEN` 则跳过（fork 后 push 不会失败）。
  2. 安装 Rust stable + `wasm32-unknown-unknown` + `worker-build@0.8.7`（缓存二进制）。
  3. `npm ci` 安装锁定版本的 wrangler（4.151.1）；在 `crates/worker` 下执行 `wrangler deploy`（构建由 wrangler.jsonc 中的 `worker-build --release` 完成）。
  4. `wrangler secret bulk` 从 GitHub Secrets 同步 `ADMIN_TOKEN`、`AGENT_SECRET`。
  - 未使用 `cloudflare/wrangler-action`：直接调用仓库锁定的 wrangler 更可控，也便于和本地保持一致。
  - 前端（`web/`）引入后再加 `npm run build` 步骤；PoC 阶段为单页 `crates/worker/assets/index.html`。
- `release-agent.yml`（tag `v*`）：`cargo-zigbuild` 构建 `x86_64/aarch64-unknown-linux-musl` 静态二进制（本地已用 docker 版 cargo-zigbuild 验证），发布 GitHub Release；`scripts/install-agent.sh` 下载并安装 systemd 服务。
- 所需 Secrets：`CLOUDFLARE_API_TOKEN`（模板 Edit Cloudflare Workers）、`CLOUDFLARE_ACCOUNT_ID`、`ADMIN_TOKEN`、`AGENT_SECRET`；可选 `TG_BOT_TOKEN`、`TG_CHAT_ID`。

## 后果
- 依赖 ADR-0003 的零预置资源；若将来引入 D1，需要在 workflow 中幂等创建并回写 ID。
- Worker 默认部署到 `*.workers.dev`，国内大陆访问需自定义域名（文档说明）。

## 备选方案
- Cloudflare「Deploy to Cloudflare」按钮 / Workers Builds：构建环境是否带 Rust 工具链不确定，可控性差。
