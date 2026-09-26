# 移动端输入、横屏与全屏 Agent

2026-09-26，用户追加授权并确认：**刚进入不显示系统键盘，点击 Terminal 要输入时才显示。**

## 最终交互

- 进入终端即可接收实体键盘输入，不需要先开输入模式；冷启动不主动唤起系统键盘，点击 Terminal 才唤起。
- 右侧键盘图标只打开“特殊按键”半透明浮窗。回车、Tab、退格、Esc、方向键、Ctrl-C、粘贴、历史及系统键盘开关均用图标表示，保留可访问标签。执行一个动作后立即关闭浮窗；已有系统键盘保持打开，除非用户选择收起。
- 移除挤占终端下方空间的固定按键工具栏。图标减小，保留约 44–48 dp/pt 点击区域；侧栏和特殊按键背景使用同一不透明度设置。
- 字号范围为 **6–24 sp（Android）/ pt（iOS）**，浮层背景不透明度为 **0–100%**。原来的 16 / 88% 默认值和已保存偏好保留。
- 横屏隐藏系统顶部状态栏、应用顶部栏和底部状态占用，将工作空间/设备入口放到侧边；会话标题、路径、连接状态在工作空间侧栏内可查。旋转不重连、不改变所选会话。
- Agent 占满应用可用区域，保留系统安全边距并随键盘避让。输入与发送并排；高度不足时将范围、历史、设置等收进“Agent 操作”菜单，仍保留输入权限开关。键盘不会遮挡发送按钮。

## 实现要点

Android `MainActivity` 将特殊按键与系统 IME 分离，使用悬浮图标网格，硬件按键继续由 Activity 转交当前终端。避免默认给文本编辑型 Terminal 焦点唤起 IME；点击 Terminal 的既有 InputConnection 路径继续支持中文、组合输入与粘贴。

Android 11+ 使用 WindowInsetsController 隐藏状态栏，以避免 FLAG_FULLSCREEN 关闭 IME resize；布局及全屏断言按系统 Insets 后的可用区域计算。Android `AgentPanel` 根据可用高度压缩导航与输入区。

iOS 同步特殊按键浮窗、较小 SF Symbols、6 pt 与 0–100% 背景不透明度、横屏侧栏和全屏 Agent；Terminal 的硬件焦点与系统键盘状态分开。

## 验证

`emulator-5586` / Android 16，隔离账号及真实临时 PTY：

- 不点击 Terminal/键盘按钮，实体输入可直接到达 Shell；初始系统键盘保持关闭。
- 点击 Terminal 才显示 IME，回车与 Ctrl-C 实际生效，按键浮窗用一次关闭，已有 IME 保留。
- 6 sp、0% / 100% 边界和保存后的 25% 生效，Activity 重建后偏好仍在。
- 横屏无顶部栏，侧栏入口与信息存在，所选会话及 generation 不变。
- Agent 横竖屏填满可用区域，横屏实际点击输入框后键盘出现，发送按钮仍完全可见。
- 主账号与连接偏好前后相同，没有对用户的真实 Shell 输入测试内容。

核心测试 `MobileInputLayoutTest` **1 项通过**；已有 Workspace UI 中登录表单、显示偏好/旋转、中文与 IME 输入、工具按钮持焦点时实体按键分发 **4 项通过**。原始结果与截图：`.local/emulator-5586-input-layout/`、`.local/emulator-5586-workspace-input/instrumentation.log`。截图已复核。另运行生产 Agent 面板的临时模型/PTY 端到端测试 **1 项通过**，覆盖设置、发送、历史及证据回读，报告位于 `.local/emulator-5586-agent-fullscreen/`。

```sh
./apps/android/gradlew -p apps/android \
  :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug --offline
PATH=/Users/carl/Library/Android/sdk/platform-tools:$PATH \
  python3 scripts/test-android-autoconnect.py --serial emulator-5586 \
  --input-layout --output .local/emulator-5586-input-layout
```

Android 构建/lint 与 iOS `build-for-testing` 通过。iOS 本轮未运行设备 UI 测试，编译结果不代表运行验证。没有更改 Rust/FFI，无需重建或重启用户的 Desktop/Server。

自动连接首帧回归 **1 项通过**，实际账号两次独立进程启动恢复 Terminal **2 项通过**。最新本地包已安装到 `emulator-5586`，最终保持原有账号和实际终端连接，截图确认首次进入未弹系统键盘。实际报告：`.local/emulator-5586-input-live/results.json`。
