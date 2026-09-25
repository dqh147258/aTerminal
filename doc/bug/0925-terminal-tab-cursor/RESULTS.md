# TAB 补全与输入反白修复结果

移动端直输误用了粘贴 API：Agent 会在每个字符外加 bracketed paste 标记，Zsh 因而反白最后粘贴区域；TAB 通过输入法文本提交时也被当作粘贴内容插入空白。独立 Zsh PTY 已复现相同输出（`ESC[7m` 反白），因此更换绘制的光标形状不能解决根因。

共享核心新增 `type_text`，将已提交的可打印 UTF-8 字符通过已有原始输入类型发送，复用输入权限、序列和队列控制；显式 `send_text` 保留粘贴语义。Android/iOS 直输接入新 API。Android 的输入法、硬键盘字符及 `ACTION_MULTIPLE` 文本事件统一处理 TAB/回车；iOS 输入代理也将这些字符映射为命名按键。未更改光标绘制、Shell 配置和 Agent。

## 验证

| 检查 | 结果与证据 |
| --- | --- |
| 共享 Rust 核心 | `cargo +stable test --locked -p ai-terminal-mobile --lib`：9 项通过；验证 UTF-8 原始输入、TAB 命名按键、粘贴兼容、控制字符拒绝、输入序列与只读拒绝。`build/tab-cursor-rust.log`。 |
| 静态检查 | `cargo +stable clippy --locked -p ai-terminal-mobile --all-targets -- -D warnings`、`git diff --check` 通过。 |
| Android 构建 | arm64-v8a、x86_64、x86 原生库、FFI、Debug APK/Test APK、lint 通过；最终 APK 已安装在 `127.0.0.1:62001`。`build/tab-cursor-build.log`、`build/tab-cursor-android-build-final.log`。 |
| Android API 25/x86 回归 | 界面/输入、增量绘制与真实服务合计 12 项，11 项通过、API 29 RenderNode 专属项条件跳过。最终字符事件调整后，两个输入测试和真实服务测试共 3 项再次通过。`build/tab-cursor-android-final.log`、`build/tab-cursor-android-latest.log`。 |
| Android 真实 Zsh | 两次专用会话覆盖 80/120 列。逐字输入 `git stat`，逐帧确认输入字符背景色没有反白；退格后 `commitText("\t")` 显示 `stash`/`status` 候选；硬键盘 TAB 完成真实文件路径并由回车执行。中文/Emoji、Ctrl-C、后台恢复和关闭专用会话通过。最终 `passed=true`、路径 `relay`。`build/tab-cursor-android-latest-results.json`。 |
| iOS 构建与输入专项 | 设备/模拟器原生库、FFI、Debug 模拟器 App 构建通过。iPhone 15 / iOS 17.5 的 `testKeyboardInAttachedFixture` 通过：输入 `git stat`、退格、键盘 TAB 候选、Return 执行 `printf`。`build/tab-cursor-ios-build.log`、`build/tab-cursor-ios-keyboard.xcresult`。 |

两端截图均已打开核对：文本右侧只有细竖线光标，没有覆盖最后字符或大片空白的反白区域。

- Android：`build/tab-cursor-android-typing.png`、`build/tab-cursor-android-tab.png`。
- iOS：`build/tab-cursor-ios-typing-no-paste-highlight.png`、`build/tab-cursor-ios-keyboard-tab-completion.png`。

## 验证边界与环境恢复

iOS 原完整流程在会话抽屉显示时失败：抽屉控件位于屏幕左侧之外，无法点击。此项属于导航问题，未将完整流程记为通过。本次改用已登录且带唯一标记的专用会话运行输入专项，发送任何按键前先确认该标记；本次输入问题已单独验证。Android 硬件行缓存测试要求 API 29，现有 API 25 设备条件跳过；本次未更改绘制代码。

仅创建/操作本轮两个专用 Shell，均已关闭；原会话 `d2d7ffd2cad658f3` 仍为 running。未重启 Server 或 Desktop Agent。临时凭据文件已删除，两端恢复普通工作区。当前修复兼容现有 Agent，只需更新 App。

## Review 后的粘贴修正与复验

系统粘贴与 IME 批量提交原先共用逐字输入路径，批量文本内的 TAB/换行可能被拆成补全/执行键。现增加独立粘贴回调：Android 键盘栏“粘贴”、Ctrl-V、系统粘贴动作及 iOS UITextField/终端菜单粘贴均调用 `send_text(..., false)`。来源不明的 IME 提交含 TAB/换行时整体作为一次粘贴，单独 TAB/回车仍为命名按键；CRLF 不会重复执行回车，拒绝粘贴也不会继续发送其后的控制键。

Android 的 `commitText` 不提供来源标记，因此单独一个 TAB/换行的剪贴板内容需使用显式粘贴入口。普通可打印 IME 文本仍使用原始键入 API。

- Rust：mobile-core 9 项、protocol 4 项通过；Clippy、格式检查及 `git diff --check` 通过。
- Android：Debug APK、测试 APK、lint 通过，日志 `build/paste-review-android-build.log`。在用户连接的 `127.0.0.1:62001` 上，IME/硬键盘、批量粘贴与只读拦截专项共 3 项通过，日志 `build/paste-review-android-input.log`。
- iOS：完整模拟器 App 编译链接通过，日志 `build/paste-review-ios-build.log`。新增独立 UIKit 输入探针（`python3 scripts/build-ios-probe.py --simulator --input-checks`），在 iPhone 15 / iOS 17.5 上验证文本提交、独立按键、原样粘贴、入队拒绝和只读拦截通过，日志 `build/paste-review-ios-input.log`。探针不连接远端会话，运行后已删除测试 App 并关闭本轮启动的 iOS 模拟器。
- 本轮未重跑真实 Shell 完整流程。Android SDK 临时 AVD 启动遇到 `delay_halt_tpause` invalid opcode / kernel panic，已关闭并删除 `aiterminal_paste_review_api33`；后续仅使用用户已连接设备，禁用规则记录在根目录 `AGENTS.md`。
