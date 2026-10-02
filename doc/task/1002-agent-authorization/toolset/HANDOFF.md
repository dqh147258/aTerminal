# 工具 helper 当前接线与验证

状态：Completed（owned helper/docs范围），已释放Cargo窗口。复用已批准主计划；用户不 Review 计划。

## 已提交实现

顺序：`c29f404` → `fe8d656` → `5ddf090` → `b81107f` → `c604b8e` → `28d5700`。每次只提交本任务 owned source；父文件由 runtime 接线。

- `host.rs` 引入 `#[path="host/tools.rs"] pub mod tools;`，在 `terminal_tools` 返回前调用 `tools::extend_catalog(&mut result, global)`，只读判定复用 `tools::is_read`。`ask_user` 由 Host 人类等待路径截获。
- `store.rs` 引入 `#[path="store/tools.rs"] mod tools;`。子模块使用现有 db/key 和签名游标；命令表惰性创建，command_id/owner/Desktop/Session/结果/证据事件持久化，并随事件 retention 清理，后续 Run 可读。没有内存 CommandTracker 字段。
- `service/runtime.rs` 引入 `#[path="runtime/tools.rs"] mod tools;`；旧 match 前调用 `invoke_added(&context,name,args.clone()).await?`。能力输出的 authorization 直接读取 `AgentHost::permission_capabilities(context)`，沿用来源对话权限。
- `Backend::write` 真正 guarded PTY 请求前调用 `store.invalidate_command_writes(&self.scope,id,&context.action_id)`；覆盖 raw text、keys、其他 Agent/Run 写入，人工输入另经 manual_revision 使关联 unknown。
- `Backend::inspection_context(&self,&ToolContext,&str,&Path)->Result<ToolContext>` 由 runtime 提供；helper 取得 OS cwd 后调用，构造身份/cwd 复核用观察许可。`process::inspect` 在已有 execution_gate 内仅 `commit_authorization(None)` 一次，再 native spawn；不能持 gate 调用会重入同一 mutex 的 `check_authorization`。原生读取不走风险审批。
- 精确任务取消在确切当前 Job/Run 的 gate 内 check/commit(None) 一次；不调用按 Session 取消的方法，不取消新 Run，不发 Ctrl-C。

实际公开 helper：`AgentHost::{list_agent_tasks,get_agent_tasks,cancel_agent_task,search_history}(&ToolContext,Value)->Result<ToolOutput>`、async `wait_agent_tasks`；`Budget::tool_status()->Result<Value>`；async `Backend::invoke_added(&ToolContext,&str,Value)->Result<Option<ToolOutput>>`。

## 行为与限制

`inspect_command` 使用 policy `5b89a2d` 的 `InspectCommand {program:String,args:Vec<String>}` / `inspect_command_plan(&str)`，严格固定 native 程序、literal argv，env_clear、stdin Null；OS 进程身份确认 cwd，30 秒硬上限，stdout≤max_read_bytes/64KiB、stderr≤8KiB，明确 truncation，取消杀进程组。Observation source=`sidecar_read`，不改变 PTY 输入/目录。

Shell hooks 返回 sequence/完整 command/instance/dialect/command_association，仍是 observations。`process::cwd` 与 `shell_foreground` 做 OS 身份佐证，不用 hook cwd 作永久规则。**mtime 晚于输入不能证明空 draft/typeahead**：Actor 仅零输入的初始 prompt，或已知完整提交后确切 sequence/command 的 prompt 才恢复 empty；未知 raw 输入保持 false。runtime 负责 Actor 瞬时 input_revision 及最终消费复核。此方案替代早期仅比较 mtime 的建议。

只有精确提交/序号/文本/prompt/退出码一致才 completed。无 hooks、已有 bash DEBUG trap、历史重写、人工/其他 Agent 插入、冲突或不可关联 PowerShell 命令保持 unknown。PowerShell alias/function/dynamic/background/compound 不复用上一条 native 退出码。

批量任务固定≤32确切IDs，any/all、不因超时取消。wait_agent_tasks登记依赖等待，timeout以共享活跃时间扣除整树人类暂停。历史搜索完整分块，单页最多256KiB/128steps，游标绑定 scope/filter/generation/watermark/chunk；空页 cursor+scan_incomplete 不代表无匹配。delta 对可变 grid 变化要求重取，不构造 append 日志；wait_terminal after_revision 包含等待前更新。

未知应用任务仍 unknown，没有万能 Codex/TUI 完成适配器。Unix OS cwd 已支持；缺身份/Windows cwd 未支持时保持不可用。此机未安装 pwsh，PowerShell 没有实测。没有新增目录/OS沙箱，没有调用付费模型或改用户 Desktop/账号/终端。

## 最终本子任务证据

实际runtime父模块39fcb54通过Git同步到已批准隔离scratch；policy5b89a2d；工具源码最终28d5700。执行：

- `cargo +stable test --locked --jobs 2 -p ai-terminal-agent-runtime -p ai-terminal-agent --no-default-features --lib --target-dir ... toolset -- --test-threads=2`：22/22通过（Desktop13/Runtime9），包括实际native observation单次commit/旧permit拒第二spawn/cwd复核拒绝、原生输出限额/取消、真实bash/zsh/PTYShell结果、Broker精确command→completed/exit1/持久读取、等待前更新/超时/取消、nonvision能力、history分块/边界/保留、固定批量/跨账号拒绝/旧Run取消。
- 同样两lib `cargo +stable clippy --locked --jobs 2 ... -- -D warnings` 通过。
- `cargo +stable fmt --all -- --check`、owned文件 `rustfmt --check`、`git diff --check`通过。

Broker测试围绕真实临时PTY使用本地Actor桩（submit tests permit=None），不能作为真实Actor输入proof/客户端端到端证据；native观察许可用runtime真实实现验证。runtime自己的真实Actorproof/父回归由它交付，不将两套测试计数重复加总。PowerShell此机未安装，未运行实测；未知TUI application_task一律unknown。不调用付费模型、不改现有账号/终端/设备。

隔离Git测试副本 `/Volumes/Code/public-worktree/aTerminal/.worktree-tasks/toolset-check-0a94b6` 使用实际父接口，无测试fake成功。相关源码未在本assigned worktree改父文件。

部署使用说明已更新 `deploy/ASSISTANT.md`，包括safe native inspection、PTY审批、对话full、once/always/deny、CLI详情/规则撤销、六类工具与结果边界。root继续独立Review、main整合High和跨端验收；本子任务不等待这些协调者范围才报告completed。工作树/分支保留，不合main、不删工作树、不自行关闭终端。
