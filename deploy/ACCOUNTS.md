# 账号登录与设备管理

账号版使用同一服务上的用户名和密码登录。Desktop 无需 GUI，手机无需管理员 token 或配对 key。服务器是设备身份目录的信任根；终端内容继续由设备之间的 Noise / 经认证的 WebRTC 通道加密。

## 服务端初始化

更新 Server、Desktop 和移动 App 后再切换账号模式。数据库启动时自动新增账号表，保留原 `pairs` 表；升级前使用 SQLite backup API 或停服务后备份数据库及 WAL，不能只复制运行中的主数据库文件。

默认关闭公开注册。管理员在服务器本机交互创建账号，密码至少 12 字节：

```sh
AI_TERMINAL_DB=/path/to/terminal.sqlite3 cargo +stable run -p ai-terminal-server -- user add alice
AI_TERMINAL_DB=/path/to/terminal.sqlite3 cargo +stable run -p ai-terminal-server -- user reset-password alice
```

已安装二进制时用 `ai-terminal-server user add alice`。Compose 部署中在 server 容器执行同一命令，复用容器的 `AI_TERMINAL_DB`：

```sh
docker compose exec server ai-terminal-server user add alice
```

密码由隐藏终端提示读取，不作为命令参数传递。用户名支持字母、数字和 `_.-@`，区分大小写。改密/重置会注销该账号的所有设备会话并关闭相关远程连接。现有管理员 token 仍用于服务启动和显式 legacy 管理，绝不能交给手机用户。

## Desktop

```sh
ai-terminal auth login --server https://terminal.example.com --username alice --name 'My Desktop'
ai-terminal auth status
ai-terminal devices list
ai-terminal devices revoke DEVICE_ID
ai-terminal auth logout
```

CLI 登录后后台 Agent 负责在线状态、凭据刷新和远程连接；可以继续使用 `--list`、`--attach`、`--watch`、`--close` 等原命令。未登录时本地 Shell 仍可用。关闭 CLI 不会关闭 Agent；退出账号关闭远程访问但保留本地 Shell。

使用独立 Agent 状态目录时，所有命令指定相同目录，参数放在子命令之前：

```sh
ai-terminal --state-dir /private/path/to/agent auth login --server https://terminal.example.com
```

macOS 使用 Keychain，Windows 使用系统凭据存储；Linux 桌面使用 Secret Service。无 DBus 的 Linux 自动使用私有配置文件。macOS/Linux 无可用系统凭据库时，可在启动 Agent **之前**显式设置 `AI_TERMINAL_CREDENTIAL_STORE=file`，凭据保存到 `$XDG_CONFIG_HOME/ai-terminal` 或 `~/.config/ai-terminal` 的 0600 文件，目录为 0700。Windows 不提供普通文件回退。`auth status` 显示当前存储方式。不要把这些文件或 App 凭据导出放进 Git。

凭据库暂时锁定时，Agent 保留本地功能，远程功能不可用；解锁并重启 Agent 后恢复。账号私钥不放在 Agent 临时状态目录。服务器使用私有 CA 时，可对 CLI 指定 `--ca-file /path/to/ca.pem`；普通移动登录应使用系统信任的 HTTPS 证书。

账号会话按设备撤销。账号切换先退出旧账号；旧账号的 Shell 不会归属新账号，本地 CLI 仍可查看和关闭它们。首次启用账号模式时，未归属的本地会话归给该账号；之后创建的本地会话使用当前/最近账号归属。

## 手机

在 App 输入 HTTPS 服务地址、账号、密码，登录后选择在线 Desktop，再选择会话。账号设备面板只显示在线设备，离线设备仍保留在账号记录中。Desktop CLI 附着会话期间，有写权限的手机与桌面可同时输入同一 PTY；桌面脱离后手机只能查看画面与历史，桌面重新 `--attach` 后自动恢复输入。Shell 真正结束后只有只读历史，不能复活进程。旧版只读配对始终只能观察。Tab、退格、Esc、方向键和 Ctrl-C 可用硬键盘或特殊键工具栏发送；输入法组合文字只在确认候选后发送。终端画面不使用独立可见的草稿输入框或发送按钮。

Android 普通登录使用 Rust TLS 默认根证书，当前界面尚无私有 CA 导入入口。`https://192.168.0.36:7200` 这类使用本地私有 CA 的部署已在 Debug 验收模式通过，但该模式的 App 私有 CA 文件只用于自动化，不等于普通用户可直接登录；正式本地使用需提供默认根证书信任的证书或后续补充 CA 导入。验收记录见 `doc/task/0924-local-deployment-test/RESULTS.md`。

账号菜单支持刷新设备、修改密码和退出；设备列表支持移除设备。App 进入后台断开自己的输入流，桌面输入不受影响；回前台重新连接会话。登录状态存于 iOS Keychain / Android Keystore 加密存储。

输入操作进入有界客户端队列及收到 ACK 都不代表命令执行完成；应以真实终端回显和 Shell 输出判断结果。连接中断时停止该设备输入，不自动重放结果未知的命令。各设备独立保持输入顺序；同时敲键时字符按 PTY 收到的先后顺序交错。

## 升级与旧配对

新版手机要求 Desktop 支持 `stream/2`；旧 Desktop 会得到明确升级错误，不能默默解释新版流式消息。新版 Desktop 仍支持显式旧邀请连接，便于迁移。

Desktop 一旦登录账号，现有 legacy 连接会关闭，旧配对文件不再加载；退出账号后也不会自动恢复旧邀请。私有配置目录和 Agent 目录中的 `account-mode`/`.mode` 标记用于保留此边界。不得为了修复登录问题随意删除这些标记。

只有明确回退整个开发版本时，才在停止 Agent、撤销账号设备授权、备份数据后，恢复旧构建及其独立旧状态目录。不要让同一 Agent 同时接受账号授权与旧邀请。数据库新增表不修改旧 pairs，旧构建只应使用经过审查的备份。

## 连接与安全边界

Access token 有效 15 分钟，refresh token 随刷新轮换；Server 保存摘要。连接邀请为 60 秒单次授权、绑定双方设备 ID/公钥，既有连接最多 12 小时。后台控制通道检查注销/撤销；丢失控制通道不能无限期维持直连。异常结束后重新选择设备创建新授权。

设备私钥只留在设备。Noise IK 双端静态身份认证绑定完整连接授权；中转看不到终端内容。首次设备身份仍由账号服务认证，因此不声称能抵抗已攻陷的身份目录伪造全新设备。已有设备 ID 的公钥变化会拒绝连接，需要撤销并重新登记。

## 诊断与自动化探针

```sh
cargo +stable test --locked --workspace --exclude ai-terminal-bindgen
cargo +stable clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings
cargo +stable build -p ai-terminal --bin ai-terminal --example input_latency
target/debug/examples/input_latency direct 0 normal 1000
target/debug/examples/input_latency relay 80 normal 1000 40 120 output
target/debug/examples/input_latency relay 80 normal 1000 40 120 history
target/debug/examples/input_latency relay 150 stalls 1000
```

探针需 Unix PTY；`output/history` 负载需 Python 3。参数为路径、注入 RTT、`normal/stalls`、样本数、行、列、负载。使用独立临时用户目录、Agent、数据库和回环服务，完成后停止并清理。`stalls` 是每 100 个读块附加 150ms 的有序停顿，**不是实际 1% UDP 丢包**。回显计时截至 Rust 状态副本，不含原生屏幕呈现。不要把此探针当作公网性能保证。

设置 `AI_TERMINAL_PERF=1` 可记录请求 ID、输入序号及队列/ACK/PTY 入队耗时；不会记录按键内容、密码或令牌。iOS Instruments 可查看 `InputEnqueue` 与 `TerminalDraw` signpost。Debug 的 `AI_TERMINAL_RENDER_BENCHMARK`（App 可写路径）配合 `--render-fixture` 可输出离屏绘制统计，`AI_TERMINAL_RENDER_SAMPLES` 控制样本数。

完整验证与已知限制见 `doc/task/0922-account-input/RESULTS.md`、`HANDOFF.md`。
