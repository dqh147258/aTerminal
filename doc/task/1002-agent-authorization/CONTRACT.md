# 授权与新增工具合同

本文件是本任务接口与行为的权威来源。协调者可以根据实现证据修订并通知执行者；不把未经协商的分支接口当作最终合同。

## 对话权限与旧协议

- 新 Agent v1 请求显式携带 `permission_mode: "ask" | "read_only"`。新客户端默认 `ask`；完全授权保存在当前 `Scope.agent` 的 Desktop 记录中，作为 `full_authorization: bool`。UI 读取后显示实际服务端值，不只保存手机草稿。
- `set_permissions` 用户 RPC 修改当前对话的 `permission_mode` 或 `full_authorization`，带 `expected_revision`。冲突重新读取，不覆盖别的用户刚作的决定。`read_only` 时完全授权不启用。
- Global 委托沿用来源对话的授权上下文/共享根，不把完全授权永久写入子 Session 对话。独立用户启动的 Session Run 使用自身对话模式。关闭开关后尚未执行的动作重新检查，不撤回已写字符。
- 开启完全授权时唤醒该来源对话/委托链中仍有效的审批等待，实际动作前仍复核权限、版本与取消；不自动回答 question pending。关闭后下一次动作重新走策略，不能通过已缓存 full 值继续放行。
- 旧请求未带 permission_mode 时，`allow_input=false` 保持只读，`allow_input=true` 映射为 ask，绝不升级为对话完全授权或绕过逐项审批。旧客户端无法响应 pending 时，状态明确提示需要新版客户端或 CLI 授权；不能为兼容而直接执行危险动作。老 Desktop 返回不支持时新客户端清楚提示，不偷偷 fallback 到全授权。
- 账号/设备权限、模型 read_only、Session 所属、控制版本、Desktop attachment、取消和总预算高于全部操作授权。`full_authorization` 只绕过风险审批。

## 人类交互 RPC

尽量保持现有 `Operation::Agent` JSON envelope 和 `RemoteTerminal.agent` 通道，避免不必要的 FFI 变更。

| action | 字段 | 语义 |
| --- | --- | --- |
| `permissions` | 无 | 读取对话模式、全授权开关和 revision |
| `set_permissions` | `expected_revision`, 可选 `permission_mode`, `full_authorization` | 修改当前对话；仅真实可操作用户设备/本地 CLI |
| `pending` | 可选 `cursor` | 有界列出当前对话及其委托链可响应的未完成审批/问答 |
| `approval_details` | `pending_id`, 可选 `cursor` | 分页读取该动作完整已脱敏展示正文；仅用于用户查看，不重新执行 |
| `resolve` | `request_id`, `pending_id`, 审批 `decision: once|always|deny`，或问答 `answer` | 对确切 pending 作幂等回答；Host 验证类型、scope、fingerprint 与终态 |
| `rules` | 可选 `cursor` | 列出账号/Desktop 的永久规则，展示可读范围和版本 |
| `revoke_rule` | `request_id`, `rule_id` | 撤销规则，后续动作重新审批 |

响应形态已与执行者对齐：`permissions` 直接返回 `{permission_mode,full_authorization,revision,can_mutate}`；`pending` / `rules` 返回 `{items,cursor,has_more}`（cursor 为不透明字符串或 null）；`state` 增加同结构的 `permissions` 与 `pending`；`resolve` 返回 `{pending,duplicate}`，`answer` 为字符串（选项直接提交选项文本）；`revoke_rule` 返回 `{revoked,rule_id}`。错误沿用现有 RPC 的 Reply.error 字符串，稳定标识如 `permission_revision_conflict`，客户端冲突后重读。pending.options 为字符串数组。`can_mutate` 来自真实用户设备 grant（可操作设备/本地 CLI=true，只读设备=false），仅辅助 UI；Host/bridge 仍独立校验。不能把终端控制租约当成 Agent 管理权限。`always_unavailable_reason` 可选，用于明确永久授权不可用原因。

长详情具体接口已确定：pending 增 `requires_details` / `arguments_truncated`；`approval_details` 返回 `{pending_id,fingerprint,text,cursor,has_more,truncated:false}`，text 为已脱敏完整 JSON 的有界分页片段；`resolve` 支持 `fingerprint` / `details_ack`，需详情动作的 once/always 必须 details_ack=true 且 fingerprint 精确对应，deny 不要求详情。CLI 提供 approval-details 与 resolve --details-ack --fingerprint；详情 token 只关联用户展示，不增加模型执行能力。

pending 的公共字段：`id`, `kind: approval|question`, `state`, `agent_id`, `run_id`, `session_id`, `created_at`, `expires_at`, `title`, `reason`；审批另有 `tool`, `arguments_preview`, `cwd`, `fingerprint`, `can_always`, `rule_preview`；问答另有 `question`, 可选 `options`。参数预览有界、秘密脱敏，Host 内部保留确切动作及指纹。UUID 不能当权限凭证。

普通命令、文件路径和非秘密参数必须显示，不能把所有自由文本隐藏后要求用户批准。策略提供 `redacted_preview_with_secrets(&Value, &[String])->String` 与 `redacted_details_with_secrets(&Value, &[String])->Value`；Host 已知凭据参与脱敏，精确指纹始终使用原始参数。超过预览限额明确 `truncated/requires_details`，两端取得完整脱敏详情后才允许 once/always；deny 始终可用。详情/resolve 的具体确认字段由 runtime 执行者统一定义并通知两端，不能各端自行猜测。CLI 同样提供完整详情查看，非 TTY 不隐式批准。

请求在 Desktop 持久化，手机断线不丢失。等待期间不运行下一步副作用或无意义的模型循环。采用明确 suspended/waiting 状态并在取消时解除等待；工具调用额度不因恢复重新计数。共享预算不因答复重新开始：当仍有其他活跃子任务时，根任务原活跃时限继续生效；仅整个共享执行树都因等待人类而不可运行时暂停活跃计时。pending 独立 TTL 默认 24 小时。审批后在真正动作前再次核对凭据、fence、cwd、工具版本与取消。

Desktop 重启后的旧执行线程/未知动作不自动恢复或重放；旧 pending 标记 interrupted/expired，并保留可见原因。永久规则与对话开关仍保留，下一条真实用户任务重新取得执行上下文。不要把旧 pending 的 approve 作为启动新 root 的途径。

一次授权只消费指定动作；always 仅在可形成稳定精确指纹时可选。拒绝记录本次动作，模型获得 `authorization_denied`；重复相同拒绝动作在该 Run 返回拒绝，不重复打扰用户。问答答复是明确用户输入，作为工具结果返回，不把其他报告升级为用户授权。

## 策略模块与集成责任

`crates/agent-runtime/src/authorization.rs` 提供公开纯函数/类型，建议最小 API：

```rust
enum PermissionMode { Ask, ReadOnly }
enum Decision { Once, Always, Deny }
enum Risk { Safe, RequiresApproval, Forbidden }
struct ActionDescriptor { /* tool、完整 args、cwd、来源/版本、目标及证明信息 */ }
struct Assessment { /* risk、reason、fingerprint、redacted_preview、can_always */ }
fn assess(action: &ActionDescriptor) -> Assessment;
```

最终 Rust 字段由策略执行者尽早确定并通知 runtime。所有指纹对 JSON map 键顺序稳定、对参数值/数组顺序/cwd/相关版本敏感。稳定指纹需要账号/Desktop 及明确目标能力的绑定；未知 cwd/无法固定程序/脚本/MCP 版本时禁用永久规则。永久授权不得通过同名 server/tool 或更新后的脚本包继续扩大权限。

已确定的 Rust 字段：`ActionDescriptor { account_id, desktop_id, tool, source: ToolSource, source_id, tool_version: Option<String>, target, cwd: Option<String>, arguments: Value, execution_identity: Option<String>, shell_proof: Option<ShellProof>, permission_management: bool }`；字符串字段默认 String。`ToolSource = Builtin|Mcp|Skill|Unknown`，`ShellDialect = Bash|Zsh`；`ShellProof { dialect, at_prompt, input_buffer_empty, observed_revision, current_revision }` 只由 Host 的可信状态构造。`Assessment { risk, reason, fingerprint, redacted_preview, can_always }`，提供 `assess`、`stable_fingerprint`、`redacted_preview`。指纹不包含瞬时 revision，但最终动作执行必须再次检查 proof/fence。原始 `input_text`/`send_keys` 无可靠完整输入上下文时不提供永久授权；结构化命令才能形成可复验的永久规则。已识别的权限管理模型动作使用 Forbidden，full 也不能代替真实用户更改授权。

接线：Host 暴露 `AgentHost::ask_user(&ToolContext, Value) async -> Result<ToolOutput>`；工具 helper 的 `definitions(global)` 注册新工具；父文件通过 `#[path="host/tools.rs"] pub mod tools`、`#[path="store/tools.rs"] mod tools`、runtime 的 `#[path="runtime/tools.rs"] mod tools` 引入子模块。Broker 调用 `Backend::invoke_added(context,name,args) -> Result<Option<ToolOutput>>`，命令缓存 `tools::CommandTracker` 由父 Backend 构造。命令 ID 对 scope 和已归档结果的关联必须可持久恢复，缓存不能成为唯一结果来源。

Safe 范围：内置只读工具、纯等待、明确识别的简单安全命令。安全命令只能采用严格正向规则；复合 Shell、解析错误、用户脚本、未知程序、MCP 或原始 TUI 输入进入审批。不接受模型传 `safe:true`、tool annotations 或截图里的 Shell prompt 作为判定凭证。不增加 cwd 沙箱。

规则 cwd 使用 `process::cwd` 的进程身份复核结果，不回退到 hook 或 initial_cwd。Shell hooks 本身标记 trusted_for_authorization=false，只能与 Host/OS 状态共同提供观察；空输入行需要明确输入边界（最后人工/Agent 输入与 prompt 事件的关联或实际行编辑器适配），无法确认则 proof=false 并审批，不能照屏幕提示符猜测。

实际 Shell 的 aliases/functions/PATH 不能仅凭名字证明安全。为保证常规安全读取确实无需授权，新增 `inspect_command`：策略严格正向语法转换为固定绝对只读程序和字面 argv，Broker 在身份复核的当前 cwd 启动原生子进程（不经过 Shell、不改变 PTY draft/目录），归档 stdout/stderr/exit_code 并明确 source=sidecar_read。未知语法拒绝该读取工具，模型可改用正常 run_command 申请授权。这没有目录/OS 沙箱；它是可确定的读取执行入口。run_command 仍在原 PTY，不能用 command/builtin 字符串包装宣称解决任意用户函数覆盖。

永久规则的 execution_identity 必须覆盖整个实际执行语言：仅 hash 第一个绝对程序不足以放行解释器脚本、env wrapper、compound 或动态环境表达式。初版仅为可固定的单个程序、字面 argv 和必要固定字面重定向生成稳定规则；外部脚本/解释器/复合或可变目标无法证明时 can_always=false。同字面命令但程序/配置版本改变仍重新审批。

runtime 执行者负责 `lib.rs` 导出、Store persistence、Host gate、RPC、远程授权与 CLI；策略执行者仅新增自己的模块/测试，不编辑这些共享文件。工具执行者新增 helper 模块并尽早报告父模块要接的少量 hook；runtime 执行者负责接线，避免两个工作树全面覆盖父文件。

## 新工具及角色

现有工具继续可用，所有新工具也经过统一门控。

| 工具 | 参数概要 | 角色与行为 |
| --- | --- | --- |
| `inspect_command` | `command`, Global 必填 `session_id` | 两者；严格固定程序的常规安全读取，在 Session 实际 cwd 原生执行，无 Shell 解析，返回归档 stdout/stderr/exit_code；不改终端输入/目录 |
| `run_command` | `command`, Global 必填 `session_id` | 两者；在现有 PTY 提交完整命令，绑定输入状态和授权，返回 `command_id`/accepted，不能凭 accepted 宣称完成 |
| `get_command_result` | `command_id` | 两者；同账号/Desktop，Session 限自身；返回 shell 命令状态、退出码、cwd 和证据来源/引用；无关联证据为 unknown |
| `wait_command` | `command_id`, `timeout_ms` 1–30000 | 两者；取消/共享时限感知；超时不停止程序 |
| `list_agent_tasks` | 可选根/状态过滤、`cursor` | 仅 Global；有界列出合法委托任务 |
| `get_agent_tasks` | `task_ids`，有界数组 | 仅 Global；逐个返回确切任务结果，不读 latest Run 代替 |
| `wait_agent_tasks` | `task_ids`, `mode: any|all`, `timeout_ms` 1–30000 | 仅 Global；固定任务集合、事件通知、有界结果、超时不取消 |
| `cancel_agent_task` | `task_id` | 仅 Global；审批受控；取消确切 Run，不取消该 Session 后来启动的 Run，不发 Ctrl+C |
| `ask_user` | `question`, 可选 `options` | 两者；请求结构化用户输入，真实 UI/CLI resolve 恢复；有界文本，不代表额外操作授权 |
| `search_history` | `query`, 可选 Session/时间/类型、`cursor`、有界 limit | 两者；Global 同账号/Desktop，Session 仅自身；返回准确记录 ID/有界片段，分页与保留 generation 绑定 |
| `read_terminal` 扩展 | `mode=delta`, `after_revision` | 两者；明确变化/需要重取/unknown，不把可变 TUI 差分误认为追加日志，不改变已有 tail/search/screen |
| `wait_terminal` | Global 必填 `session_id`, `after_revision`, `timeout_ms` 1–30000 | 两者；等待比已观察版本更新或超时，随后按需读；变更不是完成证据 |
| `get_capabilities` | 可选目标 Session | 两者；返回实际角色、可用工具、模式/完全授权、视觉/Shell hooks/应用适配器、当前共享剩余预算及限制 |

凡 Global 面向终端的 schema 都要求 session_id；Session schema 不提供切换目标参数，兼容旧 Session 参数时必须等于绑定目标。工具数量以最终实际注册为准，新增别名必须有用途。

命令结果通过 Host 提交记录与会话 Shell hooks 的命令序列/文本证据关联，不能把下一次 prompt 或上一个退出码误配给新命令。命令结束不代表其后台子进程或应用任务结束。无 Shell hooks、相关证据过期、人工插入命令、关联冲突、仅进程存在等情况保持 unknown。初版实现可靠 Shell 命令关联；任意 TUI 的 application_task 保持 unknown，不伪造通用应用完成适配器。

## 跨端与恢复

Android/iOS 对当前会话读取并修改 Desktop 实际权限状态；完全授权开关默认关闭，不从旧手机 allowInput/草稿升级。审批和问答置于聊天中的持久卡片；一次/永久/拒绝可达，永久不可用时说明原因；等待期间可以停止 Run。切换对话/账号后不得把旧 pending 的响应送到新 scope。

CLI 提供同等 pending/resolve/permissions/rules 能力；`--json` 与非 TTY 不隐式询问或挂起。交互命令可显式等待并回答；所有回答仍通过同一用户 RPC。规则列表和撤销可以从 Agent 面板进入，不必另造整套设置中心。

旧设备/旧 Desktop 明确显示能力缺失。只读设备仍只能浏览；不能 approve、answer、改模式或撤销规则。权限管理 RPC 不作为模型工具暴露，已识别的模型权限管理 CLI 入口在执行策略中 Forbidden，永久规则/full 不放行它。这里不宣称 OS 级身份隔离：已授权任意程序与 Desktop 同 UID 时仍可访问该用户系统资源；本任务按用户要求不增加严格 OS 沙箱。
