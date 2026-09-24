# Android 原生移动工作台交接

## 第二轮续作状态（2026-09-23）

以下新增状态覆盖首轮的“全屏面板、仅建议、未测试真机”描述；首轮结果保留作为历史。

已实现局部72%高度半透明设置/聊天浮窗、实时字号预览、原始列宽横滑、侧滑菜单、按账号恢复最后设备/会话及历史状态。AI 按 FOLLOWUP-CONTRACT 直接接收一次输入授权和有界监控，持久化去重 input/output/observation 事件；可取消，不以模型回复声称命令成功。关闭浮窗后主屏继续查询监控任务。

用户已授权 M2012K10C (`dmronjvo9pwsbinf`)；已使用 install -r 保留数据，调试测试使用独立 acceptance-account / acceptance-connection / acceptance-display 命名空间。未读取原账号凭据、未清空原数据、未改变系统锁屏设置。

真机已通过7项存储/事件、5项UI、3项原生渲染测试。协调者专用真实120列PTY和确定性模型的流程通过登录/输入/横滑/浮窗字号、AI_DEVICE_OK/AI_DEVICE_DONE、输入/输出/观察事件、关窗监控、后台自动恢复、取消不发送Ctrl-C、sleep30被真实Ctrl-C中断后恢复输入。证据在 `build/android-followup/real-results.json`、对应测试日志和真机截图。

剩余：专用Shell已真实移除，但原共享Rust订阅继续Watch已删除会话，导致channel离线，关闭后历史步骤超时；协调者已复现并负责修复。客户端现区分已确认关闭与刷新失败，未知关闭显示待确认，不加入自动重连fallback。等待新库及新专用Shell重跑完整尾部，确认关闭后通道仍可查询其他会话、历史显示已关闭、临时账号退出及本次ADB映射清理。本段尚不表示第二轮完成。

## 实现

- Kotlin View 完整登录、真实设备连接、会话搜索/切换/创建/关闭、终端历史、账号改密/撤销/退出。原 TerminalView 单独成文件，保留 Rust 增量状态与 RenderNode 缓存。
- 深灰/浅蓝设计、侧边工作空间、分段 tabs、全屏设置与 AI 面板；标题、消息、设置文字和输入操作区为实色。Lucide 图形 22dp、触控 48dp，来源及 ISC 许可见 `apps/android/NOTICE-LUCIDE.md`，APK 同时包含许可。
- 原生输入、功能键、输入控制权；字号 12-24sp、透明度 60-96%，默认 16/88，持久化及恢复默认。
- 直接调用 `remote.assistant(sessionId, requestJson)`。上下文默认不附带；模型回答只可复制/放入终端草稿，用户另行编辑发送。语音识别只填草稿，有权限拒绝、不可用和取消状态。
- 对话使用服务地址+账号命名空间，设备+会话独立键，私有存储；请求 ID 在发送前持久化。关闭/暂停/切换后的旧回调丢弃，重新打开按 ID 查询，不自动重发不明结果请求。退出清空可见账号/终端/聊天状态，保留对应账号私有离线历史。

## 构建与自动化

工作树：`/Volumes/Code/public-worktree/AITerminal/0922-android-workspace`

从 `apps/android` 执行：

```sh
JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' ANDROID_HOME=/Users/carl/Library/Android/sdk ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug
```

debug APK 和测试 APK 构建成功，lint 无错误。残留警告是 SDK/依赖版本、现有绘制分配、程序化 View 构造和文本国际化等，不扩展本次范围。

产物（相对于工作树）：

- `apps/android/app/build/outputs/apk/debug/app-debug.apk`，包含更新的 arm64-v8a/x86_64 Rust 与 libc++ 库，约 55MB。
- `apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk`，约 945KB。
- `apps/android/app/build/reports/lint-results-debug.html`
- `build/screenshots/android-workspace/instrumentation.txt`：`OK (13 tests)`。

模拟器 API33、360x640dp：WorkspaceStateTest 5 项、WorkspaceUiTest 5 项、DisplayConsistencyTest 3 项全部通过。覆盖服务/账号/设备/会话隔离、历史搜索/草稿/请求状态持久化、UTF-8 请求预算、设置边界、密码隐藏切换、输入控制权守卫、原生键盘、旋转、晚到 AI 回调不得覆盖新回复，以及软件/硬件增量渲染像素一致性。最后仅补充旧 Android 的 fullBackupContent=false，重新构建/lint，不重复运行与备份无关的 UI 测试。

测试运行命令：

```sh
adb -s emulator-5584 shell am instrument -w -e class dev.aiterminal.app.WorkspaceStateTest,dev.aiterminal.app.WorkspaceUiTest,dev.aiterminal.app.DisplayConsistencyTest dev.aiterminal.app.test/androidx.test.runner.AndroidJUnitRunner
```

现有 DeviceAcceptanceTest 已适配抽屉/输入控制的新 UI；因无本次隔离服务账号 fixture、且未授权操作真机，此真实端到端测试未运行。未读写真机，也未复制 .local 或秘密配置。

## 截图

`build/screenshots/android-workspace/` 中已逐张确认的稳定竖屏：

- `login.png`：真实空登录页，360x640dp 下按钮完整显示。
- `drawer.png`：完整 MainActivity 的工作空间抽屉与分段标签，无虚构在线设备。
- `chat-unavailable.png`：点击实际 AI 入口的未登录守卫面板，包含 AI Agent 标题、关闭与登录入口。
- `settings.png`、`terminal.png`：MainActivity 完整层级。终端仅使用已有 debug-only render_fixture 测试快照，不是在线设备或产品演示数据。
- `chat.png`：在 MainActivity 全屏面板容器内的 AssistantPanel 确定性测试对话；不是线上模型结果。
- `chat-keyboard.png`、`chat-keyboard-actions.png`、`terminal-keyboard.png`：原生键盘及小屏操作区域。
- `settings-landscape.png`、`drawer-landscape.png`：横屏布局，页面内容可滚动。

没有为截图向产品添加账号、在线状态或模拟模型回复。完整在线聊天仍需要真实账号、Desktop 和模型配置。

## 环境与手动确认

已有 API34 镜像启动失败。为本任务创建了专用 AVD `aiterminal_workspace_api33`（`/tmp/aiterminal-workspace-api33`），测试后已停止；未删除用户原 AVD。宿主 Intel 12 代导致默认内核在 TPAUSE 指令处 panic，临时启动参数 `-qemu -append clearcpuid=517` 关闭 WAITPKG 后成功。重启命令：

```sh
/Users/carl/Library/Android/sdk/emulator/emulator -avd aiterminal_workspace_api33 -port 5584 -no-snapshot -no-window -no-audio -no-boot-anim -gpu swiftshader_indirect -feature -Vulkan -qemu -append clearcpuid=517
```

用户/协调者后续验证：使用真实服务账号与 Desktop 验证连接/创建/关闭/改密/撤销；配置 Desktop 模型后确认 pending/completed/failed 和可选屏幕上下文；有识别服务的真机验证语音授权、取消、识别结果只填草稿；在目标 Android 15/其他厂商输入法和大字体下确认安全区与键盘。模拟器未验证真实语音/线上模型，不能将组件测试结果当作这些服务的端到端验收。

修改限于 `apps/android/**` 与本任务文档。未修改 Rust、根 README、Cargo.lock 或其他平台；未提交、合并或删除工作树，执行器终端保留。
