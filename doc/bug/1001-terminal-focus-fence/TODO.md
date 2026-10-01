# 执行记录

- [x] 隔离真实 PTY/actor 回归在旧代码失败，确认焦点通知增加 manual_revision 并触发 Agent fence。`cargo +stable test --locked -p ai-terminal-agent --lib focus_notifications_preserve_agent_fence_and_reach_real_pty -- --nocapture`：revision 实际 1、预期 0，AgentAcquire 错误为 `manual_input_preempted_agent`。
- [x] 最小修正与关键回归通过，保持协议发送、普通输入抢占和所有写入校验。Desktop lib 29 项、CLI bin 14 项通过。新增 actor 从真实 PTY 接收应用启用/禁用 focus 模式，回读焦点与授权 Agent 写入 bytes；覆盖去重、冲突/序号/epoch/control/presence 检查、Agent gate、普通输入/粘贴/鼠标/畸形 focus 和 resize 抢占。
- [x] 完成 Desktop/CLI 测试、fmt、clippy、diff 检查，提供可审查 diff 与验证证据。额外 CLI Agent 11 项 + Desktop PTY 2 项通过，真实模型测试原有 ignored；Desktop/CLI all-targets clippy -D warnings、修改文件 rustfmt、git diff --check 通过。workspace fmt 仅 HEAD 已有 lib.rs 模块排序差异，细节见 PLAN/HANDOFF。

- [x] 协调者已 review 并明确授权指定范围提交；subject 按指令使用 `[未Review]`。仅提交两个源码文件与 PLAN/TODO/HANDOFF，main Git 测试同步由协调者执行，不 merge，不纳入 lib.rs 独立 fmt 修正。
