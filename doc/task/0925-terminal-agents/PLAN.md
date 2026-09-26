# Desktop 双层 Agent 与可回读终端记忆

- Status: Completed
- Updated: 2026-09-26

## 目标与范围

在 Desktop 引入 Rig Agent Loop、全局 Agent 与 Session Agent，复用现有加密桥接，让 Android/iOS App 能发送任务、追加消息、查看状态、停止编排和回看证据。实现终端状态/程序观察、增量文本读取、字符串与按键输入、终端画面 PNG、UUID 原文回读，以及即时摘要和分层上下文压缩。

本计划将用户本轮需求作为新的产品边界：允许受控的通用终端输入，替代旧 `AI-SPEC.md` 中“仅操作外部 AI TUI”的设想；旧方案不构成当前能力限制。保留现有账号、控制权、人工抢占、Desktop attachment 和只读设备校验。停止编排不等于 Ctrl-C，关闭终端不等于删除记忆。

采用默认：一个账号在一个 Desktop 上有一个全局 Agent，每个 Terminal Session 最多一个活动 Session Agent；Agent 会话、Terminal Session、一次执行 Run 使用不同 ID。截图指 Desktop 权威终端网格的离屏渲染，不依赖手机在线或捕获宿主桌面窗口。范围不含语音、远程 OS 桌面截图、任意外部终端接管或 OS 级沙箱。

“附带所有历史”定义为该 Agent 当前完整有效上下文：任务约束、累积摘要、近期对话、证据引用和本次原文；压缩前是完整历史，压缩后以摘要和可回读引用承接较早历史。不在每次分析时重新展开整个原文档案，否则会抵消压缩并最终超过窗口。原始档案保存在 Desktop，压缩只改变模型上下文投影；用户配置的历史保留策略可以独立清除过期档案。

本轮补充约束：分析使用与主线一致的历史前缀和模型请求配置，仅在末尾追加分析最后一条 Terminal 记录的指令；PTY 状态自动入历史但不触发模型，用户消息前捕获最新状态，仅实际状态变化时插入；只有真实用户消息能发起新的 AI 工作。Desktop 提供历史清理命令和持久保留策略；App Session 历史每页 50 条，滑动到底部才继续加载更早记录。

新增产品范围：统一资源/动作式 CLI；应用管理的文件默认放在 `~/.aTerminal`；Terminal 常用能力做成内置 MCP、低频流程做成内置 Skills，两者不允许用户配置修改/卸载/覆盖。用户可添加 JSON MCP 和 Codex 格式 Skills；常用模型供应商、模型参数与默认绑定同时支持 CLI 和 Android/iOS 配置。配置合同见 [EXTENSIONS.md](EXTENSIONS.md)，内容锚点读取合同见 [TERMINAL-READING.md](TERMINAL-READING.md)；两份文件与本计划共同评审和执行。

## 移动端登录持久化追加（2026-09-26，用户已授权执行）

保留 Android Keystore / iOS Keychain 保存会话与刷新令牌的现有设计，不保存密码。已确认两端登出都等网络调用返回才清除本地凭据，存在关闭进程后恢复旧身份的窗口。改为立即清除并以账号代际保护旧请求写回与 UI 回调；断网启动保留已保存身份。Android 用隔离账号验证登录、进程终止重启、离线恢复、网络注销未返回时退出重启及重新登录；iOS 构建验证对应实现。已完成：Android 六阶段跨进程测试及原有自动连接回归通过，两端构建通过，详见 [MOBILE-LOGIN.md](MOBILE-LOGIN.md)。

## 构建默认登录地址追加（2026-09-26，用户已授权）

Android/iOS 支持构建参数注入默认 Server；登录页仅展示地址及小字修改入口，手动保存地址优先且持久化。本地包内置 `https://192.168.0.36:7200` 与已有公共开发 CA，复用 `.local/local-dev/account.json` 登录用户指定的 `emulator-5586`。不重建或停止既有 Server/Desktop。验证地址折叠、编辑保存、跨进程恢复，以及正常登录 UI 使用内置地址完成本地登录。均已完成：Android 六阶段回归及实际本地登录/恢复通过，App 保持登录；两端构建通过且 iOS 产物配置回读正确。详见 [LOGIN-SERVER.md](LOGIN-SERVER.md)。

## 自动连接与首帧进入首页（2026-09-26，用户追加授权）

现场已确认一个在线 Desktop 和一个运行中的 Terminal；App 经“连接中 / 打开会话”最终进入该终端。确认的流程问题：登录身份恢复即展示空首页，设备连接和首帧选择随后才执行；初次无设备后心跳不刷新设备，心跳也缺少排队去重。两端增加连接准备状态，在首帧或明确无设备/无会话/失败结果后才展示首页；未连接时定期发现在线 Desktop，已连接但无会话时发现新会话。保留单在线 Desktop 自动选择与多设备按已有记录恢复的规则，不擅自随机连接多个 Desktop。自动化记录首页首次绘制状态，覆盖恢复登录和 Desktop 后上线；设备验证仅使用已授权 emulator-5586，不操作用户 Shell。已完成三种自动连接回归、六阶段登录回归与真实工作空间两次冷启动；另确认并通过保留数据重启修复模拟器自身 TLS 转发异常。真实首帧就绪为 1894 / 1751 ms，详见 [AUTO-CONNECT.md](AUTO-CONNECT.md)。

## 移动端输入与布局追加（用户已授权）

用户进一步确认首次进入不弹系统键盘，点击 Terminal 才弹出；实体键盘默认可输入，系统键盘与特殊按键分离；右侧键盘入口打开图标式半透明浮窗，选择一个动作立即收起。缩小图标但保留可点击区域；字号 6–24，浮层背景不透明度 0–100%。横屏隐藏系统顶部状态栏和应用顶部栏，将会话、连接信息与操作移至侧栏；旋转保持连接。Agent 占满可用应用区域。两端同步实现，Android 用隔离 PTY 验证默认输入、单次特殊键、旋转、字号/透明度持久化和 Agent 全屏；iOS 构建验证。已完成，并额外修复横屏 Agent 输入法遮挡，验证发送按钮在 IME 上方；实际工作空间已恢复。详见 [MOBILE-INPUT-LAYOUT.md](MOBILE-INPUT-LAYOUT.md)。

## Agent 焦点与重复状态展示修正（2026-09-26，用户追加授权，已完成）

确认 Android `fullScroll(FOCUS_DOWN)` 在每次历史刷新后把草稿焦点转至消息 TextView，改为仅 scrollTo 更新坐标；多轮刷新/选区/IME 回归通过。两端展示层对已有历史中的同一终端相同状态文本折叠，保持原文及真实转换，解决旧后台仍产生状态或旧档案已有重复记录的场景。ModelScope 实测与移动面板验收通过；指定目录 Claude 启动任务已执行并保留信任确认页，附着辅助和一次末尾模型超时均详见 [AGENT-FOCUS.md](AGENT-FOCUS.md)。

## 计划制定时的状态与证据

- 初始工作区干净；当前 HEAD `c1158fd`。根 `AGENTS.md` 为空。以上来自 `git status --short`、`git log -4 --oneline` 和文件读取。
- `crates/desktop-agent/src/assistant.rs::Assistant::call`：内存任务、请求去重、取消、最多 4 个活动任务；不是持久 Agent Runtime。`crates/remote/src/assistant.rs::answer`：非流式请求，最多一次 `terminal_input`。
- `crates/desktop-agent/src/service.rs::session_loop`：每个 PTY 有自己的串行 actor；现有快照最多缓存 16 帧。`AssistantInput` 校验 control epoch 和人工输入序号，但没有持久的逐工具动作去重账本。
- `crates/terminal-engine/src/lib.rs::Engine`：唯一 Alacritty 状态机；10,000 行 scrollback；`history(offset, limit)` 最多返回 200 行、不包含当前 live grid，offset 随滚动变化。`revision` 是状态版本，不是稳定文本行号；`Snapshot` 有文字、颜色、样式、光标、alternate screen、尺寸 epoch。
- `crates/desktop-agent/src/pty.rs::Session`：管理 PTY 子进程和异步写队列；`exit_status` 是被直接启动进程的退出状态。当前没有 Shell 命令完成事件或前台程序信息。依赖 `portable-pty 0.9.0` 已提供 `Child::process_id()`；Unix `process_group_leader()` 实现使用 `tcgetpgrp`。
- `crates/protocol/src/local.rs::SessionInfo` 仅有 Session ID、初始 cwd、退出状态和控制信息。`service.rs::encode_input` 已按真实终端模式编码粘贴和部分 named keys。`crates/desktop-cli/src/render.rs` 输出 ANSI，不是位图渲染器。
- `crates/mobile-core/src/remote.rs::assistant` 要求非空 session ID，写入绑定当前选中终端；当前不能表达全局 Agent。Android `MainActivity.kt::openChat` 和 iOS `ChatPanel.swift` 当前为不可交互占位；旧聊天实现仍留在仓库。
- Android `WorkspaceStore.kt::ChatStore` 把整段 `Conversation.messages` 序列化为 SharedPreferences 字符串，列表读取会解码全部历史；iOS `ChatStore.swift::load` 读取并解码所有归档 JSON。需要改为有索引的分页存储，不能只给全量内存列表加 UI 分页。`desktop-cli/src/main.rs::Args` 已有 `--history` 终端回滚读取参数，新的 `history` 管理子命令需保持其兼容。
- 当前 `service.rs::default_state_dir` 使用 OS 临时目录；`account.rs::Vault` 的 keyring 身份由目录 hash 派生，文件凭据另存在 XDG/APPDATA/HOME 配置路径。迁移到 `~/.aTerminal` 必须考虑活跃旧实例、登录态引用和设备身份，不能仅替换路径字符串。
- 当前 CLI `account.rs::Management` 使用 `auth`、`devices` 子命令，其他终端管理仍是根 flags；当前模型仅从环境读取 base_url/model/key。尚无 MCP Client/Server、Skills Loader、Provider 注册表或 App 配置服务，本轮都是新增范围。
- 前轮核查的正式版本：Rig `v0.42.0`（2026-08-17）有多轮/流式工具循环、Hooks、可序列化运行状态，但明确存在破坏性升级；不把主分支功能或内部序列化格式作为本项目持久协议。尚未完成本仓库依赖/模型兼容性编译验证。

参考：[Rig 发布](https://github.com/0xPlaygrounds/rig/releases/tag/v0.42.0)、[该版本 Agent 文档](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-agent/README.md)、[OpenAI Prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)（本轮已读取正文：渲染后的前缀匹配、tools/schema 等配置会影响前缀、压缩从首个改动处影响复用）、现有 `deploy/ASSISTANT.md`、`doc/task/0921-terminal-architecture/AI-SPEC.md`。其他供应商按实际能力适配，不套用 OpenAI 的缓存保证。

扩展资料：[官方 Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk) 已核查 tools/resources 和 stdio/Streamable HTTP 支持，具体正式版本待编译适配锁定；[Codex Build skills](https://learn.chatgpt.com/docs/build-skills) 已读取正文，确认 SKILL.md、可选资源、渐进加载、`.agents/skills` 发现与 `agents/openai.yaml` 元数据。格式兼容不等于复制 Codex 专有宿主能力。

用户提供的 Codex 源码 `/Volumes/MacData/Data/DownloadCode/codex` 已只读核查，HEAD 为 `4981037e99a322ee9cf29bc8730cb64d263fa00b`。具体文件、符号、测试依据与采用/不采用的边界见 [CODEX-REFERENCE.md](CODEX-REFERENCE.md)。参考其生命周期、历史投影、MCP/Skills/模型目录设计；继续以 Rig 为模型层，不依赖整个 Codex Runtime，不把源码参考当作实现或执行批准。

## 方案与执行

用户于 2026-09-25 明确授权“按照计划执行”。按第 10 节顺序实施；依赖与合同验证先行，执行和验证证据记录在 TODO.md。运行时与移动端已接通，实施及本机自动化验证已完成，具体结果见 [TODO.md](TODO.md)，未覆盖的平台与设备验证边界见 [HANDOFF.md](HANDOFF.md)。

### 1. 组件与运行边界

新增 `crates/agent-runtime`，承载 AgentHost、Rig 适配器、MCP/Skills 运行与工具合同、事件/记忆存储、上下文投影和压缩策略。它通过窄的 `TerminalBackend` 接口访问 Desktop，不依赖 PTY 或移动 UI。Desktop 实现接口并保留唯一 Terminal Broker 和 ConfigService；现有 terminal-engine 负责权威文本和快照，PNG 渲染放在独立模块/小 crate，仅 Desktop 引用。

采用源码参考中的请求绑定设计：RunSnapshot 持有模型/思考强度、工具目录与实际 handler/连接、Skill 包版本的不可变引用，不能只存一个版本号后执行时又读最新全局配置。消息使用 ContextEntry 包装，稳定 ID、origin、root_user_message_id 和 artifact refs 与供应商 role 分开；模型/终端文字不能自行获得可信 origin。权限撤销仍在实际执行时重新检查。

AgentHost 是确定性的 Tokio 服务，不是模型：维护 Agent 注册表、按 Agent 有界 mailbox、Run 状态、取消和事件发布。不同 Session 可并发；同一 Agent 只允许一个活动决策循环，同一终端写操作串行。全局 Agent 通过工具管理 Session、创建/暂停 Session Agent、投递任务、读取状态；长委托返回 `task_id`，进度异步回报，不长时间占用父工具调用。

**模型调用触发规则：** 仅认证的真实用户消息可创建根 Run。该 Run 内的工具循环、读取后分析、必要的 LLM 压缩及显式委托的子 Agent 调用都关联同一个 `root_user_message_id` 并共享预算，属于处理该请求的后续步骤。PTY 输出/状态、后台计时器、Agent 进度/完成回报、App 浏览历史、清理和重连都只记录/展示，不新建或重启 Run，也不单独发起模型请求。根任务结束或取消后，排队的旧事件不能再次唤醒模型；重启只恢复状态和待处理信息，后续模型工作等用户再次发送消息。

用户追加消息进入 mailbox，在模型/工具边界消费；停止和人工抢占走独立高优先级取消通道。人工抢占作废旧写能力，恢复需重新授权和观察。全局 Agent 可以在当前用户请求内显式等待子任务结果后继续，但任务结束后的被动回报不唤醒它。为 Run 设置时间、模型轮次、token/费用与回读预算。

**自动状态历史：** 收集进程启动/退出、前台作业、cwd、控制权、Session 生命周期等语义变化，生成 `source=pty_status` 的应用历史消息；它是观察，不是用户指令。相同语义状态去重，高频状态合并为有界变化摘要，不对每个字节或 revision 写一条消息。记录持久化不调用模型；大段输出仍按需读取，状态事件只含状态摘要与最新 revision。

每次接受用户消息时，先向对应 Session actor 捕获状态快照，在同一历史事务中与最近一次状态事件比较：状态变化时追加 `[pty_status_snapshot, user_message]`，未变化时只追加用户消息并复用已有状态引用。用户 2026-09-26 追加要求替代原先“即使相同也插入”的策略。采样时间/revision 表达捕获边界，后续变化另记事件，不假装 PTY 与数据库原子同步。状态不可读取时插入明确的 unavailable，而不是陈旧的 running。全局消息附带有界会话状态概览与可回读引用。内部分析指令有不同 source，即便供应商适配需要映射到 user role，也不通过真实用户入口、不得触发新 Run 或再次插入状态。

在途模型请求持有不可变上下文快照；新状态先入事件库，在合法消息边界追加到上下文尾部，不改写已发送消息或打断 tool-call/result 配对。状态记录的积累不触发 LLM 压缩，必要压缩等下次用户 Run 的发送前执行。

Rig 仅负责模型适配、流式输出和单 Run 的推理/工具循环；项目持有工具授权、执行顺序、持久事件和上下文。禁止框架自动并行同 Session 的写工具或自动重试有副作用工具；读取产生新原文后进入第 5 节的分析屏障，再允许后续写入。依赖验证先锁正式版；若该版本无法表达此控制边界，可用它的低层模型接口驱动项目状态机，不依赖未核实的 Hook 行为。

### 2. 工具合同

下表是后端能力合同，不表示全部作为顶层模型工具常驻。常用读写/状态/回读经内置 MCP 暴露，截图、Session 生命周期等低频操作经内置 Skill 按需调用相同后端；具体分组与固定扩展入口见 [EXTENSIONS.md](EXTENSIONS.md)。

| 工具 | 行为与核心结果 |
| --- | --- |
| `list_sessions` / `get_terminal_state` | ID、epoch/revision、尺寸、控制/连接状态、初始与观测 cwd、程序集合、完成状态、来源和能力位 |
| `create_session` / `close_session` | 结构化 cwd/程序参数；关闭明确表示终止该会话进程，使用统一写授权 |
| `start_session_agent` / `send_agent_message` / `get_agent_state` / `stop_agent` | 绑定目标、异步委托、追加消息、任务状态；停止仅取消编排 |
| `read_terminal` | `mode=tail|search|screen`，max_lines、内容起止锚点、可选 view_id；tail 可无锚点，search 必须有非空开始锚点；底部向上读取，返回 UUID、正文、默认首 10 / 尾 20 行原样展示证据与去 TUI 搜索锚点（行数可配置）、边界匹配/停止原因 |
| `wait_terminal_change` | 仅活动用户 Run 内显式调用的有界等待；结果作为该工具返回继续当前循环，无后台监控唤醒 |
| `input_text` | 字符串和 submit 分开，按 bracketed paste 模式编码，多行无法安全粘贴时明确拒绝 |
| `send_keys` | 结构化按键/修饰符/有限重复次数；复用模式感知编码，不把原始转义串交给模型 |
| `capture_terminal` | 返回 screenshot UUID、PNG、session epoch/revision、尺寸和渲染元信息；图像按模型视觉能力发送 |
| `read_record` | 按 UUID 和 part=anchors/body/summary 读取锚点、原文、摘要，或截图/归档索引；有界分页使用不透明令牌，返回同一 UUID |

Session Agent 的工具在服务端绑定 session 和账号，不允许模型靠参数切换 Session；全局 Agent 通过 Broker 管理授权范围内的 Session。UUID 不是权限凭证。工具执行上下文中的 owner/device/control token 由 Host 注入，不能由模型提供或扩大。账号切换、设备权限撤销和 Session 关闭使相应活动能力失效；后台 Agent 的控制租约独立于 App 连接，但仍受 Desktop attachment 规则和人工抢占约束。

所有变更动作有 host action ID 和持久 payload hash。相同 ID、相同参数返回原状态；相同 ID、不同参数拒绝。`accepted` 仅表示已入写队列，不冒充命令已运行；可另记 writer 的 `written/failed` 回执。崩溃窗口中无法确认的动作置为 `unknown`，不自动重发；新授权动作需在重新观察后产生。写账本提交先于入队，存储失败不执行 AI 写操作。

### 3. 文本增量与程序状态

**内容锚点读取。** 模型使用 `read_terminal(mode=tail|search|screen, max_lines=200, start_before?, stop_before?, view_id?)`。tail 从底部取最后 N 行，允许完全无锚点，也可带可选 stop；search 从中间向上查找，必须显式提供非空 start，stop 可选；缺失/全空白/失效的开始锚点不能用 view_id 或上次位置替代，也不能自动退回 tail。扫描向上，返回文字仍按从上到下排列。start/stop 是关键原始行的内容块，可直接传文本或引用已分析记录的 head/tail；两个匹配块都不包含在返回正文中。先取 200 行后，可用这段的 head 作为新 start，最多再向上取 1000 行，用较早记录的 tail 提前截止。终端行号不是跨读取定位依据，详细参数、边界和空白规则见 [TERMINAL-READING.md](TERMINAL-READING.md)。

一次新读取从现有 Alacritty 权威状态捕获同 revision 的有界不可变 ReadView，包含正常缓冲的 scrollback + live grid；alternate screen 不混入后台 Shell 历史。继续向上读取优先复用同一 view_id，内部位置仅在该视图中有效，用于切片和消歧，不要求模型持有行号。扩展引擎的原子文本视图读取和必要版本元数据，直接服务按需读取；不要求持久稳定逻辑行号或完整逐行变化日志。

跨视图锚点只能在选定源内进行内容匹配。重复匹配要返回候选/歧义；start 找不到不擅自从底部重读；stop 未在有界窗口内找到时返回明确未命中与停止原因，可用新 head 继续找。清屏、scrollback 擦除、全屏重绘和 resize 不代表旧内容仍在当前终端；原 view 过期后不能假恢复，只能回读已存 UUID。所有读取都有输出行数、字节、扫描与视图缓存限额，达到限额明确 partial/gap/expired；不保证捕获读取之间的所有瞬间输出。PTY 变化仅记录状态，不因制作视图或匹配失败自动调用模型。

**固定首尾证据。** Host 在本次区间内默认从顶部提取最多 10 条、底部最多 20 条非空原始内容行作为原样 head/tail（分别可配置），并生成排除已识别 TUI 的搜索锚点，同时记录空白跨度。max_lines 默认计非空内容行，空白另计；连续空白仅在模型投影中折叠，原始档案保持原样。全空白、不足配置行数、超长行截断都有明确标记，不拿空字符串当锚点。分析结果强制携带这些程序提取的锚点，LLM 只写摘要和关键事实，不能改写定位原文。

**程序与完成。** 状态分三层：`session_process`（直接启动的 Shell/程序）、`foreground_job`（可能为 pipeline 的多个进程）、`application_task`（TUI 内部一轮任务）。保留 PID + 进程启动身份，防 PID 复用；返回 `running/exited/waiting/unknown`、可空 exit_code、observed_at、evidence_source，未知绝不折算为完成。

Unix 使用 PTY 前台进程组配合安全进程查询；Windows 使用可得进程树信息并标注其不能等价证明前台任务。会话级 opt-in Shell integration 支持 zsh/bash/PowerShell 的命令开始、结束、exit code 和 cwd，保持用户现有 hooks，不改全局 rc 文件；仅在新建/明确启用的受管 Shell 注入，既有未接入会话诚实返回能力缺失。集成事件是状态证据，不是授权；普通终端输出可伪造标记。无结构化适配器的 TUI 内部任务完成只能标记推测/unknown，不用安静几秒、标题或提示符作为确定完成。

### 4. UUID 原始记录与存储

使用 Desktop 本地 SQLite（`rusqlite`、WAL、单写入队列、事务；不引入外部数据库）。采用 uuid crate 生成 UUIDv7；一个 UUID 对应一块不可变观察记录，不是每个字符一个 UUID。文本原文是本次捕获的终端字符表示，保留换行/空白和视图内精确切片，不把不可见 ANSI 控制流作为模型文本；对模型呈现空白折叠的投影并声明区别。

记录包括 owner/device/session/epoch、UUID、种类、view/revision、内部切片、首尾锚点与空白跨度、时间、内容 hash、长度、截断/gap、payload。PNG 与文本统一作为可回读 artifact；首版使用有大小限制的 SQLite BLOB，减少外部文件与数据库之间的提交不一致。记录使用不透明分页令牌和有界切片回读；内部 byte/line offset 仅定位不可变档案，不当作 live Terminal 行号。单次读取超限拆页，不静默截掉未送达部分。相同 request ID 重试复用 record UUID；hash 用于校验/物理去重，不能把两个时间点的相同内容视为同一次事件。

分开保存不可变事件、原始记录、分析结果和可重建的上下文投影。原文与首尾锚点落盘后才能把 UUID/读取引用交付给模型或 App；摘要成功并校验后事务提交新的投影版本。崩溃后能发现 pending 分析；失败不删除原文、不发布空摘要、不把未分析记录标为处理完成。

参考 Codex 的历史分层，另存 retained facts（任务约束、未决动作、最新状态），不依赖压缩后的对话推断这些事实。持久事件 sequence、真实用户输入 revision、投影压缩 generation 分开；投影检查点带 covered_event_seq。恢复只加载最近有效检查点和后续事件，原文按 UUID 懒读；不为了恢复模型上下文展开全部终端档案。模型可见消息/工具结果 ID 首次生成即持久保存，重新投影不随机生成。

上下文压缩本身不删除原始档案；磁盘保留策略由第 8 节独立执行。未配置策略时不擅自按天删除；配置后后台自动清理。只 pin 活动 Run/在途读取真正需要的记录，不因旧摘要仍引用它就永久豁免清理。达到硬限额先按既有策略回收；仍无法释放时停止新 AI 观察/写动作并报告 `storage_full`，人工终端不受影响。被清理的 UUID 返回明确的 `record_expired`，保留的摘要标明相关原文不可用。数据库/WAL 限制文件权限、模型密钥不写入档案；不宣称仅靠权限就有磁盘加密。

### 5. 每次读取后的分析与压缩

每次新读取（或 UUID 原文回读）形成以下顺序：

`活动用户 Run 读取固定边界 → 原文与引用提交 → 保持主线历史及配置 + 末尾分析指令 → 分析响应 → 校验 → 摘要/证据提交 → 替换最近原文 → 继续同一 Run`

**与主线共用请求构造器。** 固定相同 provider/model、system/developer 消息、工具定义及顺序、temperature/top_p、推理配置、输出上限、response_format、tool_choice 和供应商缓存配置（使用时的稳定 cache key/断点策略）。不新建独立 summarizer system prompt，不换模型、不调低温度、不临时切换 JSON schema 或移除 tools。对截至本次读取完成的主线历史保持内容、role、消息/内容块顺序完全一致，唯一追加是末尾应用指令：“分析最后一条 Terminal 记录（UUID=…），保留关键原句并总结；首尾原始锚点由系统保留，本次只返回分析结果，不调用工具”。主线已有静态指令统一说明该内部阶段及来源规则。

尾部指令约定结果字段 `summary`、`key_quotes[{record_id,quote_id?,text}]`、`facts[{claim,evidence,certainty}]`、`open_questions`、`observed_status`，由本地解析和校验，不为分析单独改 provider 输出 schema。这是可审阅的观察结果，不保存模型内部思维过程。工具 schema 保持不变，但 Host 在分析阶段禁止实际工具执行；若模型仍发起调用，按原协议结算拒绝结果并有界纠正，绝不执行写入。Host 将确定性 head/tail 附入 ObservationDigest；逐条验证引文确实来自对应原文并分配档案内 quote_id、引用 UUID 可访问；完成状态不能提升底层证据等级。终端输出始终作为不可信观察数据，不能成为 system 指令或新授权。

**缓存边界。** 保持静态前缀及历史编码稳定，状态/分析指令只追加在尾部，不往开头放当前时间、动态状态或随机 ID。分析前固定所用历史版本，不借机重排/重压较早消息；超过硬预算时先完成必要压缩再确定新的共同前缀。分析完成后只替换最近原文所在的消息单元；旧历史集中到达到阈值时再压缩，避免反复扰动前缀。历史替换从第一个变化位置起可能失去缓存，不能同时承诺“即时删原文”和“整个前缀永远不变”；相同温度等配置遵从用户要求，但缓存命中也取决于供应商规则、精确前缀、保留时间等。记录实际 input/cached token 与缓存诊断（如提供），不凭估算宣称命中。

原文在下一次分析请求里仅出现一次，避免工具结果和提示中各复制一遍。“马上分析”发生在本次读取提交后、下一次操作前；首版同一 Agent 的终端读取/UUID 回读顺序执行，每次尾部指令都只分析最后一条读取记录。内部指令和真实模型分析响应也进入该 Agent 的历史与账本，不丢弃后重新伪造一套对话。分析失败时仅在当前用户 Run 内有界重试；仍失败暂停并保留 pending 原文，后台状态变化或重启不重试模型。App 查看原文不触发分析。

提交后的模型投影示例：

```text
工具结果：{ record_id: "…", session_id: "…", lines: 80, revision: 912 }
观察摘要：测试已结束，报告 2 项失败；进程退出码来源为受管命令事件。
首部展示：本次记录最上方默认 10 条非空原始行（可配置，不足则全部）
尾部展示：本次记录最下方默认 20 条非空原始行（可配置，不足则全部）
搜索锚点：从同一原文排除已标注 TUI 后由 Host 提取，不能直接使用动态 TUI 的完整底部行
关键原句：[record_id=…, quote_id=…] “…原始错误文本…”
待处理：定位失败原因；需要完整日志时调用 read_record。
```

实际存档不改写；仅生成新的模型消息投影。保留 assistant tool_call 与 tool_result 的配对 ID 和供应商要求的完整消息单元，不直接从 Rig history 中删除几条字符串；有签名/推理依赖的供应商消息块按合法整体保留或归档。首版由客户端显式构造每轮完整输入，不复用供应商隐式会话历史或 `previous_response_id` 绕过压缩；压缩边界建立合法的新请求上下文。分析摘要以应用上下文/结构化工具结果表示，保留来源，不伪装成用户消息。分析提交定位到捕获时的目标记录/投影版本，保留此后追加的用户消息和状态后缀，不用旧快照整体覆盖历史；正常状态追加不应导致无限重试。缺失工具结果按执行账本生成稳定的 failed/cancelled/unknown 结果，不机械补 aborted 或重放 PTY 输入。

### 6. 分层自动压缩与回读

模型窗口预算按 provider/model 显式配置，优先使用可用 tokenizer 和 provider usage 校准；估算有保守余量，不能把字符数当精确 token。预算含 system、工具 schema、文字/图像输入和预留输出；普通推理和读取后的分析请求均在发送前硬检查，先压缩旧上下文，为新原文保留空间；单块新原文仍过大时使用有界分页逐块分析。首版可配置默认：有效输入预算 70% 触发清理，目标降到 50%；若低收益则升级下一层，超预算不发送。

1. **读取后压缩：** 原文成功分析后立即替换为 UUID + 元数据，保留摘要、默认首 10 / 尾 20 条非空原样证据及去 TUI 搜索锚点（可配置）及有限关键原句；锚点由 Host 附加，不要求 LLM 重复生成；图像卸载为 screenshot UUID。
2. **确定性压缩：** 按时间移除旧普通引文、回读展开文本、截图展开内容与重复引用，保留摘要和 UUID；正在扩展的读取链及最近记录的 head/tail 为受保护定位字段，不随普通引文删除；不调用 LLM。当前尚未分析记录/未完成工具配对/活跃任务约束固定保留。回读内容只在需要的分析/决策窗口临时驻留，之后可再次卸载，不重复生成内容档案或无限摘要。
3. **引用目录压缩：** 大量 UUID 本身也会占满窗口。将旧引用集合及其不可改写的 head/tail 存成分层归档索引（也有 UUID），上下文保留入口和已有主题/时间/Session 标签；`read_record` 可分页展开索引并最终到达保留期内的原文，已清理来源明确返回过期。索引层不调用 LLM，压缩本身不切断来源关系。
4. **LLM 压缩：** 确定性清理无足够收益才压缩较早的对话/观察摘要，保留任务、约束、已做动作、未完成事项和证据引用。新摘要保存 covered event 范围、source/index UUID 和版本，核验引用集合/覆盖范围后原子替换；失败保留旧版本，有界重试后暂停。大型待压缩历史分成预算内消息单元处理，不把超限上下文直接发给压缩模型。

证据摘要允许多代压缩，但原始记录不发生有损改写。LLM 可能遗漏事实，因此硬性用户约束、执行账本和当前状态由确定性存储保留，不能只依赖摘要。回读设每轮字节/次数预算，必要引用短期 pin 并可到期释放；无法在预算内保留必要信息时明确暂停，禁止递归压缩/回读死循环。全局 Agent 不自动展开所有 Session 的原文。LLM 压缩只可发生于用户已发起的 Run，不因状态事件、定时清理或历史浏览而单独请求模型；其请求也共用主线配置，能在预算内保留相同前缀时只追加尾部压缩指令。

### 7. 截图、桥接与 App

PNG 从一次固定 epoch/revision 的 `Snapshot` 离屏绘制，文字、配色、宽字符、组合字符、样式、光标和 alternate screen 来源一致；使用 Rust 字形布局/栅格库和 PNG 编码，字体许可与回退在依赖验证时确认。输出声明 `rendered_terminal`；不承诺与宿主 Terminal 字体逐像素一致，也不包含引擎本就不支持的图形协议。文字记录和截图需要关联时从同一 actor 捕获边界产生。

视觉模型接收图片内容块；文本模型仍可生成/展示截图并读取关联文本，但不声称模型看过图片。缩放/图片字节有界，保留原始截图 UUID。

增加版本化 Agent RPC/事件：`scope=global|session`、agent/run/request/action/event ID；App 按 event cursor 补读，Server 仍只转发密文。工具细节和完整档案保留 Desktop，图片/原文在 App 按需分页/分块取回，遵守现有消息上限。可靠消息源是持久事件流，token delta 可临时推送并在完成后用正式消息归并；慢客户端用游标追赶，不能拖住 PTY。

Android/iOS 开放现有 AI 浮窗，增加全局/当前 Session 入口、发送/追加、停止、Agent 状态、证据原文/截图查看。沿用原生 UI 和现有账号/设备隔离；业务界面主要显示摘要和证据，不把 UUID、压缩阈值作为普通聊天内容。语音入口继续关闭。旧 App/旧 Desktop 通过能力协商保持人工终端可用，不对未知版本尝试 AI 写入。

**Session 历史每页 50 条。** 独立的历史浏览视图按最新在前、较早在后排序：首次只取最新 50 条，滚动到底部才加载更早的下一页，不自动排空所有页；实时对话视图与历史翻页状态分开。接口 `list_history(scope, before_cursor, limit=50)` 返回 items、next_cursor、has_more、history_generation 和 snapshot watermark，首版服务端将单页上限设为 50。历史条目计数规则与第 8 节一致，原文/截图按需另取，不把完整 artifact 塞进每条列表数据。

使用 scope 内单调 history sequence 的 keyset 查询（`sequence < before`），不用会因新增/删除漂移的 offset。不透明分页游标绑定 owner/device/session、列表种类、generation 与边界，服务端验证，不能在不同 Session/数据列表复用；它与 Terminal 内容锚点是不同接口。一次翻页链固定首屏高水位；最新消息走独立事件通道，去重按稳定 ID，用户看旧页时保持可见条目锚点、不自动跳回最新。单 Session 同时最多一个加载请求；失败允许重试同游标，切 Session 后丢弃旧响应，has_more=false 后停止请求。在途清理导致 generation 变化时明确重置当前翻页链，不混合前后快照或假装历史完整。

Android/iOS 的本地历史缓存同时改为按账号/设备/Session/sequence 索引的分页存储（优先复用 mobile-core 的统一 SQLite 实现），列表只读元数据，不能先读全量 JSON 再切 50 条。内存保留有界页窗口，离开可见区后逐页释放并能再次读取。旧归档一次性逐文件/逐会话迁移，单个大 JSON 用流式导入，避免首次打开全部加载；迁移失败保留旧文件且不丢记录。新 Desktop 历史为权威，旧仅存手机的归档保留为独立只读来源，不自动伪造成模型历史。与 Desktop 恢复连接时先核对 history_generation 并清除过期缓存；离线无法收到删除通知时明确显示缓存状态，不承诺远程立即擦除离线副本。

### 8. Desktop 历史清理与保留策略

新增本地管理命令，与原 `--history <session>` 共存。以下为拟实现接口，不代表现在已可执行：

```sh
aTerminal history clean --older-than 30d
aTerminal history clean --before 2026-09-01
aTerminal history clean --keep-last 10000
aTerminal history clean --session SESSION_ID --keep-last 10000 --dry-run
aTerminal history retention set --older-than 30d
aTerminal history retention set --keep-last 10000
aTerminal history retention show
aTerminal history retention off
```

三个选择器互斥，必须指定一个；支持 `--session` 限定单个 Session/Agent 历史。默认作用域为当前账号在此 Desktop 的历史，`--keep-last N` 对每个 Session（全局 Agent 视为独立 scope）分别保留最后 N 条，不让高输出 Session 挤掉其他 Session。N 必须大于 0。相对天数以执行时刻减 N×24h 计算；日期解释为 Desktop 本地时区该日 00:00 之前，也支持带时区的 RFC3339 时间；内部统一 UTC，并在结果/预览中打印实际截止时刻、作用域和选择器。

“一条记录”指可独立展示的 history item：用户消息、助手消息、PTY 状态消息，或包含工具调用/结果/关联分析的完整交互单元；token delta、原文分片和数据库内部行不单独计数。为其分配稳定 ID/sequence；工具配对不能拆开清理，日期条件按完整单元最后活动时间判断。历史分页的 50 条同样按 history item 计数。

`clean` 自动执行一次选择/回收并报告候选、已删除、因活动使用而跳过的条数与空间；`--dry-run` 只预览。`retention set` 持久保存相同规则，Desktop 启动后、每小时及容量压力时分批运行；`--before` 也可保存为固定截止策略但不随时间移动，持续清理推荐相对天数或条数。无策略不自行选择删除期限。定时清理仅做数据库维护，绝不触发 LLM。

通过现有本地管理入口调用 Desktop 的单写入存储服务，不让独立 CLI 与运行中的 Host 无协调地删除数据。固定候选高水位后分批事务删除，幂等可恢复，Run 启动/回读 pin 与候选删除在同一存储序列内仲裁；返回的跳过计数解释为何暂时多于 keep-last N。pin 在 Run/读取结束后释放，崩溃后先恢复未决状态再解除，不能因为 Session 仍打开就永久禁止清理。

回收覆盖被删除历史的原文、截图、已无保留引用的摘要/索引及过时投影版本；共享 artifact 在没有保留所有者/活动 pin 后才回收。普通旧摘要的引用不永久 pin 原文，清理后通过可压缩的过期范围/最小标记声明来源已过期，不无界保留每条被删记录的全文/墓碑。仍在执行所需的约束、当前状态、未决动作和防重放安全水位保留，不能删除去重账本后让旧 action 再执行。下一次合法上下文构建剔除已删历史，只附加确定性的 history_pruned 边界，不立即调用模型补摘要。

每批变更更新 history_generation，App 按该版本失效分页和缓存；不允许被清理历史由旧移动缓存再次上传复活。SQLite 空闲页供后续写入复用，按有界空闲维护进行 WAL checkpoint/incremental vacuum；区分逻辑回收、可复用页和实际文件缩小，不每删除一条就全库 VACUUM。失败/空间不足时报告实际结果，不扩大清理范围。

### 9. 配置、扩展与供应商边界

实现统一 `aTerminal <resource> <action> [ID] [options]`，新旧 CLI 共享服务合同，旧 flags 保留兼容映射；App 设置复用同一 ConfigService。`~/.aTerminal` 管理版本化配置、用户 Skills、内置镜像、数据库、日志、备份和 IPC；OS 管理的 keyring 保持现有安全后端，目录存凭据引用，文件后端统一收敛到 credentials。旧实例运行时不迁移活 PTY/不启动第二个默认 daemon；迁移不删除源。

内置 MCP 和 Skills 随二进制发布，保留 builtin 身份，不接受用户覆盖、修改、停用或删除；低频 Skill 调用的终端动作仍经 Broker。用户 MCP 使用 JSON 和 rmcp，首版支持 stdio/Streamable HTTP；用户 Skills 支持 Codex SKILL.md/frontmatter、资源/脚本、openai.yaml、显式/隐式调用与渐进加载。扩展变更、MCP 通知和采样请求不能自动唤醒模型，任意用户脚本的执行不冒充 OS 级沙箱。

Provider 与 Model Profile 分离，首版覆盖 OpenAI、Anthropic、Gemini、Azure OpenAI、OpenAI-compatible 常见预设、Ollama/LM Studio。CLI 添加/编辑 Provider 和模型默认使用交互式向导，逐步选择协议、连接参数、凭据，从 Provider 拉取可搜索模型目录后选择模型并配置思考强度；保留非交互方式。Android/iOS 设置同样提供模型和思考强度的可修改表单，支持全局/Session 默认与覆盖。强度按模型实际支持的 level/budget/adaptive 等能力显示，不能用温度代替思考强度；模型/强度绑定原子保存。当前 Run 冻结配置/工具/Skills 版本，分析请求与主线完全一致；修改下个 Run 生效，无隐式模型切换。认证配置管理走独立授权 RPC，不向 Agent 暴露管理密钥工具。

目录布局、命令例子、MCP JSON、Skills 兼容范围、Provider 支持矩阵、远程修改和迁移细节以 [EXTENSIONS.md](EXTENSIONS.md) 为共同执行合同。

### 10. 有序实施

1. 按 CODEX-REFERENCE 的边界拆分与测试场景验证 Rig/rmcp 正式版与 Rust 1.94、各 Provider 必需协议、流式工具循环、分析屏障、取消及三平台依赖；验证原子 ReadView、底部向上内容锚点搜索及空白处理不会改变终端语义。锁最小 features，重大架构阻碍返回评审。
2. 统一 CLI 与 ConfigService，实现 ~/.aTerminal、版本化配置/原子提交、旧目录/凭据引用迁移、Provider/Model Profile、分步向导、Provider 模型目录、思考强度/能力校验及当前 Run 配置快照。
3. 实现项目 ID/状态/事件、SQLite 档案与执行账本、分页回读、PTY 状态入历史/用户消息前置快照及上下文投影，建立事务与恢复不变量。
4. 扩展 terminal-engine 原子 ReadView、tail/search/screen、首尾锚点与内容搜索、程序状态、会话级 Shell integration、统一输入/按键 Broker 与离屏 PNG。
5. 实现内置 MCP、只读内置 Skills、用户 MCP JSON/连接管理、Codex 格式 Skills 安装/发现/渐进加载/受管脚本，统一身份/授权/生命周期。
6. 接入单 Session Rig Loop，完成同历史/同配置尾部分析、引文校验、分层压缩、用户触发门控、取消和恢复。
7. 实现全局 Agent、Session Agent 注册、mailbox、用户 Run 内委托和并发预算；验证不跨 Session 写入、不被被动回报唤醒。
8. 实现 Desktop 历史清理 CLI、自动保留策略、分批回收与引用过期/分页失效。
9. 扩展加密协议、mobile-core/UniFFI、Android/iOS AI 对话、每页 50 条历史/有界缓存/旧历史迁移，以及 Provider/模型选择/思考强度/MCP/Skills 设置；更新旧 AI 定义、CLI 帮助、部署与兼容性文档。
10. 完成下列自动化与构建验证，记录真实供应商和平台验证范围，不把模型桩结果称为线上验收。

## 锚点策略修正（2026-09-26，用户追加授权）

默认原文首部 10 行、尾部 20 行；账号级 `terminal_reading.head_lines/tail_lines` 可配置，当前 Run 冻结配置。原文、展示首尾和 UUID 回读保持无损；单独生成搜索首尾锚点，剔除已识别的 TUI 行。

alternate screen 的内容明确属于 TUI，不作为日志搜索锚点。普通日志底部混入的状态栏、输入框、进度/旋转指示、交互提示和 UI 边框由分析阶段以精确原句标注；Host 校验原句存在后派生搜索锚点，不允许模型改写原文。显式文本、record edge 和消歧候选统一过滤。模型/调用者可对手工锚点明确标注 TUI 原句，匹配仅忽略已标注 TUI 与空白，不猜测删除普通日志。过滤后空锚点明确失败，不能退化成任意位置搜索。更新 MCP/内置 Skill 提示、CLI/App 配置与回归场景。

修正完成：常规全工作区 110 项测试通过，ModelScope 真实复验 1 项通过，Android 16 `emulator-5586` 仪器测试 1 项通过。默认 Desktop 空闲时加载新实现并持久配置 10/20；详见 [ANDROID-AGENT-UI.md](ANDROID-AGENT-UI.md) 与 [LIVE-MODELSCOPE.md](LIVE-MODELSCOPE.md)。

## 追加真实供应商验收（2026-09-26）

用户提供并授权配置 ModelScope API Key、`https://api-inference.modelscope.cn/v1` 和 `Qwen/Qwen3.8-27B`，继续执行原计划中的真实供应商自动化验收。使用临时 HOME/SQLite/PTY，只发送合成测试数据；测试和报告不嵌入凭据。直接配置当前 Desktop 的 Provider/Model，保留现有会话。

验收已完成：ModelScope 真实流式工具循环、终端读取/分析/写入、回读与请求去重通过；修复了真实响应暴露出的证据指令歧义，保持严格验证。常规回归 105 项通过（live 默认忽略），live 显式 1 项通过。默认 Desktop 检查无会话和活动 Agent 后载入修复，默认绑定已生效。详见 [LIVE-MODELSCOPE.md](LIVE-MODELSCOPE.md)。

## 执行结果（2026-09-26）

已完成 Desktop 双层持久 Agent、真实 MCP/Skills、终端读取/分析/输入与截图、Shell 状态、历史清理/保留策略、CLI/配置服务、mobile-core SQLite 缓存及 Android/iOS 对话和配置界面。原文、工具参数/结果和分析归入完整交互单元，重启不自动推理或重放；用户继续后可恢复未完成分析。

最终工作区 **104 项测试通过**；全工作区 clippy（`-D warnings`）、fmt 和差异检查通过。Android 三 ABI、iOS device/simulator 原生库、UniFFI、Android debug/测试 APK 与 lint、iOS 模拟器 App 构建通过。证据明细见 [TODO.md](TODO.md)，使用方式见 [Agent 说明](../../../deploy/ASSISTANT.md)。

最初阶段未执行真机交互、真实供应商调用或 Linux/Windows 运行验收；随后追加的 ModelScope 真实验收见上节。没有将本机模型桩或编译结果等同这些验证。按技能要求保留 [HANDOFF.md](HANDOFF.md) 的用户验收参考，不作为尚未完成的代码实施项。没有部署、重启用户现有终端或提交工作区。

## 验证

关键自动化用模型桩、临时 SQLite 和隔离 PTY；不操作用户现有终端。批准后创建 TODO 并记录实际命令和结果。

- 文本合同：tail 无锚点读取成功，search 缺失/空白 start 必须拒绝且不回退；200 行底部读取 → 以 head 向上扩展最多 1000 行 → 在旧 tail 提前截止；两个边界排除、相邻块不重不漏、重复锚点/缺失/颠倒/全空白/不足配置行数/超长行/扫描限额明确。覆盖 UTF-8、CJK/组合字符、软换行、进度条、清屏/擦除历史、全屏重绘、resize、alternate screen、读取中并发输出/视图过期；禁止混合时间画面伪造连续记录。
- 状态与输入：直接子进程退出、前台 pipeline、Shell 命令返回码、TUI 活着但内部任务未知；人工抢占、账号切换、只读设备、相同 action 重试、同 ID 改参数、输入写入失败和取消后的旧动作拒绝。
- 持久化与压缩：读取提交/分析前后/投影提交前后注入崩溃；UUID 原文及首尾锚点一致，锚点确定性附入且不被 LLM 改写、最近读取链锚点不被自动清理，旧锚点可轻量回读；引用校验、token 预算、tool-call 配对不变；多代压缩与目录分页仍可回读；分析失败不丢原文或覆盖新消息；磁盘满不执行未记录的 AI 动作。
- 触发与缓存合同：PTY 状态连续变化/退出、子 Agent 回报、定时清理、分页、重连和重启造成模型调用数为零；真实用户消息前捕获状态且只有变化才新增快照，未变化复用已有状态引用，同一 request 重试不重复插入；工具循环/分析/压缩/委托均有 root_user_message_id。比较实际请求构造结果，分析与主线配置相同、旧消息前缀逐项相同、只追加末尾指令；不靠模型桩证明真实供应商命中缓存。
- 清理：三种互斥选择器、日期时区/边界、每 Session keep-last、完整交互单元、活动 pin 与并发回读/写入、批次中断恢复、共享 BLOB 回收、被引用原文过期、去重水位、策略重启生效与关闭；验证数据能逻辑回收并复用空间，清理不执行模型或终端动作。
- 分页：0/49/50/51/多页记录、每页含大原文引用、同一时刻多消息、并发新增、请求重试、快速切 Session、底部触发/加载互斥、清理期间 cursor 失效、离线缓存失效和旧格式迁移；验证数据库/网络/内存均有界，禁止“全量读取后切片”。
- CLI/配置/迁移：所有命令共享命名与 JSON 错误合同，旧 flags 等价；默认目录与 state-dir 隔离、Windows 私有权限、活跃旧 daemon 不被重启/复制、目录冲突/迁移中断可恢复、原 vault/设备身份不丢失、ConfigService 并发冲突/多文件提交失败、密钥脱敏和跨账号/只读设备拒绝。
- MCP/Skills：本地模型桩 + 假 stdio/HTTP server 验证 initialize/工具目录分页/调用/图片/取消/超时/协议错误，副作用不重发，sampling/notifications 不唤醒；内置条目各种修改/覆盖/删除路径全部拒绝；Codex 包解析、openai.yaml 隐式禁用、重名来源、symlink/路径穿越、脚本依赖失败/取消、当前 Run 版本冻结和前缀稳定。
- Provider/App 配置：交互向导上一步/取消/提交、非 TTY 不挂起、目录分页/缓存/错误/手工 ID、各协议流式/工具/usage 合同、endpoint/deployment 差异、强度 level/budget 范围与参数互斥、不支持/未知能力、换模型清除不兼容强度；App 设置实际能修改模型和思考强度，CLI/App 回读一致，默认/Session 覆盖正确，下一 Run 生效且读取分析使用同一配置。离线不自动覆盖、密钥 write-only、配置保存/浏览/元数据获取不产生生成推理。真实供应商测试按已提供凭据与用户测试消息执行，缺少条件记录未验证，不把兼容预设当实测支持。
- 源码参考落地回归：捕获序列化请求体比较稳定前缀、工具目录和 ID；合成角色为 user 的 PTY 消息仍不能启动 Run；更换端点/认证身份后模型目录缓存隔离；检查点恢复保留未决动作及新后缀、模型投影不读全量原文、游标跨 scope 拒绝。仅借鉴 Codex 测试场景，不运行其全套产品测试来代替本项目验证。
- 截图：固定字体的确定性 PNG 测试、CJK/组合字符布局和颜色/光标检查，图片与文本 revision 对齐；不只验证文件存在。
- 端到端：两 Session 并发、全局异步委托/追加、App 断线后 Desktop 继续、事件补读去重、Desktop 重启后 PTY 已不存在时正确 unknown/orphaned；不自动创建同名 Shell 并重放旧动作。
- 执行 `cargo +stable fmt --all -- --check`、受影响包 tests/clippy 和 `cargo +stable test --locked --workspace`；构建 desktop、mobile-core、Android 和 iOS 模拟器目标，具体命令沿用 `deploy/LOCAL-DEBUG.md` 与 CI。macOS 本机验证真实 PTY；Linux/Windows 使用可用 CI/环境验证，不把本机编译当跨平台运行通过。
- 本轮仅代码与文档检查，未探测设备或运行测试。实现后先判断设备环境是否可靠；真机测试按技能要求另行询问是否需要。不能可靠执行时，将全局/Session 对话、证据回读、截图、断线重连与人工抢占的场景写入 HANDOFF，不写入自动测试或 TODO 冒充已验证。

## 风险与回退

- PTY 输入与数据库无法形成 exactly-once 原子事务；选择不自动重发未知动作，并暴露不确定状态。现有 Desktop 重启会销毁 Shell，Agent 持久化不承诺 PTY 存活。
- 不保证所有平台/任意 TUI 都能确定“任务已完成”；以 capability 和证据分层表示缺失。支持的 Shell 集成必须保持原有配置行为，失败时显示无该能力，不猜测完成。
- Rig 0.x、终端视图与锚点边界、字体布局是主要适配风险；锁版本、窄接口、终端语义回归测试，不更换现有终端引擎或复制第二套解释器。
- 原始档案包含终端敏感内容，持续保存会占磁盘；账号隔离、按需发送、文件权限、容量上限与自动保留策略落实在存储层。保留期内的原文可回读核验；已清理原文明确过期，不承诺永久回读。数据库删除不等于对磁盘介质或离线手机的安全擦除。
- 实时原文卸载和后续历史压缩会改变部分缓存前缀；通过同请求配置、尾部追加和阈值批处理减少影响，真实缓存命中率以供应商返回为准，不为了缓存保留过期原文或突破上下文上限。
- 统一目录涉及原 vault 目录 hash 和活 daemon 发现；按 EXTENSIONS 的迁移合同处理，不通过重启现有 Shell 强迁。用户脚本/第三方 MCP 为本机进程权限，Skill 文本和内置只读保护不是机器级沙箱。Provider/认证方式未验证的能力必须明确显示，不自动回退泄露上下文。
- 新 Runtime 可由 feature/config 关闭；旧人工终端协议保持，数据库采用版本化迁移和迁移前备份；回退不自动删除档案，不恢复旧 AI 写任务。模型/存储故障仅暂停 AI，不停人工终端。

## 未决问题、歧义与确认

None.
