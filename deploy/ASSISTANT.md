# Desktop Agent 使用与验证

Android/iOS 的 AI 浮窗连接 Desktop 上的持久 Agent，可选择全局或当前终端、发送和追加任务、停止编排、浏览历史及证据。语音入口继续关闭。模型请求和扩展执行都在 Desktop；协调服务器仅转发加密数据。

## 配置与发送

```sh
aTerminal providers add
aTerminal models add
aTerminal models set-default PROFILE --scope global
aTerminal models set-default PROFILE --scope session-default
aTerminal models set-default PROFILE --session SESSION_ID
aTerminal models discover --provider PROVIDER
aTerminal agents send --session SESSION_ID --message '读取最新输出并解释'
aTerminal agents send --message '检查各终端状态' --allow-input
aTerminal agents show --session SESSION_ID
aTerminal agents stop --session SESSION_ID
```

交互向导提供供应商协议、连接、隐藏密钥输入、模型目录搜索/分页、手工模型 ID、能力声明、思考强度和默认绑定。非 TTY 使用 `--file` 或明确 flags；`--json` 不询问输入。App 的“Agent 设置”复用相同修订校验服务，模型与思考强度是独立表单，保存不产生模型推理。目录未声明的能力保持未知，需要按供应商资料确认，不能根据模型名称猜测。

模型支持 OpenAI Responses、OpenAI Chat、Anthropic、Gemini、Azure OpenAI 和 Ollama 协议；LM Studio 等兼容服务选择对应协议和地址。Azure 使用 deployment ID，目录需要显式 `catalog_url`。实际供应商兼容性需用用户配置的测试任务验证；本地协议桩不证明线上工具质量、可用性或缓存命中。

默认根为 `~/.aTerminal`，`--state-dir` 可隔离测试实例。配置保存凭据引用，秘密在 Desktop keyring 或显式文件凭据后端中；手机不回读密钥。一个 Run 固定模型、思考参数、工具目录、Skill 包版本与连接。普通修改在下一 Run 生效，凭据撤销会终止相关调用。

旧 `AI_TERMINAL_AI_*` 不会覆盖新配置。可用 `aTerminal config import-legacy-env` 显式导入，默认只读并保留未知能力，随后编辑模型并绑定默认。旧 Assistant 协议仅在 Desktop 显式设置 `AI_TERMINAL_LEGACY_ASSISTANT=1` 时开放；新 App 使用 Agent v1。

## 执行与停止

每个账号/Desktop 有一个全局 Agent，每个终端最多一个活动 Session Agent。真实用户消息可以启动 Run 或追加到当前 Run；委托沿用同一用户根、时间/调用/读取预算。PTY 状态、子任务回报、历史浏览、配置更新、重连和重启不会独立唤醒模型。

“允许操作”允许本次任务调用有副作用的终端和用户扩展工具。终端写入仍要求 Desktop 附着、当前授权和人工版本检查。人工输入/resize 抢占旧 Agent；取消优先于尚未入队的动作。停止编排不发送 Ctrl-C，不回滚已经写入的字符，也不自动重放结果未知的动作。关闭 App/网络断开不会取消 Desktop 已接受的任务；账号或设备撤权会停止调用。

用户 MCP 与 Skill 脚本拥有 Desktop 用户进程权限，不是 OS 沙箱。只有用户在设置或 CLI 中安装/启用；工具输出、终端文字和 Skill 资源不能创建新的用户授权。

## 读取、图片与记忆

终端读取使用同一 Alacritty 权威状态机的不可变 ReadView。`tail` 可无锚点；`search` 必须有非空内容开始锚点，不找不到就回退底部。重复锚点要求消歧。原文保留空白与 UTF-8，模型摘要附带 Host 生成的原样首尾与搜索锚点。原样首部默认 10 行、尾部默认 20 行，可在 Agent 设置或 `aTerminal config terminal-reading --head-lines 10 --tail-lines 20` 中修改（1–100，下个 Run 生效）。日志底部的动态 TUI 原句由分析标注并经 Host 校验，搜索锚点统一剔除这些行；原文仍保留。没有稳定锚点时明确报错，不拿动态 TUI 的完整末尾行或空锚点继续搜索。

读取先按 UUID 存档，再使用主线同一配置与历史前缀追加分析指令。分析完成前不执行下一批写动作。分析失败/重启保留档案；下一条真实用户消息可恢复分析，不重复读取或重放写入。上下文依次卸载原文、删除旧可选引文、归档引用和有界压缩；原始证据在保留期内仍可回读。

PNG 是固定字体离屏终端网格，标记 `rendered_terminal`，不是 OS 桌面截图。图片与关联文本来自同一 epoch/revision；视觉模型收到图片块，文本模型收到关联文本及引用。CLI 可保存当前画面：

```sh
aTerminal sessions capture SESSION_ID --output terminal.png
aTerminal agents history --session SESSION_ID
aTerminal agents record UUID --session SESSION_ID --part anchors
aTerminal agents record UUID --session SESSION_ID --part body
```

历史每页 50 条，固定首屏水位，用 scope/generation 绑定的不透明游标读取更早页。App 在底部操作时加载，内存最多三页；实时对话与历史浏览分开。原文和图片按需取回。SQLite 缓存拒绝旧 generation 覆盖新缓存；离线时明确显示缓存状态。

手机旧归档逐会话流式导入到独立只读来源，原文件保留。Android 不读取整个旧 SharedPreferences 对话集合；iOS 不解码所有归档到内存。旧本地记录不会上传成为模型历史。

## MCP、Skills 与 Shell 状态

```sh
aTerminal mcp add --file server.json
aTerminal mcp list
aTerminal mcp validate SERVER_ID
aTerminal skills add /absolute/path/to/skill
aTerminal skills show user/SKILL_ID
aTerminal skills disable user/SKILL_ID
aTerminal skills remove user/SKILL_ID
```

内置 Terminal MCP 和五个内置 Skills 由二进制发布，`builtin/` 镜像不能覆盖运行时实现。用户 MCP 支持 stdio / Streamable HTTP，按 Run 惰性连接、固定目录，禁止会话过期后自动重发副作用调用；不接受 sampling。目录/输出/诊断/超时均有界，诊断对注入环境值脱敏。

全局和 Session Agent 均提供只读内置 MCP `wait`，例如 `wait({"duration_ms":1000})`。`duration_ms` 必须明确提供整数 1–30000；缺失、类型错误或越界会报错，不自动截断。它只异步延时并返回实际 `elapsed_ms`，不查询或操作 Terminal，也不表示任务完成；取消与 Run 总时限会中止等待。Terminal 任务可能耗时，可按 `wait` → `get_terminal_state` / `read_terminal` → 未完成继续等待回读的顺序循环，直到取得可靠完成证据、被取消或 Run 预算耗尽。不能根据静默或提示符猜测完成。现有 `skill_action` 的 `builtin/wait-terminal` / `wait` 仍等待 Terminal revision 变化或超时，返回 `changed` / `state`。

用户 Skills 支持 frontmatter、`agents/openai.yaml`、资源分页、显式 `$name`、隐式策略、依赖声明和受管脚本。App 可以从手机文件夹分块上传、编辑用户 SKILL.md、启停或删除。路径逃逸、内置覆盖、版本文件变更被拒绝。脚本使用显式解释器与参数，不自动安装依赖。未引用且超过七天的旧版本在 Desktop 启动时回收，同时保留配置备份引用；不会删除活动 Run 使用的包。

新建终端可显式启用会话内 Shell hooks：

```sh
aTerminal --shell-integration -- /bin/zsh
```

支持 bash/zsh/PowerShell 的独立启动配置，保留用户 rc/profile 和现有钩子，不修改全局 rc。bash 遇到已有 DEBUG trap 时保留它。没有 hook 或应用适配器时，程序任务完成状态仍为 unknown；静默、提示符或前台进程存在不证明应用任务完成。当前 OS cwd 观察在 Unix 校验 PID 启动身份，Windows 不可用字段保持未知。

## 历史维护

```sh
aTerminal history clean --older-than 30d --dry-run
aTerminal history clean --session SESSION_ID --keep-last 10000
aTerminal history retention set --older-than 30d
aTerminal history retention show
aTerminal history retention off
```

选择器互斥，支持 `--older-than Nd`、`--before DATE/RFC3339`、`--keep-last N`。默认对当前账号/Desktop 的各 Agent 分别处理；保留策略可设账号默认或 Session 覆盖。启动后、每小时及容量压力时进行有界维护，不调用模型。清理保护活动引用、整个工具交互单元和防重放账本，并使分页/缓存 generation 失效。复合投影索引可能随来源清理失效，剩余原文仍可按其 UUID 回读。

单份档案最多 4 MiB，SQLite 上限 2 GiB。没有保留策略时不会擅自选择删除期限；存储失败先阻止 AI 动作，人工终端继续可用。报告区分逻辑回收与可复用页，不保证每次清理都缩小数据库文件或擦除离线手机副本。

自动化与平台验证记录见 [任务清单](../doc/task/0925-terminal-agents/TODO.md)，真机与真实供应商验证边界见 [交接说明](../doc/task/0925-terminal-agents/HANDOFF.md)。

## 已实测的 ModelScope 配置

2026-09-26 使用用户授权的 `https://api-inference.modelscope.cn/v1`、OpenAI Chat 协议和 `Qwen/Qwen3.8-27B` 完成真实 SSE 工具循环验收。当前 Desktop 模型别名为 `modelscope-qwen`，全局与 Session 默认均已绑定，思考使用供应商默认。测试和复跑命令见 [真实验收报告](../doc/task/0925-terminal-agents/LIVE-MODELSCOPE.md)。该结果不扩展为其他供应商、视觉或特定思考强度的实测结论。

Terminal 状态消息按实际内容去重：后台采样、用户消息和委托快照共用最近状态比较；仅采样时间、Shell 报告时间或人工输入计数变化不会新增消息。进程身份、前台任务、Shell 阶段/目录/退出码及 Desktop attachment 等变化仍记入历史，A→B→A 保留两次变化。相同状态复用原记录，重启后仍有效；人工输入抢占校验独立保留。旧历史记录不自动删除。
