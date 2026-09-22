# Server 部署

账号版的创建用户、CLI/App 登录、设备撤销与旧配对迁移见 [ACCOUNTS.md](ACCOUNTS.md)。以下配对 token 说明仅用于显式 legacy 流程。

本机已配置 `192.168.0.36:7200` HTTPS/WSS 与 `127.0.0.1:7201` 健康入口，并完成 iOS 模拟器试用。当前按要求已关闭，数据保留。完整本地调试与重新启动说明见 [IOS-LAN-TRIAL.md](IOS-LAN-TRIAL.md)。

当前为单所有者、自托管的配对/密文中转服务。此配置提供 HTTPS/WSS 控制面及中转；终端端点通过加密信令协商 WebRTC，直连失败或严重迟延时使用中转。Server 不持有终端解密密钥。

在仓库根目录生成管理员凭据（不提交到 Git）：

```sh
mkdir -m 700 -p deploy/secrets
openssl rand -hex 32 > deploy/secrets/admin-token
chmod 444 deploy/secrets/admin-token
```

父目录保持 0700；文件 0444 使非 root 容器可读 bind-mounted Compose secret，父目录阻止其他主机用户读取。生产可替换为平台的 secret 管理。

公网部署需要域名解析到服务器，开放 TCP 80/443：

```sh
DOMAIN=terminal.example.com docker compose -f deploy/compose.yaml up -d --build
```

Server 本身不发布 HTTP 端口，Caddy 获取并续期 TLS 证书。SQLite 仅保存配对路由、凭据哈希、过期和撤销信息；备份应覆盖一致性 SQLite/WAL 状态。

桌面生成配对邀请：

```sh
cargo run -p ai-terminal -- --pair --server https://terminal.example.com --server-token-file deploy/secrets/admin-token
```

将输出的 `aiterminal:...` 邀请导入目标手机。邀请含敏感配对能力，只向目标设备分享；`--read-only` 创建观察权限。每份配对有独立桌面公钥/PSK，中转 token 与解密密钥分离。凭据当前 30 天过期，过期后重新配对。

桌面 `--revoke-pair PAIR_ID` 移除本地配对并停止其桥接；Server 管理员也可 `DELETE /v1/pairs/PAIR_ID`，带管理员 Bearer token，立即关闭对应中转连接。停用远程控制不终止正在运行的 Shell。

本机开发（仅 loopback 允许明文 HTTP，内容仍使用应用层 Noise 加密）：

```sh
docker compose -f deploy/compose.local.yaml up -d --build
cargo run -p ai-terminal -- --pair --server http://127.0.0.1:8787 --server-token-file deploy/secrets/admin-token
```

未配置域名/证书前不应使用公网明文地址；客户端会拒绝非 loopback 的 HTTP 地址。WebRTC 默认只有 host candidates，同一局域网可直连；跨 NAT 建议指定自有 STUN/TURN：

```sh
AI_TERMINAL_ICE_SERVERS='stun:turn.example.com:3478' cargo run -p ai-terminal -- --pair --server https://terminal.example.com --server-token-file /path/to/admin-token
```

可选 `compose.turn.yaml` overlay 部署 coturn，需设置 `DOMAIN`、`TURN_PUBLIC_IP`、`TURN_USER`、`TURN_PASSWORD`，开放相应 UDP 端口。它使用部署期长凭据且有配额，是自托管验证配置；短时 TURN 凭据签发/续期属于未完成的发布加固项。不得匿名开放 TURN，也不要将带凭据的 ICE URL 写进公共日志。

本机验证遇到默认 Docker 地址池耗尽，可显式使用 `compose.test-network.yaml`，其默认子网 `10.253.77.0/24` 必须先确认不冲突；无需删除其他项目网络。

当前机器镜像代理拉取失败，已通过缓存镜像离线构建 Linux Release 二进制，并使用 `Dockerfile.runtime` 打包运行。标准 Dockerfile 的精确基础镜像构建尚待正常网络验证。
