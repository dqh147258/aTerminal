# 移动端 TAB 补全与输入反白修复

- Status: Completed
- Updated: 2026-09-25

## 目标与范围

修复 Android 模拟器键盘 TAB 插入制表符、移动端输入最后字符或空白反白的问题，验证实际 Shell 与移动画面。Android/iOS 共用输入根因一并修正，保留显式粘贴语义。

## 当前状态与证据

`d8d373d` 只补充硬键盘事件拦截并改变默认光标形状。两端终端直输仍调用 `RemoteTerminal.send_text`，其 `input_kind=1` 由 Agent `encode_input` 包装为 `ESC[200~...ESC[201~`。在独立 `/bin/zsh -f` PTY 重现：粘贴 `t` 输出 `ESC[7mtESC[27m`；粘贴 TAB 输出反白空白，普通输入没有反白。白块是 Shell 的粘贴区域，不是光标绘制。Android `committedText` 未将 TAB 转换为命名按键，且模拟器 `ACTION_MULTIPLE` 文本事件未处理。

## 方案与执行

用户本轮明确要求“请修正”“重新解决并且验证”，作为本次修复与设备验证的执行授权。

1. 共享核心增加 `type_text`，仅接受已提交的可打印文本，经已有原始输入类型发送；保留 `send_text` 粘贴接口和权限/序列控制。无需升级正在运行的 Agent 或重启 Shell。
2. Android/iOS 终端直输使用 `type_text`；输入法 TAB/回车走命名按键；Android 处理模拟器文本事件。
3. 增加请求语义、Android IME/硬键盘事件与实际 Zsh 输入反白/TAB 补全回归。构建原生库、FFI 与 App，在专用会话验证并检查截图。

## 验证

Rust 输入请求验证；Android 输入与像素回归；Android 模拟器通过真实 LAN、独立 Desktop PTY 验证逐字输入无反白、IME TAB/硬键盘 TAB 补全、退格、回车、中文。构建 iOS 并验证可用的模拟器。只操作专用测试会话，记录运行结果与环境边界。

执行结果：两端直输与 TAB 映射完成。Rust 9 项通过；Android 真实 Zsh 输入与补全、最终专项 3 项通过；iOS 构建与键盘专项通过并检查截图。iOS 完整流程的无关抽屉导航失败及 Android API 29 专属测试跳过已记录在 `RESULTS.md`。

## 未决问题、歧义与确认

Review 后补充：系统粘贴需要独立接入 `send_text`；来源不明的 IME 批量文本含 TAB/换行时整体作为粘贴，只有单独控制字符作为按键。Android 提供粘贴按钮、Ctrl-V 和系统粘贴动作；iOS 拦截 UITextField 粘贴并提供终端菜单粘贴。回归覆盖批量文本、CRLF、中文、只读及拒绝入队。

Android `commitText` 不标识来源，单独一个 TAB/换行的剪贴板内容应使用显式粘贴入口，以免与 IME 按键混淆。
