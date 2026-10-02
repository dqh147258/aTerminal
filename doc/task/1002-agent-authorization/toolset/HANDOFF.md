# 当前接线合同

状态：In progress；模块可供 runtime 接线，测试尚在进行。

父文件由 runtime 负责：

- `host.rs`：`#[path="host/tools.rs"] pub mod tools;`；`terminal_tools` 返回前调用 `tools::extend_catalog(&mut result, global)`；`TerminalBackend::is_write` 对 `tools::is_read(name)` 返回 false。`ask_user` 由 Host 的人类等待路径截获。
- `store.rs`：`#[path="store/tools.rs"] mod tools;`。子模块访问现有私有 db/key/游标，命令表惰性创建，无需新 migration 字段。
- `service/runtime.rs`：`#[path="runtime/tools.rs"] mod tools;`；`invoke` 的旧 match 前 `if let Some(output)=self.invoke_added(&context,name,args.clone()).await? { ... }`。`get_capabilities` 的 `authorization:null` 由父填入实际授权源对话的 permissions，不能猜测默认。
- `Backend::write` 真正 guarded PTY 请求前 `self.host()?.agents.store.invalidate_command_writes(&self.scope,id,&context.action_id)?;`；此 hook 覆盖文本、keys、其他 Agent/Run 写入。人工输入由 manual_revision 使关联 unknown。
- `Backend::is_write` 也对 `ai_terminal_agent_runtime::host::tools::is_read(name)` 返回 false；cancel_agent_task/run_command 保持写动作。

实际 API：`AgentHost::{list_agent_tasks,get_agent_tasks,cancel_agent_task,search_history}(&ToolContext,Value)->Result<ToolOutput>`、`wait_agent_tasks` 同签名 async；`Budget::tool_status()->Result<Value>`；`Backend::invoke_added(&ToolContext,&str,Value)->Result<Option<ToolOutput>>` async。无 CommandTracker 字段：命令完全由 Store 持久记录，准确 owner/Desktop/Session 绑定、事件 retention FK，后续 Run 可查。

Actor 输入证明：对全部成功 PTY 写入维护单调 input_revision/last_input_at_ns（人工和Agent都算）。同一个 Actor 操作中读取 hook 状态，只有新 prompt 的 reported_at_ns 严格晚于最新输入、OS身份仍匹配且 foreground_group==process_id 才能附加 `host_input_boundary:{input_revision,last_input_at_ns,prompt_reported_at_ns,input_buffer_empty:true}`；未知/相同时 false。`process::shell_foreground(&SessionInfo)->bool` 提供 OS 佐证，`process::cwd` 提供独立cwd，不能回退hook目录。Hook的 `sequence/command/instance/dialect/command_association` 只作为命令结果观察。

当前 delta 保守报告 authoritative grid 已改变并要求 fresh tail；未知TUI/屏幕覆盖不会拼成追加日志。无应用完成适配器，任意 Codex/TUI application_task 一律 unknown。
