# iOS 移动工作台实现

- Status: Completed
- Updated: 2026-09-23

## 目标与范围

按父计划实现 `apps/ios/**` 原生移动工作台，保留 Rust renderer、加密连接和控制权；不修改 Rust、Android、根 README 或 Cargo.lock，不操作真机，不提交。

## 当前状态与证据

`AITerminalApp.swift` 已有真实账号/设备/会话，`TerminalView.swift` 使用 UIKit 绘制 Rust frame。现有页面没有设计分层、持久设置、AI 与语音。设计位于 `/Volumes/Code/OpenDesignProjects/4c6b6741-85d2-4ded-b4c0-37377166a5c9`；AI 合同读取自父计划同目录 `CONTRACT.md`。当前 worktree 干净；现有绑定暂未包含 assistant。

## 方案与执行

复用父计划中用户授权“先写计划，不再 Review，写完执行”，不重复申请。

1. 按设计实现登录、终端主体、抽屉搜索与设备/会话操作、账号管理和设置浮层。
2. 以独立聊天模型接入 assistant 合同，私有目录按服务/账号/设备/会话隔离；请求不自动重发，显式终端上下文，历史可离线阅读。
3. Speech/AVFoundation 原生识别仅修改草稿，处理取消/权限/不可用；增加工程权限说明。
4. 同步协调者生成的绑定与库，使用独立 DerivedData 构建模拟器，检查关键页面、小屏/旋转/长文本，记录限制。

2026-09-22 第二轮沿用用户直接执行授权，以父目录 `FOLLOWUP-CONTRACT.md` 和最新文字设计覆盖首轮全屏浮层/仅建议行为：

1. 设置/聊天改为局部半透明浮窗，始终留出终端可见区，字号实时预览；终端保留原始列宽横滑，增加边缘侧滑菜单，登录仅保留登录字段。
2. 按服务/账号保存最后设备/会话，登录及前台自动恢复在线目标；维护已查询会话状态，其他设备历史显示待确认，不虚构在线。
3. 接入 allow_input/monitor/cancel、新状态与每请求单调事件游标；关闭浮窗继续当前任务轮询，切会话暂停旧 UI 轮询，恢复按 ID 查询。
4. 先构建/布局/状态测试；协调者更新原生库和独立账号/PTY/确定性模型夹具后，必须在明确 UDID 的模拟器中真实登录并验证 PTY 输入、恢复、AI 输入/监控/取消，不以布局 fixture 代替。

## 验证

Xcode simulator Debug/Release 构建；聊天编码、隔离键、尺寸边界与请求状态的确定性检查；模拟器截图检查登录、抽屉、设置、聊天。Debug fixture 不读取或写入产品账号/聊天数据，不执行命令。真实模型/语音验证留 HANDOFF。

已完成：最终 Debug/Release x86_64 simulator 构建通过；四项 XCUITest 全通过（0 failures），包括稳定横屏键盘的真实坐标断言和后台取消/过期回调；ChatStoreChecks 隔离/持久化/请求边界通过。iPhone SE 和 iPhone 15 截图已检查。验证产物和真实环境边界见 `HANDOFF.md`。

第二轮目标模拟器：iPhone 15 `AC1104EB-5507-4260-8D39-DB2BD68771C7`，小屏回归继续使用 SE `A04FFCA0-1F31-4851-86E6-160D8D6A81C6`。真实服务测试使用专用 Keychain/UserDefaults/聊天目录，不读取既有用户凭据，不卸载/清数据；测试内容仅限协调者批准的独立 PTY。

恢复证据（2026-09-23）：`ios-live-editor.log` 已通过真实登录、120 列 PTY、独立 printf 输出、横滑、字号预览、80/120 列切换、AI_DEVICE_OK / AI_DEVICE_DONE 独立输出及 observation；停在较早 input 标签可视性断言。专用 IntegrationAssistantHistory 核对为 input=1/output=2/observation=2，事件已持久化。修正测试滚动后，等待协调者新库/新夹具复验恢复、取消、sleep 30/Ctrl-C、关闭历史和退出；旧 endpoint 已 ECONNREFUSED，不复用旧 session。

最终完成（2026-09-23）：新独立服务的 `build/ios-live-final-resume.xcresult` 真实全流程1项通过，0 failure/0 skipped，包括明确stopped响应、sleep30/Ctrl-C后独立输出、关闭后历史已关闭及退出；专用Shell已关闭并通知协调者可停服务。`build/ios-floating-final-layout.xcresult`四项小屏回归通过，数据/事件边界检查通过，最终Release构建通过（仅4条原静态库debug-map警告）。真实截图和复现边界见HANDOFF。

## 风险与回退

AI 绑定由协调者提供，开发先隔离桥接点；模型请求不可混到切换后的会话。系统语音权限弹窗会短暂 inactive，后台断连仅在 background 执行。保留现有测试入口及原生 renderer。

## 未决问题、歧义与确认

None.
