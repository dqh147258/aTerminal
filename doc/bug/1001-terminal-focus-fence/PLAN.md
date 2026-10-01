# 终端焦点通知不应抢占 Agent

- Status: Completed
- Updated: 2026-10-01

## 目标与范围

只修复启用 focus_reporting 时合法窗口焦点通知错误增加 manual_revision、导致 Agent 被人工抢占的问题。保留协议发送、input_seq、control、presence 和 Agent 写授权/取消检查；普通键入、粘贴、鼠标及真实 resize 仍抢占。仅改必要 Desktop/CLI 与测试文档，不改 agent-runtime host.rs/model/store，不操作模拟器、现有服务或 Downloads。

## 当前状态与证据

`crates/desktop-cli/src/input.rs::encode` 在模式 bit 4 开启时将 FocusGained/FocusLost 编码为 ESC[I/ESC[O；`managed.rs` 通过 Operation::Input 发送。`crates/desktop-agent/src/service.rs::handle_session` 将所有非空 bytes 视为 manual_revision 增量；dispatch 调用 AgentHost::preempt，AgentAcquire/AgentWrite 也检查该 revision，因而焦点通知触发 manual_input_preempted_agent。

## 方案与执行

2026-10-01 任务明确传达本轮用户自由扩展阻塞工具与并行任务的授权，要求记录简短中文计划后直接执行，无需重复确认。本计划沿用该执行范围；先提供可审查 diff，待协调者审查并明确通知后才做 Git 测试同步提交。

1. 先加入隔离真实 PTY/session actor 回归：应用从 PTY 输出启用 focus_reporting，输入焦点通知并检查 revision、Agent fence、字节送达及去重；在旧代码上运行获得失败证据。
2. 最小修改 Input 分类：只有当前 Engine 启用 focus_reporting、且编码后 bytes 恰为完整 ESC[I 或 ESC[O 时不增加 manual_revision。保持既有发送与所有授权、序号、presence 校验位置。
3. 覆盖禁用模式时相同 bytes、普通输入/粘贴/鼠标/混合或畸形 focus、真实 resize 的抢占，以及焦点输入的序号/epoch/control/presence 与 Agent gate 检查。CLI 加 focus 编码路由回归。

实现与自动验证完成：生产代码仅 Input 分类条件改变，写入/commit/鉴权/取消路径保持原位。2026-10-01 协调者确认已 review 最小生产 diff 与真实 PTY/actor 回归，明确要求仅提交 service.rs、input.rs 和本目录必要文档，subject 使用 `[未Review]`，供 main 正式 Git 测试同步。本轮遵照该指令提交，main 同步由协调者执行，不 merge；main 的 lib.rs 模块排序修正由协调者独立处理，不纳入本提交。

## 验证

先运行新增 actor 回归确认旧逻辑失败，再完成修正并运行相关 crate 测试、workspace fmt --check、Desktop/CLI clippy --all-targets -D warnings 与 git diff --check。测试仅使用新建临时 PTY/actor，不启动或连接已有服务；真实 Android→Codex 复验由协调者另行安排。

已验证：

- `cargo +stable test --locked -p ai-terminal-agent --lib`：29 passed。沙箱首轮 28 passed、已有 cwd 回归因 ps 被禁止失败；允许隔离测试 ps/lsof 查询后 29/29。
- `cargo +stable test --locked -p ai-terminal --bin aTerminal`：14 passed。
- `cargo +stable test --locked -p ai-terminal --test agent --test desktop_pty`：Agent 11 passed、1 原有真实模型测试 ignored；Desktop PTY 2 passed。隔离临时服务/回环 mock/PTY，不操作现有服务。
- `cargo +stable clippy --quiet --locked -p ai-terminal-agent -p ai-terminal --all-targets -- -D warnings`：通过。
- `rustfmt +stable --edition 2024 --config skip_children=true --check crates/desktop-agent/src/service.rs crates/desktop-cli/src/input.rs`、`git diff --check`：通过。
- `cargo +stable fmt --all -- --check`：仅 HEAD 已有 `crates/desktop-agent/src/lib.rs` 的 recent_directories 模块顺序差异。对 HEAD 文件单独 rustfmt 同样重现；本轮不扩大修改，细节见 HANDOFF.md。

## 风险与回退

该例外只针对精确完整的 focus 协议 bytes，并以应用当前模式为准；多个事件、附带文字或无启用模式均保持人工抢占。撤销 Input 分类条件即可回退，不改协议/schema。

## 未决问题、歧义与确认

None.
