# 执行清单

- [x] 阅读设计与现有实现，形成已授权计划。
- [x] 建立共享接口合同及三个 worktree 子任务，记录启动信息。
- [x] 实现并验证 Desktop AI 与共享移动桥接。
- [x] 审查并集成 Admin，实现管理 API 和浏览器验证。
- [x] 审查并集成 Android，完成 debug APK 构建和关键检查。
- [x] 审查并集成 iOS，完成模拟器构建和界面检查。
- [x] 完成集成回归、运行入口和使用文档，记录必要的人工验证边界。

## 第二轮执行

- [x] 按最新文字设计固定 AI 输入/监控合同并恢复两端执行器。
- [x] 共享 AI 实际输入、持续观察、取消与控制权/去重测试通过。
- [ ] Android 局部浮窗、实时字号、侧滑/恢复/状态完成并在已连接真机测试。
- [ ] iOS 对应交互完成并用模拟器连接真实测试服务/PTY 验证。
- [ ] 集成两端、回归、更新交付与实际设备验证记录。

## 第二轮证据

- Android 已连接 `dmronjvo9pwsbinf`，用户明确授权本轮真机测试；两端沿用原 subtask/run，不新增重复任务。
- 工作树工具因 macOS `kern.boottime` 的 usec 从 599216 调整至 503837 而误判原进程死亡；只读 ps 确认 PID、启动时间和 TTY 一致。已获自动审批后修复 `/Users/carl/.codex/skills/worktree-tasks/scripts/task_runtime.py` 的 Darwin 启动标识比较，仅忽略微秒，仍校验 PID 身份；未编辑数据库或释放锁。两个原执行器已接收任务。一次只读查询的自动审批曾因审批服务 503 失败，之后只读诊断和必要操作正常获准，当前不构成阻塞。
- 新增端到端测试 `assistant_inputs_once_monitors_real_pty_and_fences_stale_writes` 通过：真实 PTY 输入、重复提交仅一次、观察真实 revision、后续人工输入序列正常、人工输入/失去控制后的旧 AI 写入拒绝、取消后不执行。
- 第二轮核心完整回归：49 项 Rust 测试全部通过，workspace clippy `-D warnings` 通过。Android arm64/x86_64 和 iOS device/Intel simulator 原生库已重新构建并通知执行器。
- 测试服务由协调者启动：`.local/mobile-floating-android/account-fixture.json` 与 `.local/mobile-floating-ios/account-fixture.json`，独立账号/120列PTY/模型桩，密码未输出；验收结束需停止本次两个 fixture 进程并删除相应转发。
- 2026-09-23 用户要求恢复关闭的子任务，Android/iOS 已从原对话恢复并登记运行。iOS 首次握手超时重试成功，映射见 `WORKTREES.md`。
- 真实Android测试已通过主要流程但关闭终端后刷新失败。新增自动断言复现 `close_selected` 后 `sessions()` 返回 `offline`；确认 Desktop Watch 在会话移除后继续发送错误。修复 Close 取消对应订阅并返回 state_sequence，Mobile 清除过期更新；同一加密PTY测试现确认关闭后仍可列出/创建/选择会话，clippy通过。
- 新原生库已重建；旧中断的fixture服务已退出，残留专用Agent已停止，新的独立配置位于 `.local/mobile-floating-android-resumed/account-fixture.json`、`.local/mobile-floating-ios-resumed/account-fixture.json`，已通知两端使用新端口和会话。

## 首轮完成记录

- 2026-09-22：用户已授权计划完成后执行，无需再次 Review。
- 父任务 `20260922-203945-723-mobile-admin`，三个执行器已登记 `running` 且进程存活；详情见 `WORKTREES.md`。
- Desktop AI：3 个管理/上下文单测、1 个真实 HTTP 模型桩测试、移动内核加密通道集成测试通过；同 ID 重试仅一次模型请求，响应期间可输入终端。`cargo +stable clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings` 通过。
- 新 UniFFI 绑定、Android arm64 原生库、iOS device/Intel simulator 原生库及 XCFramework 已生成并通知两端执行器。
- 第一轮 `cargo +stable test --locked --workspace --exclude ai-terminal-bindgen` 全部通过；等待三个客户端/Admin 子任务集成后复验。
- Admin 子任务 completed，源码已集成；5 项账号 + 4 项 Admin 测试通过，独立 Chromium 在 1440x1000、390x844、320x720 验证登录/创建/重置/撤销/XSS/错误/无浏览器存储。协调者检查截图并要求修正桌面顶对齐，复验截图通过。预览 `http://127.0.0.1:64535/admin/`，healthz 为 ok。
- Android：子任务模拟器 13 项测试通过，最终主目录 `assembleDebug assembleDebugAndroidTest lintDebug` 全部通过；APK、截图和测试记录已集成。未操作真机。
- iOS：子任务 Debug/Release x86_64 simulator 构建、4 项 XCUITest、ChatStoreChecks 通过；主目录 Debug 模拟器构建及 ChatStoreChecks 复验通过。稳定横屏键盘和透明层修正后的截图已经协调者检查。
- 最终主目录 `cargo +stable test --locked --workspace --exclude ai-terminal-bindgen`：47 项测试通过；全工作区 clippy `-D warnings` 和 `git diff --check` 通过。首次集成时 rsync 保留的旧 mtime 使某个 Cargo 特性组合复用旧 server 库，Admin 测试返回 404；检查确认源码已有路由、库时间更新于源码，刷新三个 server 源文件 mtime 后重编译，所有测试通过，未改业务行为或清空全局缓存。
- 主目录 APK/测试 APK/lint 构建成功，iOS Debug 构建成功，Server/CLI 开发二进制构建成功。Admin 预览最终 healthz 为 ok。产物与使用说明见 `RESULTS.md`。
