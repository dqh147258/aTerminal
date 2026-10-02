# 按 task_id 查询和等待 Session Agent

- 状态：Completed
- 日期：2026-10-02
- 授权：用户要求确认缺失后补充查询结果、等待任务完成工具。仓库 AGENTS.md 为空，起始工作树干净。

## 目标与约束

Global Agent 能用 send_agent_message 返回的 task_id 查询指定委托运行的状态、最终答复和错误，或在当前 Run 内等待其结束。现有 task_id 等于子 Run ID，同一根任务的追加消息沿用该 ID。查询旧任务不能误返回 Session 后续任务；完成只表示 Agent 运行结束，不证明终端应用任务成功。

工具采用 get_agent_task(task_id)、wait_agent_task(task_id, timeout_ms)。两者只读且仅供 Global Agent 使用，限制在同账号、同 Desktop 的委托任务。timeout_ms 显式整数 1–30000；超时返回仍未结束的状态和 timed_out=true，不取消子任务；当前 Run 的取消和总时限仍中止等待。

## 实现方向

- host.rs::terminal_tools / Desktop Broker 注册和路由工具，沿用实际内存 MCP 通道与只读授权。
- store.rs 通过委托账本和 Run ID 定位任务；结束时保存错误与确切最终 assistant 记录引用，复用历史存储/分页/保留规则，不把最新 Session 答复当成旧任务结果。
- AgentHost 提供有界查询、运行中状态与取消感知的异步等待；用状态变化通知避免模型反复轮询。结果正文有界，较长正文通过 read_record 回读。
- 更新内置 agent-control Skill 与 deploy/ASSISTANT.md。

## 验证安排

采用 Medium 强度。必要验收为相关 Rust 单元/集成测试（包含真实内存 MCP、正常完成、旧任务、新 Run、重启、结果清理、归属隔离、超时、取消及总时限）、cargo fmt 与相关 crates clippy。不执行真机或真实供应商测试：改动在 Desktop 的工具与持久化层，不改变手机 UI；自动化验证使用本地模型桩，不调用付费模型。

## 进度

- 已确认现有 get_agent_state 只按 Session 返回当前/最近 Run；wait 仅延时；缺少任务查询/等待。
- 已实现两个 Global 只读工具，完成 Desktop Broker 路由、真实内存 MCP 调用及 Skill/使用文档更新。
- assistant 记录保存所属 Run ID；委托结束时在同一事务保存状态、错误和确切答复引用。增加任务/Run 检索索引；正文按 UTF-8 边界截断，可用 read_record 回读。等待订阅完成通知，并响应调用方取消和既有总时限。
- 新增 9 个用例覆盖：精确任务结果、后续 Run、重启及历史清理、账号/Desktop/Session 隔离、正常完成通知、超时、取消、孤立任务、模型错误持久化、参数校验、只读 MCP 调用，以及终端已关闭时的 Broker 查询。
- `cargo +stable test --locked -p ai-terminal-agent-runtime -p ai-terminal-agent`：88 个测试通过（Desktop 30、Runtime 53、协议集成 5）。沙箱内首次发现已有工作目录测试依赖被禁止的 `/bin/ps`；沙箱外重跑全部通过。新增 Broker 测试的临时目录权限已修正。
- 最后增加 assistant Run 索引并保留原错误优先级后，重跑 `cargo +stable test --locked -p ai-terminal-agent-runtime`：58 个测试通过。
- `cargo +stable clippy --locked -p ai-terminal-agent-runtime -p ai-terminal-agent --all-targets -- -D warnings`、`cargo +stable fmt --all -- --check` 和 `git diff --check` 均通过。

## 交付边界

本次为代码实现与 Medium 自动化验证，没有执行真机或真实供应商测试，没有替换/重启本机正在运行的 Desktop 进程。更新 Desktop 二进制并重启后，新 Run 会获得工具目录。旧版本任务未记录结果引用或历史已被清理时，明确返回结果不可用，不猜测其他 Run 的答复。

## Review 修复与模拟器验收（2026-10-02）

用户已授权修复 Review 的两个 P2、启动模拟器本地测试，并在测试通过后提交。上述交付边界为首轮实现记录；本轮继续完成：

- 将持久化/返回的错误诊断限制为 UTF-8 安全的 2048 字节，明确 error_truncated；过长错误不能回滚 Run 的终态或保留旧 pins。
- 在同一 Store 临界区内读取并保护完成报告与最终答复，绑定当前 Global Run；清理保留活动引用，Run 结束后释放。
- 增加两个问题的回归，重跑相关 Rust 测试、fmt、clippy。
- 在 Android 16 SDK 模拟器上使用已有隔离账号/Desktop/SSE 模型测试设施，补充 Global composer → 委托 → 查询/等待 → 结果回读的实际加密 RPC 验收。模型为本地确定性桩，不代表真实供应商实测；不测试物理设备。
- 保存模拟器报告与截图，审查本任务差异后提交；保留用户现有账号、终端与其他设备。

以上工作已完成，验收与复跑方式见 [RESULTS.md](RESULTS.md)。最终 Rust 90 个测试通过；Android 16 的加密端到端测试通过，成功回读清理后保留的两页结果，并独立核对长错误任务的数据库终态与 pins 释放。fmt、clippy、差异检查通过。
