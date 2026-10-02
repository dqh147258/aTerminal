# 独立授权审查与验收

2026-10-02；当前处于阶段 2 独立源码 Review。已将协调者指定的集成 commit `8a1d18f` 合入此 review 分支，未覆盖 root 权威合同。原始审查基线为 `3f8ebe0c0ff069a471a50a9f336abad9f07444c9`。十个独立加密 RPC 场景源码已保存于 `d125bf0`；后续 MCP/deny→full/dispatch 场景继续补充。大 Cargo 窗口当前属于 runtime，尚未运行这些行为测试。

当前阻止最终 Review 完成的项目：新 run_program 永久闭环、MCP commit 序列化 R16、deny→full 行为 R14，以及必要加密 RPC 和移动端真实 UI 验收。R9/R10/R13 已见集成源码修复，仍需行为验收。所有测试结果必须区分源码审查、独立本地 Shell 复现、加密 RPC、模拟器，不把本地 Shell 复现当作完整永久规则链通过。

采用主本 `/Volumes/Code/My/aTerminal/doc/task/1002-agent-authorization/PLAN.md` 与 `CONTRACT.md` 的现有授权和 High 验证强度，不新增用户审批。仅编辑 review 文档、独立新增测试文件与协调者明确分配的两个 fixture 文件。不合并 main、不清理 worktree、不使用生产账号/终端或付费模型。

## 当前证据和优先风险

下列是基线调用链中必须由本次实现处理的风险，不能直接当作尚未完成分支的缺陷。所有项目在阶段 2 按最终实现重新核对。

### R1：输入状态不足以证明安全命令（高）

基线 `service/runtime.rs::input_text/send_keys` 最终经 `Backend::write` 到 `service.rs::AgentWrite`。人工输入 revision 防止任务开始后的抢占，但不能证明任务开始前的命令行为空，也不能识别模型前一个获批动作遗留的输入。`shell.rs::observation` 明确标注 `trusted_for_authorization:false`；prompt 状态在用户已输入半条命令时也可能仍存在。

必须观察：预置 `touch marker; ` 等未提交输入，然后模型提交看似安全的 `pwd`；先 input_text 再单独 Enter；换行、回车、粘贴和 TUI 键盘路径。不得仅按本次字符串分类 Safe。若不能证明完整命令/空输入行，应审批原始输入；结构化 run_command 也不能仅凭工具名称绕过同一问题。任意人工输入后的旧动作即使获批也不能写入。

### R2：精确规则绑定真实执行对象与目录（高）

`process.rs::cwd` 会复核 PID identity 后通过 OS 观察 cwd，`SessionInfo.cwd` 是初始目录，hook cwd 是进程可写证据。审批必须区分缺少可信 cwd 和真实目录，不能在未知时回退成旧目录形成永久规则。`extensions.rs::script` 当前使用 Run 快照的 `self.cwd`，而 PTY 后续 cd 可改变实际 Shell cwd；新 descriptor 所指目录必须与真正 script spawn 的目录一致。

阶段 2 检查：同参数/不同 cwd、相同脱敏预览/不同秘密、JSON 对象重排/数组重排、脚本内容更新、MCP 同名能力替换、解释器/可执行文件变化。审批后、调用前重新生成匹配材料。对于不可固定的远程服务版本或未知程序，只允许 once。目录符号链接重定向需明确是否能产生稳定目标；不能把简单 lexical path equality 当作实际对象保证。

### R3：full 不应给模型开启/持久化授权的能力（高）

本地 `Client::connect` 从状态目录读取 endpoint token，PTY 与 daemon 通常同 UID；`Client::call` 可以构造任意本地 Agent RPC。远程桥已经覆盖来自客户端的 account/device 字段，模型工具不应获得用户 RPC 构造器。

必须在风险审批之外硬拒绝识别到的授权管理入口，在 full 和永久规则命中时仍拒绝；覆盖直接 CLI、路径前缀、包装命令和配置修改表达式。模型报告、终端文字、MCP/Skill 结果、ask_user 答案均不是权限开关指令。纯 Shell 字符串拦截不能证明任意同 UID 代码都不可访问 token：最终报告应准确描述应用层防护边界，不能声称具备 OS 隔离。本项已提前通知协调者，要求明确最终保障范围。

协调者已明确：本次不引入 OS 沙箱，不承诺隔离任意已授权程序对同 UID 资源的修改；识别到的模型权限管理直接入口仍为 Forbidden，Host/Broker 不提供自批工具。最终验收按此应用层边界执行，A17 不扩展成无法由当前架构保证的任意 Shell 隔离承诺。

### R4：人类等待必须重构共享计时与已有超时（高）

基线 `Budget::deadline` 是绝对 Instant；Host tool execution 和 `wait_agent_task` 启动 deadline sleep/timeout。仅让 `remaining()` 在人类等待时返回更长值，无法延长已经创建的计时器。父任务 wait_child 加子任务 human wait 时，父等待本身也不能错误计作可运行工作；相反，一个正在运行的兄弟任务必须继续消耗整树预算。

必须观察：超出短活跃预算的人类等待后能继续；等待期间模型请求数不变；解除等待不会重置已消耗 calls/tokens/tools；两子任务只有一个等待时预算照常耗尽；全部等待暂停、其中一个回答恢复；根/子取消及时唤醒；24h TTL 独立于活跃预算。取消/提前退出时计数器须有守卫释放，避免整树永久暂停。

### R5：pending 决策与执行必须有不同的幂等终态（高）

一次点击只给一个确切 action grant；两个设备并发 once/deny/always 需事务化，后到者返回已有决策或冲突，不重写决定。幂等 request_id 不得跨 pending/账号/对话重用成功。拒绝相同动作的去重范围是该 Run，不能永久封禁新用户任务。

需检查重启窗口：pending created、resolved、action prepared、PTY accepted、receipt persisted。重启不重放任何已未知动作；旧 grant 不能启动新 root。撤销规则/关闭 full/切 read_only 必须影响尚未执行动作，不能只改变 UI。

### R6：等待状态不能退出既有安全 watcher（高）

基线 `Host::run` watcher 仅在 `running(state)` 为真时复核账号、来源设备和凭据。加入 suspended/waiting 后，必须继续观察撤权与取消，且不能让新的用户 Run 把尚在等待的 job 当作空闲覆盖。pending resolve 后还需授权复核；不得持有 SQLite/Job/PTY 锁等待人类，导致 cancel/resolve 死锁。

### R7：MCP/脚本副作用前的最后检查（高）

基线 `Extensions::call` 先惰性获取 server/catalog，再发实际 tool call；脚本在 spawn 时持有 execution_gate。授权必须绑定最终 schema/能力版本，实际 MCP 请求前也应检查取消/撤权，不能只在可能耗时的 catalog 连接之前检查。MCP annotations/readOnlyHint 不能自动放行。

### R8：命令结果关联必须把证据不足留为 unknown（中高）

基线 hooks 只有 phase/cwd/exit_code，不能把上次 prompt exit_code 或“accepted”当作完成。新 command_id 必须与提交时输入版本、执行序列和精确文本关联；人工插入、旧 hook、并发冲突、重启、过期、无 hooks 保持 unknown。后台子进程存活不改变已完成 Shell 命令的范围，TUI 不可假装提供通用完成证据。

### R9：撤销后重新永久授权会产生不可撤销规则（高，待修）

2026-10-02 独立读取 runtime 未提交的 `store/authorization.rs::consume_pending` 发现：先 always(P1)，revoke(P1)，同指纹再 always(P2)，SQL conflict 仅更新 revoked/value，数据库规则 ID 仍是 P1，而公开 value.id 为 P2。rules 返回 P2，revoke(P2) 找不到记录。已通过 subtask message 通知协调者。需令存储 ID 和公开 ID 一致，并做真实再授权/再撤销回归；最终集成时重新核对修复。

### R10：第一个程序的 hash 不能固定完整执行身份（高，待修）

runtime 未提交的 `action_descriptor` 仅 hash 第一个绝对路径 token，policy `can_always` 只判断 identity 非空，因而 `/bin/bash /tmp/mutable.sh`、`/usr/bin/env mutableProgram`、`/bin/printf ok; /tmp/mutableProgram` 都可能永久匹配，但未固定实际脚本/后续程序版本。精确参数文本相同仍不足以固定这些可变执行对象。已通知协调者，建议未知解释器脚本、wrapper、compound 只支持 once，直到有完整的执行身份材料。

### R11：自动安全命令必须约束 Shell 程序解析（高，协调者已报修）

裸 `ls` 等名字可以解析为 Shell alias/function 或 PATH 中自定义程序。仅严格 lexer + prompt/input proof 不足以判 Safe。需要 Host 提供可信程序解析，或采用等效的明确路径执行语言，覆盖实际安全命令可用性与 alias/function 攻击；不能通过将所有命令判 unknown 来宣称解决。

当前合同采用 `inspect_command` 原生固定只读程序/字面 argv，实际 cwd 不受 Shell alias/function/PATH/draft 影响；正常读取的自动安全验收通过该入口进行。普通 run_command 保持 PTY Shell 语义，证明不足时审批。

### R12：审批内容必须足以理解动作（高，协调者已报修）

policy 初版 `5eb063c` 把所有普通 operands、文本、路径和未知 MCP 参数隐藏。例如 `rm /tmp/a` 与 `rm /tmp/b` 的预览相同，用户无法理解批准对象。协调者已否决并安排可读 preview followup；普通命令、参数、路径应可读，仅敏感 key/flag/known secret 脱敏。过长动作的新 `approval_details` 分页和完整详情 ack 必须在服务端、CLI、Android、iOS 同时生效；未读齐不能 once/always，deny/stop 应仍可达。

### R13：结构化 once/always 的输入证明在末端可能不检查（高，待修）

恢复后读取 runtime 的 `user_interaction.rs`：`check`/`commit` 仅在 `initially_safe && run_command` 时检查输入 revision/空行，最终合同中 PTY run_command 全部 RequiresApproval，因此这些条件不会成立。永久指纹按设计不含瞬时 revision，common 使用初始 rule_eligible；若另一 Agent 在 pending 等待期间向同 Session 写半行而不改变 manual_revision，旧 grant 仍可能把命令追加到不同 draft。需绑定每次确切动作的 Actor input_revision，结构化输入的已有证明在消费前重新核对；规则命中也必须重新确认当前 can_always。不要拿终端输出 screen revision 代替输入 revision，避免无关输出造成假冲突。本项已通过持久 message 报协调者；CLI direct send 的进程核验失败，未把它当作执行者未恢复的证据。

在 `8a1d18f` 中已见 `action_fence` 绑定 epoch/manual/input_revision，所有 PTY write 的 Actor commit 比对 fence，规则使用时重查 can_always；empty_evidence 只接受初始零输入或完整提交/sequence/command 匹配。runtime 的真实 Actor proxy 竞态测试已由协调者报告通过，独立测试仍待运行。

### R14：显式 full 未覆盖同 Run 的风险拒绝缓存（中，待验）

`approve_action` 先查 action_denied 再判断 full，导致用户同 Run 拒绝后开启 full，后续新的相同参数 toolcall 仍被旧拒绝缓存挡住。结论：原拒绝动作不重放，后续新调用按当前真实用户 full 放行风险门控；Forbidden、scope、fence、取消和只读仍优先。协调者已接受，runtime 正修；独立 encrypted 测试使用 deny→question 暂停→真实 full 开启→真实 answer→新的同参数动作，并要求只有一行实际 marker。

### R15：绝对程序文件 hash 无法固定 Shell function dispatch（高，已本地复现）

隔离 Bash `--noprofile --norc` 和 Zsh `-f` 实际可定义带 slash 的 `/bin/echo` function。前后提交完全相同 `/bin/echo original >> marker.log`，系统 `/bin/echo` 的文件 SHA256 不变，而 marker 从 `original` 变为 `override`，并产生 function-hit 文件。证据 [shell-dispatch.json](evidence/shell-dispatch.json)。Bash slash alias 被拒，Zsh slash alias 成立；不要把 Bash alias 也报告为成立。`command /bin/echo` wrapper 也可被 function command() 覆盖。

完整 Host 永久规则跨 Run 验收尚未运行，不能仅凭 Shell 复现声称完整 grant 链已复现；但现有 descriptor 只固定 program bytes，没有实际解析状态，其身份保证不足。已向 root/runtime 提交最小提案：显式 native 固定程序/argv/redirs 分支提供永久能力，普通 PTY 保留 once/full。正式接口由 root 决定后再更新测试；不擅自修改 runtime/policy 所有源码。

root 已定案：独立 `run_program {program,args,stdin?}` 直接原生执行，env_clear/实际 cwd/末端许可/进程组管理，可靠 leaf 实际 hash 可永久；未知/解释器仍可 once/full。PTY run_command/input/keys 永远 can_always=false，namespace v3 失配旧 v2。此前 execution=pty|native 建议未采用，保留为历史。测试已改 native tee+stdin 精确行数，并分别在 Bash/Zsh 定义 slash-function 后重用 native 规则；普通 PTY 的同名调用仍须新审批。

### R16：MCP 最终 permit consumption 未受 execution_gate 序列化（高，待修）

集成 `extensions.rs::call` 在 lazy catalog 后复核并消费 permit，但消费时没有与 set_permissions/revoke_rule 共用 execution_gate。common 的权限读取与 consumed 标记间仍可穿插撤权完成。已建议 gate 外 check、gate 内短时检查取消/门状态并 commit_once，随后释放 gate 做 async MCP 请求；commit 为动作启动的线性点，不持 std Mutex 跨 await，不在 gate 内递归 check。已成功 CLI send 当前 runtime，并持久通知 root。

`06b4c9d` 已见 gate 内同步 commit 修复；实际 MCP marker 测试仍待执行。当前集成源码也已修 R14 的显式 full 拒绝缓存优先级，并正式采用独立 run_program/v3，R15 的旧 PTY 永久入口已禁止。

### R17：native 结果被旧 PTY 关联和失效逻辑污染（高，待修）

`06b4c9d` 的 `Store::invalidate_command_writes` 对任何未 final 命令都置 unknown，包含仍在管理进程中运行的 native 行；`tools::command_result` 对 native 未 final 行仍调用 Shell correlate，因无 baseline 立即不可逆地 final=unknown。最终真实 native exit 无法更新。已成功报告 root/runtime，新增独立双 Agent 流程：Scene native 慢进程运行，Global 先 get_result、再在同 Session 写 PTY、最后 wait_result，必须保留真实 native completed/exit0 和独立两个 marker。

分流后也需处理 restart：旧 native running 行不可因缺少活跃线程而永远 running。现 command 表没有持久 run_id，Store.open 尚未重置旧非 final native 状态，已向 runtime 报需要可靠 orphan/interrupted 恢复且不重放。

### R18：远端 HTTP MCP 显示了未使用的本地 cwd（中，待修）

`Extensions::authorization_descriptor` 把所有 MCP 的目录设为 binding.config.cwd 或冻结 Session cwd，但 streamable_http transport 没有使用这些本地目录，也不知道远端 cwd。审批可能显示错误的作用位置。建议 stdio 显示真实冻结 spawn 目录；HTTP cwd=None 并显示 unknown/remote。已成功 CLI send 当前 runtime。现独立 MCP 实际 marker 场景是 stdio，HTTP 目录准确性将以源码与聚焦测试核对。

## 阶段 2 必要验收矩阵

每项要记录测试名、被测 commit、行为证据与结果；下列均为待执行。

| ID | 场景 | 可观察通过条件 |
| --- | --- | --- |
| A01 | 新 ask / 旧 allow_input=true / 旧 false / model read_only | 危险 marker 在审批前不存在；旧 true 不直接执行，false 和 model readonly 即使 full 也不写入 |
| A02 | 严格安全命令及 shell 攻击 corpus | 正常 Safe 路径实际可用；复合语法/执行选项不会无审批触发 marker |
| A03 | 半行输入、分次 Enter、换行、控制键、TUI | 每条能触发行为的路径进入同一门控；无隐藏拼接命令被自动执行 |
| A04 | once / always / deny / 重复拒绝 | 仅对应 action 执行一次；always 后同指纹自动执行；deny 和同 Run 重试不写 marker、不重复打扰 |
| A05 | 精确指纹 | 改参数、顺序敏感数组、秘密、cwd、版本需要新审批；map 重排不造成错误分裂；未知 cwd/version 永久按钮不可用 |
| A06 | TOCTOU | pending 等待时改变 cwd/脚本/MCP 配置/人工输入，批准旧请求后没有旧目标副作用 |
| A07 | full / 委托 / 关闭 | 根 full 对本次委托生效；不写入子对话开关；独立子对话仍 ask；关闭后未执行动作重新询问 |
| A08 | full 下其他防线 | 只读设备、账号改变、来源设备撤权、model readonly、Session 越界、Desktop detach、取消、budget 均不被绕过 |
| A09 | 真假 RPC 来源 | 只读设备能浏览；resolve/answer/set/revoke 被拒；猜到 pending UUID、终端假 approve、MCP 伪报告均不能授予权限 |
| A10 | 多设备竞态/重放 | 并发 resolve 只发生一次真实副作用；request_id 冲突不改目标；stale expected_revision 不覆盖新模式 |
| A11 | 人类等待共享预算 | 使用短预算和模型请求计数验证 R4；TTL/取消独立有效；恢复不刷新预算 |
| A12 | 重启/失联 | 手机重连保留 pending；daemon 重启旧 pending interrupted，resolve 不触发未知动作；规则和模式仍保留 |
| A13 | 命令完成 | 真实 shell 返回 0/非0正确关联；延迟任务 wait timeout 不取消；接受不等于成功；无 hooks/人工冲突返回 unknown |
| A14 | 子任务批量 API | any/all 固定 task ID 集合；超时不取消；取消旧 task 不影响同 Session 新 Run，不发送 Ctrl+C |
| A15 | 历史/delta/capabilities | 跨 owner/Desktop/Session 无记录泄漏；cursor generation 失效明确；TUI delta 不假称追加；角色 schema 与实际一致 |
| A16 | Android/iOS/CLI | 一次/永久/拒绝/问答/full/revoke 真 RPC 改变真实执行；切 scope 不误投；只读设备无操作；旧 Desktop 明确能力缺失 |
| A17 | 授权管理自升级 | 模型通过受控工具尝试 CLI/配置入口在 ask/full/规则命中下均不能启用或持久化授权 |
| A18 | full 与并发 pending | 开启 full 释放有效 approval 等待，question 仍需真实答案；full/deny/once 竞态只执行一次，关闭后重新门控；can_mutate 来自真实设备 grant |
| A19 | 再授权后撤销 | always/revoke/always 后公开 rule ID 仍可撤销，随后同命令再出现审批且没有自动副作用 |
| A20 | 可变执行目标 | 解释器脚本、wrapper、compound 不因首个绝对程序 hash 获得不完整的永久匹配；身份未固定时 can_always=false |
| A21 | 可读详情与 ack | 普通危险操作的实际目标和参数可辨认；敏感值脱敏；分页读齐且ack绑定确切pending才能once/always，过期/错scope/错fingerprint不接受 |
| A22 | Agent 输入竞态 | pending 创建后另一个合法 Agent 改输入draft，旧结构化grant不沿用改变前的inputproof；人工和Agent输入版本均有覆盖 |

## 验收设施和证据边界

- 独立新增的攻击数据放在 `command-cases.json`，只作为策略/集成验收输入，不是可执行脚本，不应直接在用户终端运行。
- 可复用 fixture 的实际路径是 `crates/desktop-cli/examples/account_demo.rs`，不是分派文本中的 server 路径；现有 Android runner 是 `scripts/test-android-agent.py`。协调者已将两者唯一编辑所有权交给此 review 子任务，Android 测试源仍归 Android 执行者。
- Rust 新增独立 integration test 可置于 `crates/desktop-cli/tests/authorization_review.rs`，通过临时 state-dir、本地确定性模型和 PTY marker 观察行为；在接口与集成 commit 确定后编写，避免猜接口。
- 加密 RPC 使用临时 server/账号/配对设备/RemoteTerminal；模型和 MCP 仅 loopback/本地进程。每个 fixture 生命周期只清理自己的临时目录和进程。
- Android `emulator-5586` 是共享资源，执行前与协调者及 Android 执行者约定窗口。iOS 真 RPC 由 iOS 执行者协同提供结果。
- 阶段 1 已完成源码审查、攻击输入、通用确定性模型与 Android marker 验证 runner。初步通过 `cargo +stable check -p ai-terminal --example account_demo`、fmt、Python AST/JSON 校验与 diff check；这些只证明设施可构建，尚无授权行为/加密 RPC/模拟器通过证据。物理设备和线上供应商不计划验证。
- 故障恢复后沿用同分支，管理 run 更新为 `b2967528-01b4-4381-a7d1-454474ad4c69`。按协调者指定合并 toolset `c29f404` / `fe8d656` 与 policy 初版 `5eb063c` 用于提前审查；policy 可读预览 `900d1f2` 和最终 runtime 尚未合入。独立加密 RPC test 草稿已完成六个场景并 compile-check 通过；未运行这些授权行为测试。

## 恢复入口

fixture 所有权已确认；使用方法见 [FIXTURE.md](FIXTURE.md)。当前管理 run 为 `a290fb98-ccb2-445b-ba2b-424973ce62d0`；只能向当前 runtime 发直接任务消息，不唤醒已完成或排队执行者。待 root 确认 native 永久接口、runtime 提供最终 commit 并释放 Cargo 窗口后，合指定 commit、构建同版 example、显式运行全部 ignored 加密 RPC 测试。Android/iOS 真实 UI 由 root 在最多两个活跃执行者限制内安排。存在实质未修问题时保持 blocked/running，不报告 completed。
