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
aTerminal agents send --session SESSION_ID --message '读取最新输出并解释' --permission-mode ask
aTerminal agents send --message '检查各终端状态' --permission-mode ask
aTerminal agents show --session SESSION_ID
aTerminal agents stop --session SESSION_ID
```

交互向导提供供应商协议、连接、隐藏密钥输入、模型目录搜索/分页、手工模型 ID、能力声明、思考强度和默认绑定。非 TTY 使用 `--file` 或明确 flags；`--json` 不询问输入。App 的“Agent 设置”复用相同修订校验服务，模型与思考强度是独立表单，保存不产生模型推理。目录未声明的能力保持未知，需要按供应商资料确认，不能根据模型名称猜测。

模型支持 OpenAI Responses、OpenAI Chat、Anthropic、Gemini、Azure OpenAI 和 Ollama 协议；LM Studio 等兼容服务选择对应协议和地址。Azure 使用 deployment ID，目录需要显式 `catalog_url`。实际供应商兼容性需用用户配置的测试任务验证；本地协议桩不证明线上工具质量、可用性或缓存命中。

默认根为 `~/.aTerminal`，`--state-dir` 可隔离测试实例。配置保存凭据引用，秘密在 Desktop keyring 或显式文件凭据后端中；手机不回读密钥。一个 Run 固定模型、思考参数、工具目录、Skill 包版本与连接。普通修改在下一 Run 生效，凭据撤销会终止相关调用。

旧 `AI_TERMINAL_AI_*` 不会覆盖新配置。可用 `aTerminal config import-legacy-env` 显式导入，默认只读并保留未知能力，随后编辑模型并绑定默认。旧 Assistant 协议仅在 Desktop 显式设置 `AI_TERMINAL_LEGACY_ASSISTANT=1` 时开放；新 App 使用 Agent v1。

## 执行与停止

全局对话按账号/Desktop 隔离，每个终端最多一个活动 Session Agent。真实用户消息可以启动 Run 或追加到当前 Run；委托沿用同一用户根、时间/调用/读取预算。PTY 状态、子任务回报、历史浏览、配置更新、重连和重启不会独立唤醒模型。

新版请求默认使用 `ask`：内置观察和严格支持的原生只读命令自动执行，其余动作显示具体操作、目标终端、实际 cwd 和参数，由用户选择“授权一次 / 永久授权 / 拒绝”。`read_only` 禁止写动作。旧请求的 `allow_input=false` 保持只读，`allow_input=true` 映射为 ask；旧“允许操作”不能升级成完全授权。旧客户端无法回答审批时，需要新版客户端或 CLI 响应。

“完全授权”是当前 Agent 对话的显式开关，持续到用户手动关闭，覆盖该对话后续 Run 和本次委托链；独立 Session 对话保留自己的设置。开启会释放该来源仍有效的操作审批，问答仍等待真实答复。关闭后下一次动作重新核对权限。模型、终端文字、MCP 输出和 Skill 不能开启它。

一次授权只消费确切 pending/action；重复提交同一答复幂等。永久规则按账号/Desktop、命令/参数、实际 cwd、来源与执行版本精确匹配，并可撤销。未知目录、无法固定完整执行语言、原始键盘/TUI 输入等情况不提供永久授权；程序、目录、配置或脚本版本改变后重新审批。解释器、env wrapper 和复合命令不会仅凭首个程序名沿用规则。长操作必须查看完整脱敏详情；命令、路径和普通参数可读，秘密值隐藏。

完全授权、一次授权和永久规则都保留身份、账号/设备、Session、取消、执行预算与人工版本检查。终端写入还要求 Desktop 附着和控制 fence。人工输入/resize 抢占旧 Agent；取消优先于尚未入队的动作。停止编排不发送 Ctrl-C，不回滚已经写入的字符，也不自动重放结果未知的动作。关闭 App/网络断开不会取消 Desktop 已接受的任务；账号或设备撤权会停止调用。没有新增 OS 或工作目录沙箱，真实 `cd` 后在新 cwd 重新评估授权。

CLI 提供相同管理入口；先读取 revision，再修改当前对话。发生 `permission_revision_conflict` 时重新读取。以下 `REVISION`、pending/rule ID 和 fingerprint 均来自实际响应：

```sh
aTerminal agents permissions --session SESSION_ID
aTerminal agents permissions --session SESSION_ID --permission-mode ask --expected-revision REVISION
aTerminal agents permissions --session SESSION_ID --full-authorization true --expected-revision REVISION
aTerminal agents permissions --session SESSION_ID --full-authorization false --expected-revision REVISION
aTerminal agents pending --session SESSION_ID
aTerminal agents approval-details PENDING_ID --session SESSION_ID
aTerminal agents resolve PENDING_ID --session SESSION_ID --decision once
aTerminal agents resolve PENDING_ID --session SESSION_ID --decision always
aTerminal agents resolve PENDING_ID --session SESSION_ID --decision deny
aTerminal agents resolve PENDING_ID --session SESSION_ID --answer '用户答复'
aTerminal agents rules
aTerminal agents revoke-rule RULE_ID
```

需完整详情的 once/always 还须带 `--details-ack --fingerprint FINGERPRINT`；拒绝始终可用。`--json` 和非 TTY 不隐式询问或批准。用户问答以 `ask_user` 请求，答复作为输入返回，不授予额外操作权限。人类等待持久化在 Desktop，取消可解除等待；整棵共享执行树都因人类等待而不能运行时暂停活跃计时，仍有其他活跃子任务时继续计时。Desktop 重启中断旧 pending，不自动恢复或重放旧动作。

Global Agent 用 `send_agent_message({"session_id":"…","message":"…"})` 异步委托 Session Agent，返回的 `task_id` 是该子 Run 的 ID；同一根任务向仍活动的子 Run 追加消息会复用这个 ID。只读工具 `get_agent_task({"task_id":"…"})` 按指定任务返回 `state`、`done`、`result_text`、`result_record_id` 与 `error`，不受该 Session 后续 Run 影响。`wait_agent_task({"task_id":"…","timeout_ms":30000})` 等待该任务结束或本次等待超时；`timeout_ms` 必须显式提供整数 1–30000。`timed_out=true` 表示任务仍未结束，不会取消子任务，可以继续等待；当前 Run 的取消或共享总时限会中止工具。两种工具仅查询同账号、同 Desktop 的委托任务。

`done=true` 表示 Agent Run 已结束，需检查 `state`（`completed` / `cancelled` / `paused` / `failed` / `orphaned`）和 `error`，不能据此推断终端内应用任务成功。结果正文有界，`result_truncated=true` 时可通过 `read_record` 分页读取 `result_record_id`。返回的答复与完成报告受到当前 Global Run 的保留保护，Run 结束后释放。错误诊断最多 2048 个 UTF-8 字节，超出时 `error_truncated=true`，不会阻止任务终态保存。结果和错误沿用历史保留规则；历史被清理或旧版本未保存结果引用时，`result_available` / `outcome_available` 明确反映可用性。`get_agent_state(session_id)` 仍查询 Session 当前/最近 Run。

Global Agent 还提供 `list_agent_tasks`、`get_agent_tasks`、`wait_agent_tasks` 与 `cancel_agent_task`。批量 ID 数组最多 32 项，`mode=any|all` 等待固定集合中任一或全部 Run 结束；超时不取消。取消精确旧 task 不会停止该 Session 后来启动的新 Run，也不发 Ctrl-C。所有查询按同账号/Desktop 隔离，Session Agent 不提供这些全局编排工具。

用户 MCP 与 Skill 脚本拥有 Desktop 用户进程权限，不是 OS 沙箱。只有用户在设置或 CLI 中安装/启用；工具输出、终端文字和 Skill 资源不能创建新的用户授权。

## 原生读取与命令结果

`inspect_command({"command":"ls -la"})` 将严格支持的读取语法转换为固定绝对程序和字面 argv，在身份复核的实际 Session cwd 原生执行；Global 必须同时传 `session_id`。当前支持正向限制的 pwd、ls、cat、head、tail、wc；未知语法明确拒绝，不能悄悄回退到 Shell。原生读取清空注入环境、stdin 关闭，stdout/stderr 和运行时间有界；结果归档包含退出码及 truncation，来源为 `sidecar_read`。它不改变 PTY 输入或 cwd，也不使用当前 Shell 的 aliases/functions/PATH；Unix cwd 身份无法证明或平台不支持时返回不可用。

`run_command({"command":"完整命令"})` 在原 PTY 提交，沿用风险审批、取消和人工 fence。返回的 `accepted` 与 `command_id` 只表示提交，随后用 `get_command_result` 或 `wait_command({"command_id":"…","timeout_ms":30000})` 收集结果。完成要求 Host 提交与下一条 Shell 序号、完整命令和 prompt 退出码准确对应；输入边界未知、人工/其他 Agent 插入、缺少 hook、序号冲突或证据失效均返回 `unknown`。命令结果按账号/Desktop/Session 持久保存，后续 Run 可查，直到历史保留删除。

Shell 命令结束不代表后台进程或终端应用任务结束。当前没有通用 Codex/TUI 完成适配器，`application_task` 保持 unknown；静默、提示符和前台进程存在都不能代替完成证据。

| 读取/能力工具 | 使用与边界 |
| --- | --- |
| `wait_terminal` | 带 `after_revision` 和 1–30000 的 `timeout_ms`；等待前已经发生的更新立即返回 changed，超时不停止程序；Global 必填 session_id |
| `read_terminal` 的 delta | 带 after_revision 和之前的 view_id；同 revision 返回 unchanged，屏幕/TUI 改写、尺寸变化或过期要求 refetch，不把覆盖的网格拼成追加日志 |
| `search_history` | 查询保留的 Agent 事件/记录文本，可按 Session、类型和时间过滤；返回精确 event/record ID 与有界片段，cursor 绑定过滤、scope、保留 generation 和首屏水位 |
| `get_capabilities` | 返回实际角色/注册工具、视觉模型支持、观察性 Shell hooks、应用适配器、来源对话授权与共享剩余预算；未知能力保持未知 |

历史搜索按有界块遍历完整文本；空页带 cursor 和 `scan_incomplete=true` 时须继续分页，不能据此声称没有匹配。

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

内置 Terminal MCP 和内置 Skills 由二进制发布，`builtin/` 镜像不能覆盖运行时实现。用户 MCP 支持 stdio / Streamable HTTP，按 Run 惰性连接、固定目录，禁止会话过期后自动重发副作用调用；不接受 sampling。目录/输出/诊断/超时均有界，诊断对注入环境值脱敏。

全局和 Session Agent 均提供只读内置 MCP `wait`，例如 `wait({"duration_ms":1000})`。`duration_ms` 必须明确提供整数 1–30000；缺失、类型错误或越界会报错，不自动截断。它只异步延时并返回实际 `elapsed_ms`，不查询或操作 Terminal，也不表示任务完成；取消与 Run 总时限会中止等待。Terminal 任务可能耗时，可按 `wait` → `get_terminal_state` / `read_terminal` → 未完成继续等待回读的顺序循环，直到取得可靠完成证据、被取消或 Run 预算耗尽。不能根据静默或提示符猜测完成。现有 `skill_action` 的 `builtin/wait-terminal` / `wait` 仍等待 Terminal revision 变化或超时，返回 `changed` / `state`。

用户 Skills 支持 frontmatter、`agents/openai.yaml`、资源分页、显式 `$name`、隐式策略、依赖声明和受管脚本。App 可以从手机文件夹分块上传、编辑用户 SKILL.md、启停或删除。路径逃逸、内置覆盖、版本文件变更被拒绝。脚本使用显式解释器与参数，不自动安装依赖。未引用且超过七天的旧版本在 Desktop 启动时回收，同时保留配置备份引用；不会删除活动 Run 使用的包。

新建终端可显式启用会话内 Shell hooks：

```sh
aTerminal --shell-integration -- /bin/zsh
```

支持 bash/zsh/PowerShell 的独立启动配置，保留用户 rc/profile 和现有钩子，不修改全局 rc。bash 遇到已有 DEBUG trap 时保留它，命令关联会标为不可用；zsh 保留原 precmd/preexec，PowerShell 保留原 prompt。只对准确匹配的 Shell 命令提供退出结果；PowerShell 的复杂/动态命令保持 unknown，避免复用上一条 native 退出码。没有 hook 或应用适配器时，程序任务完成状态仍为 unknown；静默、提示符或前台进程存在不证明应用任务完成。当前 OS cwd 观察在 Unix 校验 PID 启动身份，Windows 不可用字段保持未知。

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


## Managed native programs and permanent authorization

`run_program` takes an absolute `program`, literal `args` (at most 64 / 16000 total UTF-8 bytes), and optional UTF-8 `stdin` (at most 16000 bytes; absent/null means EOF). Global calls require `session_id`; Session calls stay bound. Desktop directly executes the program in the Session's OS-observed cwd with a cleared environment. It does not enter the PTY, parse Shell aliases/functions, change the terminal draft or impose a cwd/OS sandbox. Cards show the actual program/arguments and the result uses `source=native_program`.

Risky native calls use the same once/always/deny/full permissions. Permanent rules are available only for a pinned system leaf program, its exact args/stdin, account/Desktop/Session/cwd and actual file hash. For example `/usr/bin/tee` with `args=["-a","/absolute/marker"]` and explicit stdin appends those bytes through a direct native process. Interpreters, wrappers and unknown programs remain once/full only; identified authorization-management CLI/RPC calls remain forbidden to models. Unknown native programs are still executable after explicit once/full approval. Policy v3 makes old rules based on PTY/program-name assumptions ineligible.

`run_command`, `input_text` and `send_keys` remain ordinary PTY interactions with once/full approval and input/manual/cancellation fences. They cannot receive permanent rules, since even an absolute path can be intercepted by Shell functions or aliases. Use `inspect_command` for positive fixed native reads and `run_program` for a managed effectful leaf operation. Native execution returns a durable command ID, real child exit and bounded stdout/stderr; it stops its own process group on cancellation or a 30-second execution limit. Unknown/cancelled outcomes are not replayed, and native process completion does not claim general TUI/application completion.

泛用 Skill 解释器脚本的有效 imports/外部 helper 依赖版本无法完全固定，因此只提供一次/完全授权，不提供永久规则。每次描述和实际脚本启动仍校验注册包 manifest 的所有声明文件实际内容；只改 helper、入口和 manifest 未变也会拒绝旧 snapshot。可靠 `run_program` 原生叶子永久功能保留，不新增 OS 依赖沙箱。
