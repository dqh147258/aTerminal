# 本机 Docker 与 iOS 模拟器试用

配置日期：2026-09-22。固定主机 IP：`192.168.0.36`。

本文是独立 iOS 试用环境的启动与关闭步骤；实时运行状态以本页的检查命令为准。数据卷、证书、配对、Keychain 和构建产物可跨重启保留。以下命令均在 `/Volumes/Code/My/aTerminal` 项目根目录执行。

从旧名称迁移后需要重新构建并安装 App（使用下文的 `--build`），新包名为 `com.yxf.aterminal`，原 App 数据不会自动迁移。如果项目目录也从 `AITerminal` 改为 `aTerminal`，旧 Swift 模块缓存会记录原绝对路径；构建出现 `PCH was compiled with module cache path` 时，先清理生成的缓存 `build/xcode/ModuleCache.noindex` 和 `build/swift-module-cache`，再重新构建。

## 已配置入口

| 用途 | 地址 |
| --- | --- |
| iOS/桌面连接、配对、信令、中转 | `https://192.168.0.36:7200` |
| 本机 HTTPS | `https://127.0.0.1:7200` |
| 回环健康检查 | `http://127.0.0.1:7201/healthz` |

Docker 项目名 `ai-terminal-dev`，运行 `server` 与 `lan_tls` 两个容器，复用已有 SQLite 数据卷。宿主只发布 7200/7201；8787 仅是 Server 容器内部端口。7202–7210 未占用。WebRTC 的端点 ICE/UDP 端口是动态端口，不是 Docker Server 的额外发布端口。

## 直接试用

先启动环境和模拟器：

```sh
cd /Volumes/Code/My/aTerminal
python3 scripts/prepare-lan.py
python3 scripts/run-ios-lan.py --toolchain stable --restore --interactive
```

`prepare-lan.py` 启动本项目 Docker 服务，复用证书和数据库，不更改其他服务。`run-ios-lan.py` 启动 Agent，沿用配对，在完整关闭后创建新的 Shell，会话 ID 随之更新。

iPhone SE 模拟器已安装 aTerminal，配对已保存到 Keychain。可写配对打开会话后即可点击终端画面输入并按 Return，Tab、Esc、方向键和 Ctrl-C 由特殊键工具栏或硬键盘提供；只读配对仅能观察。`--interactive` 不会自动发送测试命令。

桌面只观察同一会话：

```sh
SESSION_ID="$(python3 -c 'import json; print(json.load(open(".local/ios-lan/demo.json"))["session_id"])')"
./target/debug/aTerminal --state-dir "$PWD/.local/ios-lan" --attach "$SESSION_ID" --watch
```

去掉 `--watch` 即可从桌面附着并与手机同时输入；Ctrl+] 脱离后 Shell 继续运行，但新版 Agent 会把手机转为只读历史，重新 `--attach` 恢复输入。当前会话 ID 和连接地址保存在 `.local/ios-lan/demo.json`。

## 环境与重建

已使用的本机环境：Docker Desktop、Rust 1.94.1、Xcode 15.4/iOS 17.5 Simulator、CMake、Perl、libclang、Python 3 和 OpenSSL。确认当前网卡仍有 `192.168.0.36`，7200/7201 未被其他程序占用。`stable` 是本机现有 Rust 1.94.1 的别名；其他机器按 `rust-toolchain.toml` 安装固定版本，并省略 `--toolchain stable`。

查看工具链和可用模拟器：

```sh
rustc +stable --version
xcodebuild -version
xcrun simctl list devices available
docker info
```

模拟器 UUID 在 `deploy/ios-lan.json`。iOS Rust 目标为 `aarch64-apple-ios` 加当前 Mac 的模拟器目标：Intel 使用 `x86_64-apple-ios`，Apple Silicon 使用 `aarch64-apple-ios-sim`。

修改 Rust/Swift 或缺少客户端产物后重建：

```sh
python3 scripts/stop-local.py
python3 scripts/prepare-lan.py
python3 scripts/run-ios-lan.py --toolchain stable --build --restore --interactive
```

先停止旧 Agent，避免新 CLI 连接到仍运行的旧代码。`--build` 重建桌面 CLI、iOS 双静态库、bindings、XCFramework 和 Xcode App。若模拟器/Keychain 是新的，去掉 `--restore`，脚本会导入当前邀请。

只改 Swift UI 时，可打开 `apps/ios/aTerminal.xcodeproj`，选择 aTerminal scheme 和 iPhone SE 模拟器运行，或执行：

```sh
xcodebuild -project apps/ios/aTerminal.xcodeproj -scheme aTerminal \
  -sdk iphonesimulator -configuration Debug -derivedDataPath build/xcode \
  ARCHS=x86_64 CODE_SIGN_IDENTITY=- build
python3 scripts/run-ios-lan.py --toolchain stable --restore --interactive
```

上例匹配本机 x86_64 模拟器 XCFramework。Apple Silicon 主机需先构建 `aarch64-apple-ios-sim` 库，并将 `ARCHS` 改为 `arm64`。

**`prepare-lan.py` 只启动已有的 `ai-terminal-server:development` 镜像，不编译 Server。** 修改 Server 代码或镜像缺失时先构建并更新容器：

```sh
BUILDX_CONFIG="$PWD/build/buildx" docker build -f deploy/Dockerfile -t ai-terminal-server:development .
python3 scripts/prepare-lan.py
```

标准镜像拉取需要正常访问镜像仓库；本机此前的镜像代理有 EOF 问题，当前可复用已构建镜像。不要将这类镜像拉取错误误认为 Rust 编译错误，也不要为调试改动其他项目的 Docker 配置。

## 重复启动与验证

在项目根目录执行：

```sh
python3 scripts/prepare-lan.py
python3 scripts/run-ios-lan.py --toolchain stable --restore --interactive
```

`stable` 在本机是 Rust 1.94.1。首次重建全部客户端产物可以加 `--build`；脚本会构建 CLI、双 iOS 静态库、bindings、XCFramework 和正式 Xcode 应用。不要用手工链接 probe 替代 Xcode 的 Keychain 签名。

需要重跑自动化试用检查时：

```sh
# 导入当前邀请，验证直连及唯一测试输出
python3 scripts/run-ios-lan.py --toolchain stable

# 只依靠 Keychain 恢复，并强制经 WSS 中转验证
python3 scripts/run-ios-lan.py --toolchain stable --restore --relay-only

# 回到不注入测试命令的正常试用模式
python3 scripts/run-ios-lan.py --toolchain stable --restore --interactive
```

测试会发送一个带时间戳的 `printf` 命令，并同时核对桌面权威屏幕和 iOS 接收的屏幕，验证独立输出行、连接路径及输入能力；不是仅检查命令回显。测试配置在 `deploy/ios-lan.json`，可重新指定可用模拟器 UUID。

## 日志与断点

```sh
# 容器状态及 Server/TLS 日志
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml ps -a
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml logs --tail 100 server lan_tls

# 桌面 Agent 日志和会话列表
tail -n 100 .local/ios-lan/agent.log
./target/debug/aTerminal --state-dir "$PWD/.local/ios-lan" --list

# 定向自动化检查
cargo +stable test --locked -p ai-terminal --test agent
cargo +stable test --locked -p ai-terminal-remote --features webrtc
cargo +stable clippy --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings
```

Swift 断点放在 `apps/ios/aTerminal/aTerminalApp.swift` 的连接、选择会话、发送与刷新方法；网络状态在 `crates/mobile-core/src/remote.rs`，传输选路在 `crates/remote/src/channel.rs`，终端权威状态在 `crates/desktop-agent/src/service.rs`。网络/FFI 调用运行于后台队列；不要为了调试把阻塞请求移到 UI 主线程。

自动试用截图位于 `build/screenshots/ios-lan-*.png`。不要打印 `invitation.txt`、`pairs/*.json` 或管理员 token 到公开日志。

## 完整关闭

```sh
python3 scripts/stop-local.py
```

此脚本关闭配置指定的 iOS App/模拟器、`.local/ios-lan` 的 Agent 和其 Shell，以及本项目两个 Docker 容器；没有其他运行模拟器时才退出 Simulator 窗口。它不会关闭 Docker Desktop 或其他项目，也不删除卷、证书、配对或构建文件。

局域网 Compose 给 Rust Server 指定 `stop_signal: SIGINT`，与现有优雅退出处理一致。本次旧容器在配置更新前以 137 强停；新配置会在下次 `prepare-lan.py` 重建容器时生效，本次没有为验证退出信号而重新启动服务。

完整关闭会终止 Shell 及其当前任务，无法在下次恢复原进程；脚本将旧 `demo.json` 归档为 `demo.last.json`，下次启动沿用配对创建新 Shell。仅需暂时退出桌面客户端时使用 Ctrl+]，不要执行完整关闭。

关闭后的检查：

```sh
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml ps -a
lsof -nP -iTCP:7200-7201 -sTCP:LISTEN
xcrun simctl list devices available
```

预期两个项目容器为 Exited，7200/7201 无监听，配置的 iPhone SE 为 Shutdown。

## 常见问题

| 现象 | 处理 |
| --- | --- |
| 7200 拒绝连接 | 检查 Docker 已启动、执行 `prepare-lan.py`，查看 TLS 容器日志和端口冲突 |
| `address pools` 已用尽 | 保留现有 `compose.test-network.yaml` 的项目专用子网，不删除其他项目网络 |
| 证书不受信任 | App 使用邀请中的 CA；curl 必须传 `--cacert`。不要添加跳过证书校验选项 |
| 固定 IP 改变 | 同步修改 compose 绑定地址、`ios-lan.json`、`lan-cert.cnf`，再签发匹配地址的服务证书；旧证书的 SAN 不会自动变化 |
| `session not found` 或 `saved demo session exited` | 先运行 `stop-local.py` 归档旧会话记录，再按启动步骤创建新会话，不要删除配对或数据库 |
| `--restore` 无有效配对 | 去掉 `--restore` 从当前邀请重新导入；配对过期/已撤销时需显式重新创建配对 |
| Keychain 权限错误 | 使用正式 Xcode 工程构建；手工 `swiftc` probe 没有完整的模拟器签名元数据 |
| 源码已修改但行为未变 | 停止旧 Agent，使用 `--build` 重建并重新安装；Server 改动则重建 Docker 镜像 |
| 模拟器服务权限错误 | 在本机 Terminal/Xcode 执行相关命令；受限沙箱可能无法访问 CoreSimulator |

## TLS 与凭据

- `deploy/secrets/lan-ca.crt`：项目测试 CA 公钥证书；没有导入宿主系统全局信任。
- `deploy/secrets/lan-ca.key`：CA 私钥，仅宿主可读，不挂载到容器。
- `deploy/secrets/lan-server.crt` / `.key`：TLS 代理证书及密钥，SAN 包含固定 IP、127.0.0.1 和 localhost；本次叶证书有效期至 2026-12-20。
- `deploy/secrets/admin-token`：管理员凭据。
- `.local/ios-lan/invitation.txt`：给本次试用设备的完整邀请，包含配对能力和服务器 CA，不应公开。
- `.local/ios-lan/pairs/`：桌面端配对密钥；`.local/ios-lan/` 为 0700，邀请文件为 0600。

上述私有目录已忽略版本控制。CA 会随已授权的配对邀请显式加入该连接的信任；HTTPS 和 WSS 都继续验证证书与主机名，没有关闭验证。

浏览器未安装测试 CA 时访问 7200 会提示不受信任，这是预期行为；App 已配置专用信任。用以下命令检查 HTTPS：

```sh
curl --noproxy '*' --cacert deploy/secrets/lan-ca.crt https://192.168.0.36:7200/healthz
```

## 验证记录

- 两容器 Healthy，192.168.0.36 与 127.0.0.1 的 7200 HTTPS 返回 `ok`。
- 不提供测试 CA 时 TLS 被拒绝；即使提供 CA，使用错误主机名也被拒绝。
- iOS 模拟器通过局域网 HTTPS/WSS 配对，建立 WebRTC 直连并成功输入。
- 重启应用后从 Keychain 恢复，强制 WSS 中转仍能操作同一会话。
- 截图保存在 `build/screenshots/ios-lan-direct.png`、`ios-lan-relay-restored.png` 和 `ios-lan-direct-restored-interactive.png`。

本次测通的是本机局域网与模拟器，不代表跨公网/蜂窝或真机验证完成。
