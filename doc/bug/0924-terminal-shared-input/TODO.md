# 终端帧与桌面移动端并行输入 TODO

- Status: Completed
- Updated: 2026-09-24

## Checklist

- [x] 定位并修复 git status 中间帧校验失败
- [x] 实现 Desktop/Mobile 各自有序并行输入与只读权限回归
- [x] 移除 Android/iOS 接管输入 UI 并更新测试
- [x] 更新本地调试文档
- [x] 完成 Rust、Android x86 和 iOS 本地部署验证

## Verification evidence

- Rust 全工作区测试与 Clippy 通过；`build/terminal-shared-rust-test.log`、`build/terminal-shared-clippy.log`。
- Android x86/API 25：Debug/Test APK 与 lint 构建通过，16 项 UI 回归通过，真实服务 `MobileWorkflowTest` 两轮通过；第二轮 Desktop CLI 与 Android 同时连接，Desktop 输入标记被 Android 画面观察到，见 `build/terminal-shared-android-parallel-results.json`。
- iPhone 15/iOS 17.5：Debug 构建、真实服务 UI 测试 1 项和工作空间 UI 测试 4 项通过，见 `build/terminal-shared-ios-live-2.xcresult`、`build/terminal-shared-ios-ui.xcresult`。
- 新版 CLI 的隔离真实 Zsh 会话执行 `git status` 后继续显示提示符；原 Agent 两条用户 Shell 仍为 `running`。完整证据与旧进程升级边界见 `RESULTS.md`。
