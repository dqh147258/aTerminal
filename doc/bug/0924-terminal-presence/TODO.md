# 在线设备与终端附着状态 TODO

- Status: In progress
- Updated: 2026-09-24

## Checklist

- [x] Android/iOS 只展示在线设备并回归界面
- [x] 实现 Desktop 附着租约、只读状态订阅与重新附着输入
- [x] 允许已结束会话只读查看并优化两端光标
- [ ] 完成 Rust、Android x86 与 iOS 模拟器真实服务验收（Android 真服务未通过，来宾内核 panic）
- [x] 更新本地文档、记录可选颜色边界并提交

## Verification evidence

- Rust 全工作区测试与 Clippy 通过；直接 Ctrl+] 脱离及重新 `--attach` 同一 PTY 的实机 CLI 验证通过。
- Android x86 Nox 上 17 项界面/绘制/状态测试通过；真服务流程被 Android `system_server` watchdog 中断，之后 Nox 内核 panic，SDK x86_64 AVD 也未完成引导。完整边界见 `RESULTS.md`。
- iPhone 15/iOS 17.5 的 5 项 UI 测试、独立新版 Agent 的真实脱离/重附着全流程、旧默认 Agent 的兼容全流程均通过；结果包见 `RESULTS.md`。
- 两条默认 Agent 用户 Shell 保留，最后画面/滚动历史已私有归档；切换默认 Agent 会结束这些 PTY，待明确安排。
