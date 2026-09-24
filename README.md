# AI Terminal

Rust 桌面终端、原生 Android/iOS 客户端与轻量协调服务器。设计采用桌面唯一终端状态、原生显示副本、WebRTC 优先和 WSS 兜底。

**当前是可运行的开发版本，尚未完成发布验收。** 已实现后台会话、配对、端到端加密中转、WebRTC 直连与超时切换、Android/iOS 原生客户端。移动工作台按设计更新，并新增 Web Admin 和 Desktop 模型对话。跨公网 NAT、长期性能和发布签名仍属于发布验收范围。

## 本地运行

需要 Rust 1.94.1。仓库锁定工具链和 Cargo.lock：

```sh
cargo run -p ai-terminal
cargo run -p ai-terminal -- --cwd /path/to/project
cargo run -p ai-terminal -- -- /bin/zsh -i
```

Windows 默认优先 PATH 中的 `pwsh.exe`，否则使用 `powershell.exe`，可显式指定：

```powershell
cargo run -p ai-terminal -- -- pwsh.exe -NoLogo
```

进入独立 Shell，`exit` 结束会话并恢复宿主终端；按 **Ctrl+]** detach，Shell 保留在后台 Agent。

```sh
cargo run -p ai-terminal -- --list
cargo run -p ai-terminal -- --attach SESSION_ID
cargo run -p ai-terminal -- --attach SESSION_ID --watch
cargo run -p ai-terminal -- --history SESSION_ID
cargo run -p ai-terminal -- --close SESSION_ID
cargo run -p ai-terminal -- --agent-stop
```

`--attach` 接管输入权；`--watch` 只读且不会修改共享尺寸。`--close` 终止指定会话，`--agent-stop` 终止所有本地会话。`--state-dir` 可隔离多套 Agent，目录必须私有；默认使用当前用户的临时目录，端点凭据不应分享。输入确认表示已进入写队列，不等于命令完成；抢占时尚未写入的旧控制队列会取消，已经开始的写入不能撤销。

不能无损接管已有 Shell 的内存状态。鼠标捕获、部分特殊键、字形宽度和宿主历史仍属于兼容性验证范围。暂使用明确的 `xterm-256color` 基线；完整能力合同及产品 terminfo 是后续发布门槛。

无交互地捕获程序结束时的 Protobuf 屏幕：

```sh
cargo run -p ai-terminal -- --snapshot /tmp/screen.pb -- /bin/sh -c 'printf "hello\n"'
```

`--rows`、`--cols` 指定捕获尺寸；`--timeout-secs` 默认 30 秒；非零子进程退出码保留。

## 验证

```sh
cargo test --locked --workspace --exclude ai-terminal-bindgen
cargo clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings
cargo build -p ai-terminal
python3 scripts/test-host-terminal.py target/debug/ai-terminal
cargo test --locked -p ai-terminal-transport --test loopback
```

测试覆盖真实本机 ICE/DTLS/SCTP、超时转中转后不重复执行、撤销连接但保留 Shell、只读设备拒绝写入。需要 C/C++ 编译器、CMake、Perl、libclang；Windows 原生 WebRTC 构建还需要 MSVC/NASM。测试不代表所有 NAT/蜂窝网络均已通过。

若现有 `stable` 恰为 Rust 1.94.1，可以临时使用 `cargo +stable`；脚本支持 `--toolchain stable`。这不修改全局配置，正式 CI 仍锁定 1.94.1。

## 账号登录与远程连接

已增加内置账号、设备管理和移动登录。管理员创建账号后，Desktop 登录一次，手机登录同一账号即可发现设备；不再需要用户复制配对 key。部署、迁移和凭据存储见 [账号使用说明](deploy/ACCOUNTS.md)。

```sh
# 在服务端运行，交互输入密码
AI_TERMINAL_DB=/path/to/terminal.sqlite3 ai-terminal-server user add alice
# Desktop
ai-terminal auth login --server https://terminal.example.com --username alice
ai-terminal auth status
ai-terminal devices list
ai-terminal auth logout
```

手机输入服务地址、账号和密码，选择 Desktop/会话并接管输入。新版采用 `stream/2` 主动状态推送、有界异步输入和原生增量绘制；所有端需升级。Desktop 登录账号后不再加载旧邀请，即使退出账号也不会自动恢复旧邀请访问。

### Web Admin 与 AI 对话

服务内置 `/admin/`，使用现有管理员令牌登录，可查看服务概览、创建用户、重置密码、查询和撤销设备及连接；无需单独前端服务。部署与 API 见 [Web Admin](deploy/ADMIN.md)。

Android/iOS 提供全屏远端 Terminal、局部半透明设置浮窗、工作空间侧滑菜单、最后会话恢复和显示设置，字号滑块实时预览。移动端 AI 浮窗当前仅显示不可交互的占位 UI，不发起模型请求、终端监控或录音；历史数据仍可查阅。Desktop 端实验性模型配置和安全边界见 [Desktop AI](deploy/ASSISTANT.md)，不代表当前移动端入口已开放。

### 旧邀请迁移入口

Server 部署和凭据配置见 [deploy/README.md](deploy/README.md)。桌面创建并 detach 一个会话，再生成配对邀请：

```sh
cargo run -p ai-terminal -- --pair --server https://terminal.example.com --server-token-file /path/to/admin-token
```

手机粘贴邀请连接，选择会话；默认只读，切换“接管输入”即可申请/释放当前会话控制权。按键由桌面根据当前终端模式编码，手机不自动改变共享尺寸。后台时释放控制并断开，回到前台点“连接”从安全存储恢复配对，再选择会话。

配对前可通过 `AI_TERMINAL_ICE_SERVERS` 配置逗号分隔的 STUN/TURN URL；默认只收集本机候选，跨 NAT 通常需要自有 STUN/TURN。无法直连时继续使用 WSS。终端消息不会明文经过 Server：WSS 使用 Noise，WebRTC 使用经 Noise 认证的 SDP fingerprint 建立 DTLS。

本机试用 Server 已配置为 `https://192.168.0.36:7200`，健康入口为 `http://127.0.0.1:7201/healthz`。测试 CA 已通过配对配置加入 iOS 客户端，模拟器已测通。启动、重测与桌面观察命令见 [局域网试用说明](deploy/IOS-LAN-TRIAL.md)。管理员凭据保存在未跟踪的 `deploy/secrets/admin-token`，没有在日志/文档中公布。

2026-09-22 已按要求关闭本项目服务与模拟器，保留配置和数据。完整的启动、重建、日志、断点、验证、关闭和故障排查见 [本地调试文档](deploy/IOS-LAN-TRIAL.md)；关闭命令为 `python3 scripts/stop-local.py`。

## 移动构建（macOS 构建主机）

需要 Xcode、Android SDK/NDK r28+、JDK 17，以及 Rust 目标 `aarch64-apple-ios`、`aarch64-linux-android`、`x86_64-linux-android`、`i686-linux-android`。Android 最低支持 API 25，三种 ABI 的原生库均须按 API 25 重建；示例中的 NDK 路径需替换为本机路径：

```sh
python3 scripts/build-mobile.py ios --webrtc
python3 scripts/build-mobile.py ios --webrtc --simulator
python3 scripts/build-mobile.py android --webrtc --ndk /path/to/android-sdk/ndk/28.2.13676358
python3 scripts/build-mobile.py android --webrtc --android-abi x86_64 --ndk /path/to/android-sdk/ndk/28.2.13676358
python3 scripts/build-mobile.py android --webrtc --android-abi x86 --ndk /path/to/android-sdk/ndk/28.2.13676358
python3 scripts/prepare-bindings.py
python3 scripts/package-ios.py --simulator-target x86_64-apple-ios
./apps/android/gradlew -p apps/android :app:assembleDebug
```

Apple Silicon 构建主机的模拟器目标使用 `aarch64-apple-ios-sim`。打开 `apps/ios/AITerminal.xcodeproj` 运行；Xcode 管理模拟器 entitlements/Keychain。真机运行需设置你自己的签名团队。`build-ios-probe.py` 仅验证静态库链接与绘制，不替代正式 Xcode 应用构建。

Android SDK 通过 `ANDROID_HOME` 或本地 `local.properties` 配置。产物：

- `build/mobile/{aarch64,x86_64,i686}-linux-android/libai_terminal_mobile.so`，各配套 `libc++_shared.so`。
- `build/mobile/aarch64-apple-ios/libai_terminal_mobile.a`。
- `apps/android/app/build/outputs/apk/debug/app-debug.apk`。
- `build/AITerminalCore.xcframework`，真机与当前构建机模拟器静态库。
- `build/xcode/Build/Products/Debug-iphonesimulator/AITerminal.app`，已在 iPhone SE 模拟器验证。

两端使用同一 Rust RemoteTerminal 和状态副本，支持账号登录、设备管理、会话创建/选择、输入、历史与缩放；登录凭据通过 Android Keystore / iOS Keychain 存储。Android 当前使用原生 View 页面，iOS 使用 SwiftUI + UIKit。Debug 构建可用本地 fixture 做渲染验证；正式流程连接真实终端。当前产物是开发包，不能把 Debug 静态库体积当作最终 App 大小。

本机 Xcode 验证命令：

```sh
xcodebuild -project apps/ios/AITerminal.xcodeproj -scheme AITerminal -sdk iphonesimulator -configuration Debug -derivedDataPath build/xcode CODE_SIGN_IDENTITY=- build
```

## 文档与状态

- [移动工作台与 Admin 实现计划](doc/task/0922-mobile-admin/PLAN.md) · [执行记录](doc/task/0922-mobile-admin/TODO.md)
- [账号与输入优化结果](doc/task/0922-account-input/RESULTS.md)
- [Android 真机结果](doc/task/0922-account-input/ANDROID-DEVICE.md) · [常用设备调试说明](deploy/ANDROID-DEVICE.md)
- [已批准的实施计划](doc/task/0921-terminal-architecture/PLAN.md)
- [执行清单和验证证据](doc/task/0921-terminal-architecture/TODO.md)
- [技术调研](doc/task/0921-terminal-architecture/RESEARCH.md)
- [未来 AI 定义](doc/task/0921-terminal-architecture/AI-SPEC.md)
- [环境与兼容性验证交接](doc/task/0921-terminal-architecture/HANDOFF.md)

CI 文件覆盖 macOS/Linux/Windows 自动化及移动原生构建；**添加工作流不等于已经在远程 CI 跑过**。当前已初始化本地 Git 仓库，尚未配置远程或发布。
