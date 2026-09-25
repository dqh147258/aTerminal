# 移动端 TAB 补全与输入反白修复 TODO

- Status: Completed
- Updated: 2026-09-25

## Checklist

- [x] 区分键盘输入与粘贴并接入两端
- [x] 覆盖 IME/模拟器 TAB 路径与输入语义回归
- [x] 构建安装 Android 并验证真实 Shell 补全和输入画面
- [x] 构建验证 iOS 并记录结果
- [x] Review 修正：显式粘贴接入两端、含控制字符的 IME 批量文本整体粘贴，并补充输入回归

## 验证证据

共享核心 9 项测试与 Clippy 通过；Android 构建/lint、界面与实际 PTY 验收通过，最后 APK 的两个输入测试和真实会话测试共 3 项通过。iOS 构建与键盘专项 1 项通过。两端截图已检查、临时会话与凭据已清理。命令、日志和验证边界见 `RESULTS.md`。
