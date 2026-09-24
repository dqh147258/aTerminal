# 终端帧与桌面移动端并行输入

- Status: Completed
- Updated: 2026-09-24

## 目标与范围

修复 `git status` 触发 `invalid or oversized terminal frame` 导致 Desktop CLI 退出的问题。移除 Android/iOS 的“接管输入”操作，让有写权限的 Desktop 和 Mobile 始终可以向同一会话输入；旧版只读配对仍不能写入。AI 移动端入口继续保持占位。

## 当前状态与证据

用户运行 Desktop CLI 后输入 `git status`，CLI 报 `session error: invalid or oversized terminal frame`，并出现旧的“接管”提示。隔离 Zsh PTY 复现时，`Engine::snapshot()` 的第 6 行第 0 列包含 `text="\t"`；`Snapshot::validate()` 禁止单元文本含控制字符。`--snapshot` 只验证最终画面，因此未捕获中间帧。现有 `Control` 仅有一个 owner，移动端切换 owner 会让 Desktop CLI 的输入报控制权转移。旧版只读配对由 `remote_bridge::authorize` 在服务端限制。

## 方案与执行

用户已明确要求修正，并授权本地部署测试。本次在继续运行的独立测试会话实施，不停止原有两条用户 Shell。

1. 在 Desktop 终端引擎输出屏幕单元时把控制字符规范化为空格，保留协议对无效帧的严格校验。增加中间帧与真实 `git status` 的回归。
2. 将 Agent 的单一 owner 改成每个客户端独立的输入序列与去重窗口，由同一会话 actor 顺序写入 PTY。Detach 仅结束自己的输入流；读写权限、账号隔离、会话 epoch 与重试校验继续生效。
3. Android/iOS 打开可写会话时自动注册输入，移除“接管输入”开关及提示；只读配对自动进入只读画面。Desktop CLI 持续可输入，不因移动端连接而退出。
4. 更新测试和本地调试文档，重建 Android x86 与 iOS 模拟器 App，在真实本地 Server 上使用专用 Shell 验证两端输入、`git status`、Tab/Return 与画面同步。

执行已完成。根因、两端行为、真实部署测试与原运行中 Agent 的升级边界见 `RESULTS.md`。

## 验证

Rust 单元和 Agent/加密通道集成测试、格式及 Clippy；Android x86 设备与 iOS 模拟器 Debug 构建及本地局域网会话测试。测试账号与 Server 使用既有 `192.168.0.36` 部署，不用 `adb reverse`。验证只操作新建会话；当前 Agent 的用户 Shell 不重启。

## 风险与回退

多个设备可能交错发送按键，命令由 PTY 实际接收顺序决定；每个来源保持自身顺序与去重。旧 Agent 正在运行的 Shell 属于进程内存，不能无损热升级；新二进制验收使用独立 Agent，原 Agent 不因测试而停止。

## 未决问题、歧义与确认

None.
