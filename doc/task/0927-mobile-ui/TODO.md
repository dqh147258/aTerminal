# Android 原型 UI 落地 TODO

- Status: Completed
- Updated: 2026-09-27

## Checklist

- [x] 全屏设置、独立配置页面及统一侧栏
- [x] 聊天作用域、历史、语音和紧凑输入
- [x] 图片上传、持久记录及真实视觉模型内容
- [x] 构建、关键回归及模拟器验证

- [x] 截图反馈：保留工具调用前说明、紧凑摘要与图标详情；UI、运行时和真实证据回归通过。

## Verification evidence

- `:app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug` 通过。
- Rust：Agent runtime 26 + 模型边界 5 + Desktop 22 + 新增只读权限 1 项通过。
- `artifacts/mobile-ui/final-instrumentation.log`：4 tests 通过；含 180dp 受限高度检查。
- `artifacts/mobile-ui/font-1.3/instrumentation.log`：130% 字体 2 tests 通过。
- `scripts/test-android-agent.py --serial emulator-5586 --output artifacts/mobile-ui/agent-e2e`：1 test 通过，结果含真实进程 cwd、图像 HTTP 内容与二进制记录。
- 登录 UI 与 HEAD 逐字对比一致；`git diff --check` 通过。
- 详细结果及设备确认说明分别见 RESULTS.md 与 HANDOFF.md。
