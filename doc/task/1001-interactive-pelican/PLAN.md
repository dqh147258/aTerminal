# Android Agent / Codex 交互字符动画验收

- Status: Completed
- Updated: 2026-10-01

## 目标与范围

从 Android 16 模拟器的真实 AI Agent 输入框提交用户任务，在 `/Users/carl/Downloads/Temp2026/Temp10/test-1001` 启动 Codex 交互模式，生成并在 Terminal 中播放字符型鹈鹕骑自行车动画。禁止 `codex exec`。保留工具调用、真实终端和动画变化证据。用户后续要求的最近工作目录功能由独立 worktree 子任务实现与验收。

## 当前状态与证据

上次 `doc/task/0930-space-bunny-wait` 验收使用非交互 Codex 与 SVG，不能代表本轮通过。现有 Android workflow harness 只识别 `codex exec`，需要增加明确的交互验收模式。现有 `input_text`、`send_keys`、`read_terminal` 与 `wait` 可用于交互控制，能力审查与真实运行决定是否需要修复。测试目录存在且为空。模拟器启动脚本误报内核日志，随后 status 已确认同一进程、ADB=device、boot_completed=true。

2026-10-01 实测：初次与一次手机恢复均卡 Codex 空 TUI 的 `observation_analysis_pending`。增加 durable 拒绝分析证据后，再次实测抓到 `invalid_analysis_quote` / `unverified_analysis_evidence`：模型将更新提示中 emoji 后 U+200A 改成普通空格。按用户自由扩展阻塞工具授权，实施原文编号行引用并保持严格原文校验。另一次取消为 `manual_input_preempted_agent`；代码确认启用 focus reporting 后窗口焦点通知通过普通 Input 增加 manual_revision，独立子任务复现并修正，真实键入抢占合同保留。

## 方案与执行

用户在本轮明确授权启动模拟器、开展功能测试、自由扩展阻塞工具及 worktree 子任务；后续明确授权实现最近工作目录选择。沿用这份执行授权，不重复询问。

1. 恢复已有 LAN Server/Desktop 测试账号，启动独立附着测试 Terminal，保留其他会话。
2. 并行审查交互工具并适配 Android 交互证据验收，必要改动按明确文件审查与 Git 同步规则集成。
3. 从模拟器正常 Session Agent composer 提交用户原要求，观察交互启动、任务输入、等待与结果。对实际阻塞做有证据的最小修复或 UI 恢复，保存失败历史。
4. 验证 Codex 创建的字符动画，捕获两个以上不同时间的 Terminal 画面，检查进程和字符帧变化，并保存可运行产物说明。
5. 集成最近工作目录功能，运行针对性检查与模拟器 UI 验收；核对目录选择、去重、启动 cwd、失败提示及界面一致性。

完成记录：Codex 实际交互 launch/Enter/banner/task draft/Enter/echo 链已由归档重放验证；生成纯 ASCII `pelican_bike.py` 并在原 Terminal 播放，48 个程序帧全部不同，两个实测 PTY 画面有 4190 像素变化。任务提交后一次分析暂停经手机继续消息恢复，没有重复启动 Codex；源码另处理 TUI 引用范围的空白行，并显式标记正常行动阶段。最近目录真实创建、错误路径、重复去重、取消与身份/原会话保留通过完整 Android UI 测试；IME 按钮边界与隐藏恢复通过。具体失败/成功证据见 RESULTS.md 与最近目录独立报告。

## 验证

Android instrumentation 必须真实使用当前登录身份及正常 Agent UI，记录所用模型、提交消息、实际工具参数/结果与终端原始记录。交互模式必须拒绝非交互 launch，版本查询、草稿和模型声称不能作为启动证据。动画运行要以真实 PTY 画面变化证明，不能仅检查源码含动画循环。源码改动按组件运行必要 Rust/Kotlin 检查，最近目录功能保存单独验证结果。

## 风险与回退

Agent 分析 Codex TUI 可能暂停，失败与恢复分别记录。替换 Desktop binary 前检查活动会话；不得终止用户任务。目标目录仅用于本次任务，不覆盖其他目录内容。

## 未决问题、歧义与确认

None.
