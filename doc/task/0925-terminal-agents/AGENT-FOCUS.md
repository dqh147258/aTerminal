# Agent 输入焦点与状态展示去重

2026-09-26，用户报告输入“发送任务或追加消息”后马上失焦，附图显示大量重复 running，并要求模拟验证 Agent 能力。

已通过生产 AgentPanel + 自动历史刷新复现：`History polling stole draft focus: TextView`。每 1.5 秒历史渲染后的 `ScrollView.fullScroll(FOCUS_DOWN)` 会移动焦点到可选中的消息文本。改用不改变焦点的 `scrollTo`，不通过强行抢回焦点掩盖原因。回归持续三个真实刷新周期，检查草稿内容、选区、焦点及随后 IME 输入。

状态存储层的语义去重已在上一轮完成。移动展示层额外折叠旧历史中同一终端的连续相同状态文本，跨用户消息及分页边界比较；A→B→A 和不同终端状态保留，不删底层历史或改变分页游标。Android/iOS 同步处理，修复不依赖用户关闭现有 Shell 或重启 Desktop。

Android 焦点与状态展示回归 2 项通过。真实 ModelScope 隔离终端能力测试显式运行通过：6 次模型调用，约 113 秒，读取、分析、一次实际输入、证据回读和请求重试去重均通过。报告 `.local/modelscope-focus-live-report.json`；不会向用户真实终端注入测试命令。

移动 Agent 生产面板的模型桩端到端测试通过（设置、读取、分析、发送、回复、历史与证据）。第一次重跑失败在系统截图窗口尚未就绪，测试改为等待窗口所属包确认后复跑通过；没有放宽功能断言。Android assemble/lint 与 iOS build-for-testing 通过；iOS 此轮未运行设备测试。

## 指定 Claude 启动任务实测

用户要求在 `~/Downloads/Temp2026/Temp09/test-0926` 新建 Terminal、启动 Claude 后保持停止。通过真实全局 Agent / ModelScope 提交请求，Agent 创建会话 `08a287e03caff414`，cwd 正确，实际输入 `claude`。记录 `01a0dcfa-c32a-732b-b10e-cf8a04bbbcff` 原文显示 Claude Code 的目录信任确认界面。

不能声称这次全程无辅助：新建会话返回 `desktop_attachment_required`，由本轮助手打开 macOS Terminal 并 attach 此新会话；Agent 随后输入 claude。最后观察分析遇到 `model_timeout` 而暂停，本轮助手根据已有原文发送只收尾、禁止输入的消息，Agent 最终 `completed`。未确认目录信任，未给 Claude 输入任务，未关闭新会话或用户旧 Shell。

实际任务与过程保存在 `.local/claude-launch-0926/`，最终原文证据和状态均已核查。该案例证明创建/输入/回读链路可用，同时记录 Desktop 附着与末尾模型超时的实际限制，不将辅助收尾描述为一次无干预完成。
