# 本地脚本验证结果

- 日期：2026-09-24
- 环境：macOS x86_64、Android `127.0.0.1:62001`、LAN Server `192.168.0.36:7200`

`scripts/build-artifacts.py` 的 `desktop android` 实际编译成功，包含 Android arm64-v8a、x86_64、x86 Rust 库、Debug APK、测试 APK 和 Lint。`android ios` 的 Rust/Android 步骤成功，iOS XCFramework 生成成功；首次 Xcode App 构建因沙箱禁止访问 CoreSimulator 缓存而失败，获授权在本机重新执行同一 Xcode 命令后 `BUILD SUCCEEDED`。`desktop,android`、`ios server`、`all` dry-run 和非法平台检查通过。

一键启动在真实 Android 设备上完成安装、测试账号自动登录和 Desktop 连接。重复启动后 Desktop ID `GrX3rMlWcY4eUtqn4_snaRD5s9MELfyLnW5w_DoT2zw`、Android ID `hl6IZYk-YzmztUsWPT8pw92ic9rOgbgXDuJ1SDqOulc` 不变，分别只有一台在线；Android 在线状态在超过 15 秒的 Server 判定窗口后保持。`adb reverse --list` 为空，临时密码文件已删除，手机前台为 `MainActivity`。

`cargo test -p ai-terminal-mobile -p ai-terminal --lib --bins` 通过（8 + 3），对应的 Clippy `-D warnings`、`cargo fmt --check`、`git diff --check`、Python 语法检查和本机 PTY 回归通过。`scripts/start-local-agent.sh` 返回规范 Desktop 身份。

Server 镜像构建已验证进入 Docker 的 `cargo +1.94.1 build --locked --release -p ai-terminal-server`，但 crates.io 索引长时间没有进展，已中止；现有 LAN Server 健康运行。`scripts/local-dev-down.py` 的语法与参数入口已检查，未实际执行完整关闭：当前规范 Agent 有运行中的 Shell，关闭会结束其 PTY；待 Shell 工作完成后可按本地调试文档执行。
