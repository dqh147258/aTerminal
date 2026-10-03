# Global Agent / Session Agent 工具与权限

当前内置目录：Session Agent 21 个，Global Agent 29 个。两者共享下表21个；Global另有8个委托管理工具。启用的 MCP 工具通过 mcp_tools/mcp_call 使用，用户 Skill 通过 skills_read/skill_action 使用，不计入固定内置数量。实际可用能力以 get_capabilities 和当前配置为准。

| 工具 | 用途 | 默认授权 |
| --- | --- | --- |
| list_sessions | 列出当前账号/Desktop 可见的终端 | 自动 |
| get_terminal_state | 读取进程、cwd、控制和完成证据 | 自动 |
| read_terminal | 读取 tail/search/screen/delta；过期或屏幕改写要求重新读取 | 自动 |
| read_record | 按 UUID 分页读取不可变证据与长结果 | 自动 |
| inspect_command | 支持的 pwd/ls/cat/head/tail/wc 语法转换为固定原生读取 | 自动；不支持的语法明确拒绝 |
| get_capabilities | 查询工具、视觉、Shell hooks、授权和共享剩余预算 | 自动 |
| wait | 有界等待，不判断任务成功 | 自动 |
| wait_terminal | 等待指定终端 revision 之后的更新 | 自动 |
| run_command | 在原 PTY 提交完整命令，返回确切 command_id | 一次或完全授权 |
| input_text | 原 PTY 文本输入及提交 | 一次或完全授权 |
| send_keys | 原 PTY 按键/组合键 | 一次或完全授权 |
| run_program | 绝对程序、字面参数、可选 stdin，在实际 cwd 原生执行 | 一次；可靠叶子程序可精确永久；完全授权可跳过审批 |
| get_command_result | 查询确切命令退出与不可变输出证据 | 自动 |
| wait_command | 等待确切 command_id；未知结果不当作成功 | 自动 |
| ask_user | 显示选项或自由问答，等待真实用户 | 自动提出问题；回答不授予操作权限 |
| search_history | 搜索保留历史/完整记录，返回事件和证据 UUID | 自动 |
| skills_search | 查找已配置的 Skill | 自动 |
| skills_read | 读取 Skill 和资源 | 自动 |
| skill_action | 内置动作或用户注册脚本 | 一次或完全授权；泛用脚本不支持永久 |
| mcp_tools | 查询用户已启用 MCP 的工具目录 | 自动；可能按既有配置启动服务 |
| mcp_call | 按已发现 schema 调用 MCP 工具 | 一次或完全授权；当前未知实现依赖不支持永久 |

| 仅 Global Agent | 用途 | 默认授权 |
| --- | --- | --- |
| get_agent_state | 查询指定 Session Agent 状态 | 自动 |
| send_agent_message | 委托 Session Agent，立即返回 task_id | 一次或完全授权；子任务副作用继续走来源对话授权和共享预算 |
| get_agent_task | 查询确切委托 Run 的状态与最终结果 | 自动 |
| wait_agent_task | 等待一个确切委托 Run | 自动 |
| list_agent_tasks | 分页列出委托任务 | 自动 |
| get_agent_tasks | 批量查询固定 Run 集合 | 自动 |
| wait_agent_tasks | 对固定集合等待 any/all | 自动 |
| cancel_agent_task | 取消确切委托 Run，不影响同 Session 的新 Run | 一次或完全授权；不代替终端 Ctrl+C |

Global 的终端执行/读取必须指明 session_id；Session Agent 固定绑定其 Session。get_capabilities/search_history 可按各自 schema 选择 Session。任务 ID、证据 UUID 和参数都不能作为绕过账号/Desktop/Session 权限的凭证。

默认 ask：常规观察与严格原生安全读取自动执行，其余操作由真实用户一次授权、精确永久授权或拒绝。完全授权只作用于当前 Agent 对话及其委托链，持续至关闭；它仍受账号/设备只读权限、Session 范围、人工抢占、版本、取消和预算约束。支持显式 read_only 模式。

永久规则可查询、撤销，并精确绑定原始参数、stdin、实际 cwd、工具来源与执行版本。PTY 里的同名函数/别名能截获绝对路径，所以 run_command/input_text/send_keys 不提供永久规则；可靠原生叶子操作使用 run_program。未知程序、解释器、MCP 或泛用 Skill 依赖无法固定时显示原因并只提供一次/完全授权。没有增加 OS 或工作目录沙箱。

权限开关、pending、审批详情、resolve、rules/revoke_rule 是真实用户 UI/CLI RPC，模型没有开启全授权、自批或代答的工具。协议见 [CONTRACT.md](CONTRACT.md)，操作示例见 [ASSISTANT.md](../../../deploy/ASSISTANT.md)。
