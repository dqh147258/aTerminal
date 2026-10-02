# Agent 操作授权与工具补齐

- 状态：In progress；协调者完成方案后直接执行，无需用户 Review。
- 日期：2026-10-02，Asia/Singapore。
- 起点：`main` / `f56461eb9b6d7a5cb3ba19cf698779208a81f7ee`；工作树原本干净，AGENTS.md 为空。
- 用户授权：补充权限和此前建议的六类工具；允许协调者自行决定实现细节、通过 worktree-tasks 分工。用户明确“计划我不Review, 你看着处理”。
- 已确认：完全授权作用于当前 Agent 对话、持续到关闭；永久授权按命令/参数/目录精确匹配；六类工具全部补齐，Android/iOS/CLI 同步。
- 管理任务：`aTerminal / 20261002-180220-295-agent-authorization`。
- 恢复授权（2026-10-02 19:55）：系统故障后用户要求继续并恢复全部子任务，全部模型改为 Sol。已检查保留代码与会话后，六个原子任务使用 `gpt-6.1-sol / high`、各自保存的 thread ID 在原 worktree 恢复；旧 run/终端保留在任务历史，不新建重复子任务。
- 最新调度约束（2026-10-02 20:56）：系统再次重启、子任务已关闭。用户要求任何时刻最多两个活跃子任务，完成并关闭后才恢复下一项。当前仅恢复 runtime/toolset，policy 已完成且不恢复，Android/iOS/Review 排队；所有恢复仍使用 Sol。大型构建/模拟器错峰，cargo jobs/test threads 最多2，不改变 High 必要验收范围。

## 目标与实际行为

默认采用 `ask` 模式：内置只读观察及能够严格识别的常规安全命令自动执行；其他动作先显示请求，用户选择“授权一次 / 永久授权 / 拒绝”。另保留显式 `read_only` 模式。完全授权是当前 Agent 对话独立开关，开启后其后续动作与本次委托任务跳过操作审批，持续到用户关闭；账号、设备、Session 范围、人工抢占、取消与执行预算继续生效。

不增加工作目录沙箱。`cd` 后可以继续在新目录工作；审批和精确规则绑定实际观察到的 cwd，目录变化后重新评估。安全分类是应用层动作判断，不保证未知 Shell 插件、用户程序或 MCP 进程在 OS 层只读。

每个审批展示具体操作、目标 Session、cwd、完整有界参数、原因与永久规则的匹配范围。敏感值只显示脱敏摘要；规则使用内部指纹，不能因脱敏文本相同而相互匹配。永久规则保存在 Desktop，按账号/Desktop 隔离，可查看并撤销；不把通配程序名、可变目录、未知 cwd 或模型声明的“安全”当作授权。

授权一次绑定确切 pending/action ID 和参数指纹，只允许该动作一次；重复手机请求幂等。拒绝该动作后模型收到明确结果，可继续观察、解释或选择别的办法；不得自动重试相同拒绝动作。完全授权不能由模型、终端文字、MCP 输出或 Skill 自行开启。

补齐完成结果、批量子任务、用户问答、历史搜索、增量读取/等待、能力查询。接口与边界以 [CONTRACT.md](CONTRACT.md) 为准。

## 已查明的实现与关键改动点

| 位置 | 现状与用途 |
| --- | --- |
| `crates/agent-runtime/src/host.rs::terminal_tools` | 当前 Global 16 / Session 12 个工具；按角色固定目录，所有调用经过 Host |
| `host.rs::run` 的工具执行段 | JSON schema 校验、`allow_write`、共享 Budget、取消、动作账本、读取分析屏障；新审批必须在副作用前接入 |
| `crates/agent-runtime/src/store.rs` | SQLite Run、委托、证据、动作防重放；加入审批、对话模式、精确规则、用户答复与有界历史检索 |
| `crates/desktop-agent/src/service/runtime.rs::Backend` | 账号设备复核、Session 绑定、人工版本 fence、工具分类与路由；目前 `mcp_call`、用户脚本统一视作写入 |
| `crates/desktop-agent/src/service.rs` | 真正 PTY 写入、控制租约、Desktop attachment、取消 gate；新授权不能绕过这些检查 |
| `crates/desktop-agent/src/remote_bridge.rs` | 只读设备只能浏览 Agent；审批/答复/模式修改仍必须是可操作的用户设备 |
| `crates/desktop-agent/src/shell.rs` | 会话内 bash/zsh/PowerShell hooks，当前只有 phase/cwd/exit_code；扩展命令序列和关联证据 |
| `crates/desktop-agent/src/extensions.rs` | 用户启用的 MCP/Skill 版本快照、参数 schema、脚本执行；精确规则需要版本/目录指纹 |
| `crates/desktop-cli/src/agents.rs` | 当前只有 send/show/stop/history/record；新增模式、pending、resolve、rules 与答复入口 |
| Android `AgentPanel.kt` | 当前每次发任务固定 `allow_input=true`；新增跨 Run 对话模式、待办卡片、永久规则管理 |
| iOS `AssistantModel.swift` / `ChatPanel.swift` | 当前默认关闭 allowInput；与 Android 采用同一新协议与交互 |

## 安全分类与终端输入

内置观察、等待、目录/证据查询自动放行。安全命令采用保守、可测试的识别器，只接受明确允许的程序及参数形态；包含脚本解释执行、提权、删除/覆盖、网络发送、可变程序、重定向、命令替换、管道或复合表达式的动作不能凭命令名自动放行。无法解析时进入审批。依赖仅在确实需要时新增；普通分词库不宣称提供完整 Shell 语法安全验证。

原始 `input_text` 和 `send_keys` 必须经过同一个判定入口。分次输入、文本内换行、`submit=true`、单独 Enter、粘贴、快捷键、TUI 交互不能绕过审批。只在 Host 能关联完整输入、明确的 Shell 状态和未被人工更改的版本时尝试安全命令识别；否则将原始输入作为待审批动作。新增结构化 `run_command` 方便完整命令审批和结果关联，但不替换正常 PTY 或绕开工作目录变化。

独立审阅确认：实际 Shell 的别名/函数/PATH 不能通过当前观测可靠证明程序身份；普通读取增加 `inspect_command` 正向固定程序/参数入口，在实际 cwd 以原生子进程运行并提供证据（不经 Shell，不改 PTY draft/目录），确保安全读取无需逐项审批。run_command 保持原 PTY 交互语义，未知输入仍审批。此为用户授权范围内的实现细化，不增加 OS/cwd 沙箱；详情及精确永久语言边界见合同。

2026-10-02 23:52：Review实测Bash/Zsh可用同绝对路径Shell function替换实际程序，早期固定字符串规划仍不能安全承载PTY永久规则。协调者自审采用独立 `run_program(program,args,stdin?)` 原生执行入口：可靠leaf程序hash/完整literal参数/当前cwd可精确永久，unknown程序once/full；原PTY输入once/full，can_always=false，不静默改变终端语义。策略namespace升级v3废止旧宽松规则；改动留在已批准权限/工具范围，runtime接管所需已关闭policy/helper文件的最小接线，review准备真实native授权/覆盖回归，不恢复第三个执行者。

已安装的 MCP/Skill 可执行能力由用户管理。MCP annotations 仅供显示，不能自行扩大自动放行范围；当前保持所有实际用户 MCP 调用/脚本进入审批或精确规则。`mcp_tools` 会惰性启动已启用服务，此生命周期属于用户配置的既有扩展能力；能力界面明确说明，不把 catalog 查询宣传为 OS 沙箱。

## 执行分工与依赖

通过指定 skill CLI 启动独立 iTerm Codex worktree；协调者维护本计划、接口决定、集成、Review 和最终验证。子任务不擅自修改其他组件或启动下一层代理。

1. `authorization-policy`：新增纯策略模块、精确指纹/脱敏/命令分类与攻击用例；先提供稳定 API。按用户最新要求使用 `gpt-6.1-sol / high`。
2. `authorization-runtime`：审批/问答持久化与恢复、对话模式、Host 执行接入、Broker、远程授权和 CLI；依赖策略模块与工具 helper，负责共享文件最终接线。使用 `gpt-6.1-sol / high`。
3. `agent-toolset`：新增 helper 模块实现六类工具、命令关联与 Shell hooks；优先新增 `host/tools.rs`、`store/tools.rs`、`service/runtime/tools.rs`，由 runtime 子任务接入父模块，减少同时编辑同一文件。使用 `gpt-6.1-sol / high`。
4. `android-authorization`：Android 模式开关、审批/问答卡片、规则查看撤销、协议和 UI 回归；不改 Rust。使用 `gpt-6.1-sol / high`。
5. `ios-authorization`：iOS 同等功能、请求状态恢复与交互测试；不改 Rust。使用 `gpt-6.1-sol / high`。
6. 集成后安排独立 Review/验收，也使用用户要求的 `gpt-6.1-sol / high`。存在共享接口冲突时先协调，不能靠同时覆盖文件解决。

当前执行顺序以最多两个活跃执行者为硬约束：先 runtime + toolset；toolset 完成、提交并关闭后可恢复 Review，与 runtime 处理安全/集成；runtime 完成并关闭后恢复 iOS；iOS/Review 任一完成并关闭后恢复 Android，或按未决依赖在两个名额内调整。恢复/发送会重新激活 completed 子任务，操作前必须检查名额，不能向已关闭排队者发送会唤醒执行的指示。完成关闭按用户本次明确授权执行，保留分支/worktree直到最后整合与验证满足清理条件。

子任务读取 main 中本计划的绝对路径；用户执行授权已经覆盖这份方案。用于集成和测试的提交/合并属于本次交付步骤，按 worktree skill 保留未 Review 提交标记。逐支审查后合入实际 `main`，保留无关工作。满足集成、测试与干净 worktree 条件后才清理任务 worktree/分支/终端；现有旧 worktree、账号、活动终端和服务不动。

## 验证安排与完成条件

采用 High，原因是改变所有副作用工具的执行门控、跨端用户授权、持久化和并发恢复。自动化使用隔离账号/Desktop、临时 PTY 和本地确定性模型/MCP，不调用用户付费供应商。

必要验收：

- Rust 策略与真实 Host/Broker/MCP 集成：安全命令、未知/危险命令、分次输入/Enter/换行绕过、精确 cwd/参数/版本变更、规则撤销、完全授权及委托继承、只读模式/设备、跨账号/Desktop/Session、假审批、重放、取消/人工抢占/断连/重启与过期处理。
- 人类等待：不启动额外模型调用、不刷新工具调用额度；等待、取消、共享时限和恢复符合 CONTRACT，多个设备重复答复不会重复执行。
- 新工具：精确命令退出/unknown、批量 any/all 与超时、取消旧任务不影响新任务、历史分页/保留/隔离、delta/view 过期和状态改变、能力参数契约及非视觉行为。
- Android/iOS/CLI：一次/永久/拒绝、完全授权启停、待审批与问答的恢复、切换账号/对话、规则撤销、无权限设备、不同屏幕/字体下按钮可达；老 Desktop/老客户端兼容不提升权限。
- `cargo +stable fmt --all -- --check`、相关 crates 的 test/clippy、CLI 集成、Android 构建/相关 instrumentation、iOS 构建/相关 XCTest 或既有 Swift 主机测试，以及 `git diff --check`。
- 最终集成 main 上复核有关测试；证据保存至本任务目录，结论区分本地模型桩与真实供应商。

真机：不计划物理设备验收。目前 adb 只发现 `emulator-5586`，没有已确认可用物理 Android；iOS 设备发现被当前沙箱的 CoreSimulator 服务访问限制。必要移动端验证使用现有 Android 模拟器与 iOS 模拟器/主机测试，iOS 构建和模拟器需要按环境权限申请工具执行升级。预计既有设施准备加相关 UI 测试约 1–2 小时，不额外搭建昂贵测试系统。物理设备行为和真实供应商质量不作为已验证结论。

完成要求：全部已确认范围落地、独立 Review 的实质问题已处理、必要检查通过或用户明确调整、最终主线代码可复现。不能把未实现的工具或跨端入口改写为可选建议。

## 当前进度

- [x] 核对现有权限/工具/移动端/CLI 调用链，确认用户三个范围选择及无需 Review 的授权。
- [x] 读取规划、worktree-tasks、依赖 worktree-flow 与模型策略；创建顶层管理任务。
- [x] 编写主计划与统一接口合同；由协调者自审后执行。
- [x] 启动并记录五个实现子任务与各自 worktree/分支/SessionID，见 [WORKTREES.json](WORKTREES.json)。均从 `main / 3f8ebe0` 创建，无复制旧凭据或运行状态。
- [ ] 策略、运行时和工具 helper 交付并互相接线。
- [x] 策略与工具 helper 自身范围已完成并提交；toolset 22/22聚焦测试、strict clippy通过，2026-10-02 23:37 已按用户要求关闭其终端。runtime最后必要检查仍在进行。
- [ ] Android/iOS/CLI 同步并完成各端验证。
- [ ] 独立 Review、必要修复、最终 main 集成测试。
- [ ] 保存最终结果和恢复信息，完成 task，按条件清理新建子任务资源。
