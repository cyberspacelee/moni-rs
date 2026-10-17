# Agent 签名更新实现

监控进程保持 `User=moni`、`NoNewPrivileges`、只读系统目录。升级只把明确的稳定版本写入 `/var/lib/moni/update-request`，不执行 shell，也不接受命令提供的下载地址。root 拥有的独立 systemd helper 读取这个不可信请求，下载固定仓库的发布产物，先验证签名、再验证对应架构的 SHA-256，之后原子替换程序。默认无人查看的实时上报周期为 30 秒；1 秒采样与分钟汇总独立。

## 发布设置

使用 RSA 3072 位或以上密钥。私钥放在 GitHub Actions Secret `AGENT_RELEASE_SIGNING_KEY`，公钥提交为 `scripts/release-public.pem`，并通过管理员可信渠道分发到 VPS。Release 也分发 `release-public.pem` 供手动获取，但管理员必须先确认该公钥来自可信仓库或维护者；不得从待验证的 Release 自动下载公钥并信任。公钥更换需要管理员在主机上重新配置。

以下命令仅为维护者配置示例，私钥不得提交到仓库：

```sh
umask 077
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out release-private.pem
openssl pkey -in release-private.pem -pubout -out release-public.pem
gh secret set AGENT_RELEASE_SIGNING_KEY < release-private.pem
```

每次发布先更新 `workspace.package.version`，再创建完全对应的 `vX.Y.Z` tag。发布工作流使用独立 `agent-release` profile（`opt-level=z`、`panic=abort`、LTO、单 codegen unit，仍只 strip 原生 Agent）。Worker 的 release 配置及 wasm-bindgen 所需段不受影响。工作流生成包含两个架构程序、安装/更新脚本及公钥的 `SHA256SUMS`，先以 DER 格式比较私钥导出的公钥与仓库公钥，再生成 `SHA256SUMS.sig`；缺少密钥或公钥不匹配则不发布。

## 启用

首次安装必须通过 `--update-public-key` 提供可信公钥，并使用经过审阅的安装脚本：

```sh
sudo sh install-agent.sh --server wss://monitor.example/ws/agent \
  --token mn_SERVER_SECRET --repo owner/moni-rs \
  --update-public-key /trusted/path/release-public.pem --auto-update
```

本轮 `v0.1.1` 使用协议 3，现有 Agent 需要管理员手动替换为该版本，并在首次安装时显式提供本轮发布的可信公钥。完成后可通过管理端统一升级后续版本，或显式启用自动更新 timer；替换期间主机会短暂离线。

安装和升级都必须通过签名校验；缺少可信公钥或 `SHA256SUMS.sig` 会在修改安装前失败。重装可复用本机已固定的同仓库公钥，无需重复传入 `--update-public-key`。不加 `--auto-update` 时可由管理员统一升级；加上后 systemd timer 每 6 小时检查一次最新稳定版，并加随机延迟。已有配置默认保留，包括接口、磁盘等自定义项；只有 `--reconfigure` 会按参数重新生成连接配置。

安装器把仓库与公钥固定在 root 拥有的 `/etc/moni/update.conf` 和 `/etc/moni/release.pub`，把 helper 放在 `/usr/local/libexec/moni-agent-update.sh`。监控进程只得到 `MONI_UPDATE_REPO`，通过 `wss://` 连接时声明升级能力。请求目录属于 moni，mode 0700；root 私有状态、并发锁及上一版本保存在 `/var/lib/moni-update`。升级请求按完整文件原子发布，helper 先原子移入 root 私有目录再检查，避免跟随请求符号链接。

helper 限制 HTTPS、连接/下载超时和产物大小，先把 latest 解析到明确 tag，禁止降级；验签失败或版本不符不替换现有程序。替换前保留旧程序，新进程必须在 45 秒内完成 Hub Welcome 握手，并写入匹配目标版本与 systemd MainPID 的健康标记，再持续保持 active，否则恢复旧程序并重启。健康标记不是远程证明，Hub 另外等待新连接报告目标版本；下载、验签或回退失败会最终显示为任务超时，详细原因由主机 systemd 日志排查。断电或 SIGKILL 发生在替换过程中时，root 私有 pending 标记会使下次 helper 运行先恢复上一版本；未启用自动 timer 时需要手动启动更新服务完成该恢复。

## 管理 API

部署变量 `AGENT_RELEASE_REPO` 必须与主机固定仓库一致。管理页可调用 `POST /api/agent-updates` 指定版本及服务器 ID，再轮询 `GET /api/agent-updates`；类型与终态见 [api.md §7](api.md#7-管理端统一升级)。本次不修改前端页面。离线任务及状态保存在 SQLite，DO 重建不会丢失。

## 安全审查范围

更新不能绕过本机公钥信任：管理端被盗也不能让 helper 安装未签名的任意程序；签名私钥或可信发布代码被控制仍可能导致整批主机执行恶意程序，需保护仓库和 Actions Secret。监控用户不能修改 helper、固定仓库或公钥。管理端的成功状态依据 Agent 自报版本，不是密码学远程证明；Agent token 泄露后，攻击者仍能伪报版本和指标。

同时修复了管理员改密码后旧 Cookie 仍有效、重复 Hello 反复写库、完全相同 Rollup 重试仍写库的问题。公开看板仍会消耗共享免费额度，公开 WebSocket 握手不能依靠历史缓存消除成本；本次没有添加 Cloudflare 入口限流规则。Agent token 泄露也仍可能被用于制造连接或不同的有效数据，应及时轮换；这些剩余风险不能用签名更新替代处理。
