# 移动工作台与 Admin 交付

三个 worktree 子任务已完成，代码集成在 `/Volumes/Code/My/AITerminal`。2026-09-24 已分别提交三个工作树和主工作区，并把三个分支合并进 `main`；详见 `doc/task/0924-worktree-sync/PLAN.md`。未发布、未删除工作树或关闭执行器终端。分支和 SessionID 见 `WORKTREES.md`。

## 使用入口

- Admin 历史预览端口 `64535` 已关闭。需要重新预览时，先构建 `cargo +stable build --locked -p ai-terminal-server`，再运行 `node doc/task/0922-web-admin/start-preview.cjs`；脚本会输出新的本机 URL 和隔离令牌文件路径。正式部署见 `deploy/ADMIN.md`。
- Android：`apps/android/app/build/outputs/apk/debug/app-debug.apk`，包含 arm64-v8a、x86_64 和 x86；测试 APK 位于相邻 `androidTest/debug/`。
- iOS：`apps/ios/AITerminal.xcodeproj`；主目录模拟器 App 为 `build/xcode/Build/Products/Debug-iphonesimulator/AITerminal.app`。当前构建机模拟器架构为 x86_64，真机签名需自己的团队配置。
- AI 协议与后续启用约束见 `deploy/ASSISTANT.md`。当前移动端只有占位浮窗，不提供发送、监控或录音操作。

## 能力

Android/iOS 原生工作台按设计实现登录、设备连接、工作空间抽屉、会话搜索与管理、原生终端输入/控制权/历史、持久显示设置，以及不可交互的 AI 占位浮窗和已有历史查看。AI 对话、监控与语音草稿代码保留，当前入口暂不开放。终端侧已修复切后台遗留忙碌状态、透明层文字叠读及小屏布局问题；先前 AI 相关验证仅为历史证据。

Admin 提供概览、用户创建和密码重置、设备及连接搜索/筛选/撤销。复用现有认证和 SQLite，不需要前端容器；浏览器 token 仅保存在内存，改密与撤销使旧凭据/连接失效，保留桌面 Shell。

共享 AI 通道代码采用有界异步任务和请求 ID 去重；已用本地模型桩验证协议行为。当前移动端不调用此通道，也不代表产品 AI 功能已经开放。

## 验证

| 检查 | 结果 |
| --- | --- |
| Rust workspace tests | 47 项通过，包含 Admin 与加密 AI 通道 |
| Rust clippy / diff 检查 | `-D warnings`、`git diff --check` 通过 |
| Android 构建 | 主目录 debug APK、测试 APK、lintDebug 通过，有已记录的非阻断警告 |
| Android 模拟器 | API33 360x640dp，13 项仪器测试通过，截图检查通过；专用模拟器已停止 |
| iOS 构建 | 主目录 Debug 模拟器构建通过，子任务 Debug/Release 通过 |
| iOS 自动化 | 4 项 XCUITest、ChatStoreChecks 通过；小屏/横屏键盘断言通过 |
| Admin 浏览器 | Chromium 1440x1000、390x844、320x720；表单、撤销、错误、XSS 和无本地令牌存储检查通过 |

Android 证据：`build/screenshots/android-workspace/` 和 `doc/task/0922-android-workspace/HANDOFF.md`。
iOS 证据：`build/ios-workspace-screenshots/`、`build/ios-workspace-acceptance.xcresult` 和 `doc/task/0922-ios-workspace/HANDOFF.md`。
Admin 证据：`doc/task/0922-web-admin/` 的截图、浏览器结果和 `HANDOFF.md`。

本页验证表记录 2026-09-22/23 的阶段性结果。后续真实本地部署和 x86 设备验收另见 `doc/task/0924-local-deployment-test/PLAN.md`；公网网络、发布签名和实际模型供应商仍不在当前验收范围。移动端语音与 AI 交互尚未开放，后续场景见 `HANDOFF.md`。
