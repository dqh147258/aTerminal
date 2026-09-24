# 在线设备与终端附着状态

- Status: Completed
- Updated: 2026-09-24

## 目标与范围

Android/iOS 账号设备列表只显示在线设备；Desktop CLI 从会话脱离后，手机可查看画面与历史但不得输入，重新 `--attach` 后恢复输入。Shell 真正退出时保留最后画面和历史只读。改善手机光标的清晰度和输入时可见性。用户提出的宿主 Terminal 主题继承属于可选项，只在能保持现有终端颜色协议的前提下处理。

## 当前状态与证据

截图显示 `账号与设备` 面板把多个离线 iPhone 记录全部绘制；`MainActivity.accountPanel` 和 iOS `WorkspaceScreen.devicesPanel` 都直接遍历全量设备。Agent 当前只有各客户端独立 `Control`，CLI `Attachment::drop` 发送 `Detach` 后 PTY 仍在运行，但移动端输入流仍能写入。`RemoteTerminal.select` 在 Shell 退出时因 `Acquire` 错误无法打开只读最后画面。Android/iOS 光标目前以半透明块覆盖文字，且水平滚动区域不随活动光标定位。Desktop 终端引擎已发送 24 位前景/背景色，并支持 ANSI 调色板与 OSC 改色；Mac 宿主 Terminal 的配置不在独立 Agent 的终端协议中。

## 方案与执行

用户明确要求修正，本轮据此实施。按已向用户说明的解释，将 CLI 正常脱离与 Shell 真正退出都视为移动端只读；前者 `--attach` 同一 PTY 即恢复，后者仅保留历史。

1. 两端设备面板仅绘制 `online` 设备，离线记录仍保留在账号模型，不擅自撤销设备。
2. 新增本机 CLI 专用附着操作与带超时的 Desktop 活跃标记；最后一个 Desktop 附着离开后，Agent 拒绝远程终端输入/Resize。会话可读操作保持可用。附着状态经订阅送达手机，并按递增版本避免旧回复覆盖新状态。
3. Android/iOS 允许选择已退出会话查看最后画面和历史，清晰显示 Desktop 已离开/会话已结束/只读配对；桌面重新附着后恢复输入。
4. 光标采用清晰块/线/下划线样式，输入时确保光标进入可视区域；用户手动滚动后停止自动跟随。保留原 24 位色与现有样式解析，不把宿主 Terminal 主题硬编码到 App。
5. 构建与自动化测试后，使用独立 Agent 和真实本地 Server、Android x86/iOS 模拟器验证；不在测试前关闭默认 Agent 的用户 Shell。

执行完成：新版功能在独立在线 Agent `Local Desktop 新版` 可用，旧默认 Agent 的两条用户 Shell 保留，客户端已做旧 Agent 兼容。Rust/iOS 真服务验收通过；Android 构建和 UI 测试通过，真服务流程因 Nox 内核 panic 和 SDK AVD `offline` 未能完成，恢复设备后的步骤见 `HANDOFF.md`。宿主 Terminal 主题继承按用户允许的范围暂缓，详见 `RESULTS.md`。

## 验证

Rust 终端协议/Agent/移动共享核心测试，含脱离只读、重新附着、Shell 退出与读历史；Android x86 APK/lint、UI 与真实局域网流程；iOS 模拟器构建、UI 与真实服务流程。测试 Server 使用 `192.168.0.36`，不使用 `adb reverse`。检查光标截图和两端在线设备列表。

## 风险与回退

默认 Agent 仍是先前构建，不能在不结束其 PTY 的情况下热升级；先在独立 Agent 验证，再检查默认会话进程状态。Desktop 非正常终止由本机活跃租约兜底，最多约 15 秒转为只读。若用户其实希望 Ctrl+] 脱离后手机仍能输入，应按其澄清改为仅在 Shell/Agent 结束时只读。

## 未决问题、歧义与确认

None. 已提出可选澄清；在回复前按上一节所述的 Ctrl+] 脱离规则推进。
