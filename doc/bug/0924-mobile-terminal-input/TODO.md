# 移动端终端直输与会话同步 TODO

- Status: Completed
- Updated: 2026-09-24

## Checklist

- [x] 实现 Android 终端直输和特殊按键
- [x] 实现 iOS 终端直输和特殊按键
- [x] 修复两端运行会话列表同步
- [x] 更新相关测试与本地调试说明
- [x] 完成 Android 真设备与 iOS 模拟器的真实终端验收

## Verification evidence

- Android x86 真机 `MobileWorkflowTest` 1 项通过：终端画面直输、回车、真实 Zsh 的 Tab 路径补全、Ctrl-C、恢复及关闭会话；`build/mobile-terminal-android-clean-pty.log`、`build/mobile-terminal-android-clean-results.json`。
- iPhone 15 x86_64 模拟器真实服务 `LiveServiceUITests` 1 项通过，终端画面直输、Tab 补全和 80/120 列切换；`build/mobile-terminal-ios-live-retry.xcresult`。
- Android 抽屉在 Desktop 外部新增、关闭专用会话后均自动更新；截图 `build/mobile-terminal-session-auto-refresh.png`、`build/mobile-terminal-session-auto-remove.png`。
- iOS `build/mobile-terminal-ios-return-retry.xcresult`：真实服务 1 项通过，系统键盘 Return、Tab 补全、外部会话新增/关闭与失效旧会话回退均通过；`build/mobile-terminal-ios-ui.xcresult`：4 项通过；`build/mobile-terminal-ios-release-final.log`：Release 构建成功。
- Android `build/mobile-terminal-android-ui-regression-retry.log`：16 项中 15 项通过、API 29 专属项条件跳过；`build/mobile-terminal-android-ime-final.log`：512 字符提交与中/Emoji 组合输入映射通过；`build/mobile-terminal-android-return.log`：真实键盘 Enter 的端到端测试 1 项通过。
- 本次专用 Shell 与私有测试文件已清理，原有两条 Desktop Shell、本地 Server 与 Android 调试登录保持可用。完整边界和产物位置见 `RESULTS.md`。
