# ADR-0005: 鉴权模型（v1）

- 状态：已接受（取代 PoC 的共享密钥方案）
- 日期：2026-10-17

## 决策
- **管理员**：单一管理员密码 `ADMIN_PASSWORD`（Secret）。登录后签发 HMAC 签名的 HttpOnly Cookie（`AUTH_SECRET`，30 天）；`Authorization: Bearer <ADMIN_PASSWORD>` 拥有同等权限，供脚本使用。登录失败按 IP 限流。
- **Agent**：每台服务器一个 token：`mn_<server_id>_<random>`。只存储 `HMAC-SHA256(AUTH_SECRET, token)`；Worker 负责计算哈希，Hub 负责比对。token 支持轮换，轮换后旧连接以 4003 关闭。
- **内部 header**（`x-moni-server-id`、`x-moni-token-hash`）在 Worker 入口先剥离，再由 Worker 自己设置；Hub 只信任 Worker 转发过来的请求（DO 本身不能被公网直接访问）。
- 推荐（文档说明，不内置）：在看板前加一层 Cloudflare Access 做第二道防线，同时为 `/ws/agent` 设置 bypass。

## 后果
- 多用户、RBAC 不在 v1 范围内。
- 更换 `AUTH_SECRET` 会使所有会话和 Agent token 失效（需要重新生成 token）。
