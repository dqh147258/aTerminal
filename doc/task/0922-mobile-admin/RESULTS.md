# 移动工作台与 Admin 交付

三个 worktree 子任务已完成，代码集成在 `/Volumes/Code/My/AITerminal`。2026-09-24 已分别提交三个工作树和主工作区，并把三个分支合并进 `main`；详见 `doc/task/0924-worktree-sync/PLAN.md`。未发布、未删除工作树或关闭执行器终端。分支和 SessionID 见 `WORKTREES.md`。

## 使用入口

- Admin 预览：<http://127.0.0.1:64535/admin/>。隔离 SQLite，预览账号均为测试数据。
- 预览管理员令牌文件：`/var/folders/85/nscyc5g90sn18qw0_2l1qnt40000gn/T/aiterminal-admin-bcdTqx/admin-token`。令牌值未写入源码、截图或此文档；预览进程信息位于同目录 `preview.json`。正式部署使用自己的管理员令牌和数据库，见 `deploy/ADMIN.md`。
- Android：`apps/android/app/build/outputs/apk/debug/app-debug.apk`，主目录新构建，包含 arm64-v8a/x86_64；测试 APK 位于相邻 `androidTest/debug/`。
- iOS：`apps/ios/AITerminal.xcodeproj`；主目录模拟器 App 为 `build/xcode/Build/Products/Debug-iphonesimulator/AITerminal.app`。当前构建机模拟器架构为 x86_64，真机签名需自己的团队配置。
- AI 配置：`deploy/ASSISTANT.md`，Desktop 设置兼容模型端点、模型名与密钥。模型不自动执行命令，终端上下文默认不发送；用户可主动附带当前画面。

## 能力

Android/iOS 原生工作台按设计实现登录、设备连接、工作空间抽屉、会话搜索与管理、原生终端输入/控制权/历史、持久显示设置、AI 对话与隔离历史、系统语音草稿。修复了切后台遗留忙碌状态、旧聊天响应回写、透明层文字叠读及小屏/横屏键盘遮挡。

Admin 提供概览、用户创建和密码重置、设备及连接搜索/筛选/撤销。复用现有认证和 SQLite，不需要前端容器；浏览器 token 仅保存在内存，改密与撤销使旧凭据/连接失效，保留桌面 Shell。

共享 AI 通道采用有界异步任务和请求 ID 去重，在加密移动通道上查询，模型请求由 Desktop 发出，凭据不经过 Server/Mobile。已用本地模型桩验证 HTTP、错误处理、结果隔离、重复提交与终端输入并行。

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

真实移动账号操作、真机语音/输入法、实际模型供应商、公网网络、发布签名未作为本次自动化通过项；后续场景见 `HANDOFF.md`。本次未使用用户真机或真实模型密钥。
