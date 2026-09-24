# Android 原生移动工作台

- Status: In progress
- Updated: 2026-09-22

## 目标与范围

在 apps/android 内使用 Kotlin 原生 View 落地登录、终端、抽屉、设置、AI 对话及语音输入。保留 Rust 渲染、真实账号/设备连接和端到端加密；无演示数据或 WebView。第二轮以父任务 FOLLOWUP-CONTRACT.md 为准：局部半透明浮窗、最后会话自动恢复、一次授权输入与有界持续监控。

## 当前状态与证据

MainActivity.kt 已实现 Account/RemoteTerminal 与 Choreographer 增量渲染；PairingStore 使用 Android Keystore。当前 UI 为纵向原型，无 AI、显示偏好及语音。设计 app.css 定义 #121416 背景、#1a1d20 浮层、#a5c4d4 强调色，字号 12-24、不透明度 60-96。

## 方案与执行

沿用父计划的明确授权：用户要求先写计划、无需再次 Review、写完执行。本计划在既有批准范围内直接执行，不新增子任务。

1. 保留 TerminalView 增量缓存，拆分原生工作台 UI 与本地数据模型。
2. 实现登录状态、设备/会话真实操作、抽屉搜索、原生输入与控制权、终端历史、账号管理和持久显示设置。
3. 按合同接入 assistant API，持久化隔离聊天与请求状态；实现原生语音只填草稿及权限/取消状态。
4. 同步允许的绑定和 JNI 产物，构建 debug APK/test APK；执行确定性测试和可用模拟器截图，完成交接。

第二轮明确授权已收到，不重复批准：原地调整设置/聊天为 60%-75% 局部浮窗，保留原始终端列宽、字号即时预览和侧滑菜单；按账号持久化最后设备/会话和已确认状态；在原串行 worker 上复核会话后发送新 JSON，增加允许操作、监控、取消与按 request/event ID 去重。共享原生库和真实 PTY/模型夹具由协调者提供。新增 debug-only 独立测试存储命名空间，真机测试不读取或退出既有账号。

## 验证

构建 assembleDebug、assembleDebugAndroidTest/lint；事件去重、恢复隔离和界面测试。用户已授权 dmronjvo9pwsbinf 真机，遵循 deploy/ANDROID-DEVICE.md：install -r，不卸载/清数据，每次输入先核对前台和焦点。先独立 UI，收到专用夹具后必须验证真实登录/PTY/AI 输入、监控、取消及恢复。线上模型与确定性模型服务明确区分。

执行结果：APK/测试 APK/lint 成功，专用 API33 模拟器 13 项测试通过；竖屏、横屏、键盘和完整面板截图已核对。产物与真实服务验证边界见 HANDOFF.md。

## 风险与回退

AI 绑定由协调者稍后生成。异步结果必须绑定账号/连接世代和 session_id，防止切换后串线；退出立即清空可见状态。后台断开连接，持久请求 ID 支持回到会话后续查。

## 未决问题、歧义与确认

None.
