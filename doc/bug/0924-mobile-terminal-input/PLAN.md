# 移动端终端直输与会话同步

- Status: Completed
- Updated: 2026-09-24

## 目标与范围

让 Android/iOS 的终端画面直接接收软/硬键盘输入，文字到达 Desktop PTY 后由真正的 Shell 回显；回车、Tab、退格、Esc、方向键和 Ctrl-C 走对应终端按键，而不使用独立可见输入框及“发送”按钮。保留“接管输入”的显式控制权与 AI 占位状态。让外部已创建或关闭的 Desktop 会话及时出现在移动端列表中，并保持当前会话画面同步。

## 当前状态与证据

2026-09-24 的本地部署中，`ai-terminal-dev` Server 与 Desktop Agent 在线；`target/debug/ai-terminal --state-dir .local/local-dev/agent --list` 返回两条 `running` 会话，Admin 也显示 Desktop 在线。Android 当前抽屉首次只显示 `05c5caa5`，点击“刷新会话”后立即显示 `673fa9d7`，确认 `remote.sessions()` 能返回新会话，UI 缓存没有随抽屉打开更新。`apps/android/.../MainActivity.kt` 的 `openDrawer()` 和 `apps/ios/AITerminal/WorkspaceScreen.swift` 的 `drawerView` 使用已有 `sessions`，手动刷新才调用 `remote.sessions()`。Android `TerminalView` 与 iOS `TerminalView` 目前只绘制画面；两端分别使用 `inputBox`/`TerminalComposer` 的可见文本框与发送按钮。共享 `RemoteTerminal.sendText`/`sendKey` 已支持实际 PTY 输入和特殊键。

## 方案与执行

用户本轮明确要求修正 Android/iOS 两端，视为执行授权。

1. Android 终端画面取得键盘焦点并提供 InputConnection，将已提交文本直接送往当前受控 PTY；未提交的 IME 组合文本保留在输入法阶段。硬键盘与工具栏的回车、Tab、退格、Esc、方向键、Ctrl-C 映射到 `sendKey`。移除独立可见文本框与发送按钮，保留必要的特殊键工具栏和显式接管控制。
2. iOS 终端画面成为键盘输入目标，用原生输入代理处理输入法提交、退格与回车；终端画面可点击聚焦，特殊键工具栏保留但无独立可见草稿框。共享控制权与错误状态沿用 `TerminalModel`。
3. Android/iOS 在打开工作空间、连接恢复和前台返回时刷新会话列表；抽屉打开期间适度轮询，让外部 Desktop 新建/关闭会话及时反映。刷新只更新列表，不无故抢占或切换用户当前会话。
4. 更新两端 UI/设备验收用例和调试文档；在独立测试 Shell 验证直接输入、Enter、Tab 补全、回显、会话列表新增/关闭与 Desktop/移动端画面一致。使用现有本地 Server，不操作用户正在使用的会话。

执行结果：两端已移除独立可见终端草稿框，终端画面直接承接输入法/硬键盘，保留显式控制开关与特殊键栏；打开抽屉立即刷新并每 3 秒更新。另确认 iOS 旧会话关闭时会被恢复逻辑卡在空终端，已改为选取仍在线的会话。专用 Desktop PTY、Android x86 真机和 iPhone 15 模拟器验收通过，详见 `RESULTS.md`。

## 验证

Android API 25/x86 的 debug APK、测试 APK、lint 通过；16 项状态/界面测试中 15 项通过、API 29 专属项条件跳过，专用 Shell 的端到端测试 1 项通过，含真实 PTY 文字、键盘 Enter、Tab 补全、恢复和会话关闭。iOS x86_64 模拟器 Debug/Release 构建、4 项隔离 UI 测试及 1 项真实本地服务全流程通过，覆盖外部会话新增/关闭和失效旧会话恢复。共享 Rust 协议未修改；新增本地只用于创建独立测试会话的示例已通过编译和格式检查。证据与环境边界见 `RESULTS.md`。

## 风险与回退

IME 的组合文本不能在候选字确认前发送；失去控制或切换会话后必须停止旧输入。两端刷新列表时保留当前会话选择及终端画面，避免频繁网络轮询阻塞主线程。设备测试只操作新建的专用会话，保留现有 Desktop Shell 和 App 数据；所有特殊按键测试以实际 PTY 输出/行为验证，不以按钮点击本身作为成功。

## 未决问题、歧义与确认

None.
