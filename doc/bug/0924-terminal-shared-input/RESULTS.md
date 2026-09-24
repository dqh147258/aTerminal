# 终端帧与并行输入结果

后续默认 Agent 已完成切换并补上瞬时坏帧恢复、外部关闭会话的正常退出；当前状态见 [默认 Agent 稳定性结果](../0924-default-agent-stability/RESULTS.md)。本页下方关于旧 Agent 仍运行的描述是当时的验收记录。

`git status` 不再使终端会话报 `invalid or oversized terminal frame`。根因是 Desktop 终端引擎在 Git 输出中把制表符 `\t` 原样写入屏幕单元，而协议严格禁止控制字符；现将这种单元显示为空格，不放宽帧校验。隔离 Zsh PTY 复现时，坏单元位于第 6 行第 0 列，只出现在中间帧，最终截图检查原先无法发现。

Desktop Agent 现在为每个连接维护独立的输入序列、去重窗口和过期标识，按会话 actor 的接收顺序写入同一个 PTY。Android/iOS 选中有写权限的会话后自动可输入，“接管输入”开关和提示已移除；旧版只读配对仍由服务端拒绝写入。移动端 AI 入口继续保持不可交互占位。

| 验证 | 结果 |
| --- | --- |
| Rust 全工作区测试、Clippy | `cargo test --locked --workspace --exclude ai-terminal-bindgen` 与全目标 Clippy 均通过；含真实 Git 中间帧、双客户端顺序/去重、加密通道并行输入和只读权限回归。日志在 `build/terminal-shared-rust-test.log`、`build/terminal-shared-clippy.log`。 |
| Desktop CLI | 在独立新版 Agent 的真实 Zsh 会话输入 `git status`，画面出现 `modified: tracked.txt` 并返回提示符，CLI 保持运行。 |
| Android API 25/x86，真实 LAN | 新版 x86/x86_64/arm64 原生库、Debug APK/Test APK 和 lint 构建通过；16 项 UI/状态测试全通过。`MobileWorkflowTest` 两轮各 1 项通过：登录 `192.168.0.36:7200`、终端直输、`git status` 后继续输入、Tab 路径补全、Ctrl-C、恢复、AI 占位和关闭会话。第二轮 Desktop CLI 保持连接并输入 `DESKTOP_PARALLEL_OK`，Android 在同一画面读到该输出，然后继续输入；结果 `desktop_input_while_mobile_open=true`。路径为 `relay`，`adb reverse --list` 为空。见 `build/terminal-shared-android-parallel.log`、`build/terminal-shared-android-parallel-results.json`、`build/terminal-shared-android-ui.log`。 |
| iPhone 15 / iOS 17.5 模拟器，真实 LAN | 新版设备与 x86_64 模拟器原生库、XCFramework、Debug App 构建通过；真实服务 UI 测试 1 项通过，含 `git status` 后继续输入、系统 Return、Tab 补全、后台恢复、会话列表与画面同步；独立工作空间 UI 回归 4 项通过。见 `build/terminal-shared-ios-live-2.xcresult`、`build/terminal-shared-ios-ui.xcresult`。 |

测试只创建并关闭独立 Shell。原 `.local/local-dev/agent` 中的 `05c5caa525b75ed6` 和 `673fa9d769820939` 仍为 `running`，本地 Server 未停止。正在运行的旧 Agent 不能无损热升级，因为这些 Shell 的 PTY 状态在其进程内；当前新 Agent 以 `Terminal Fix Desktop` 在线，状态目录为 `.local/shared-input-test/agent`，可立即创建新版会话。旧会话继续使用旧 Agent 行为，待不再需要这些 Shell 时才能重启原 Agent。两端同时敲键时字符会按 PTY 收到的顺序交错，测试证明各端独立序列与画面持续同步。
