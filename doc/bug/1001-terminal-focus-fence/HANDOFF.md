# 审查与复验交接

生产修正仅在 `crates/desktop-agent/src/service.rs::handle_session` 的 Operation::Input 分类：当前 `engine.focus_reporting()` 且编码后完整 bytes 为 ESC[I 或 ESC[O 时不增加 manual_revision。协议 bytes 仍写入 PTY，input_seq 仍接受/推进；control、presence、session epoch、去重和 Agent permit/cancel/fence 检查未改。CLI 仅补 focus 模式编码路由回归。

旧逻辑回归证据：隔离真实 PTY/session actor 接收 focus 通知后 revision 为 1（预期 0），AgentAcquire 返回 `manual_input_preempted_agent`。修正后 PTY 应用回报 `1b 5b 49 78 1b 5b 4f`，即 focus gained、授权 Agent 字符 x、focus lost；重复请求未多写。应用禁用 focus 模式后，同样 ESC[I/O bytes 各自增加 revision 并拒绝旧 Agent fence。普通键入、粘贴、鼠标、部分/畸形/混合/串联 focus 与真实 resize 仍人工抢占。

验证使用临时 actor/Host/PTY，未连接现有服务或操作模拟器/Downloads。Desktop lib 测试沙箱首轮 cwd 回归失败，确认 `/bin/ps` 被禁止；允许 ps/lsof 查询后全量 29 项通过。CLI bin 14 项通过。最终检查结果见 PLAN.md。

workspace fmt 基线问题：本分支原 HEAD 的 `crates/desktop-agent/src/lib.rs` 将 `mod recent_directories;` 放在 `pub mod pty;` 之前，rustfmt 要求其移到 `pub mod raster;` 之后。对原 HEAD 单独 rustfmt 同样报告差异。本修复保持该文件原样，改动文件的 rustfmt --check 通过；协调者已在 main 做独立最小 fmt 修正，不纳入本提交。

协调者已 review 最小生产 diff 与真实 PTY/actor 回归，确认符合授权，并明确通知只提交 service.rs、input.rs 与本目录必要文档，subject 指定 `[未Review]`。所有实现/自动验证项目已完成，提交完成后通过 worktree-tasks 报告 commit 与 completed，供协调者做 main 正式 Git 测试同步；本 executor 不 merge。

协调者若安排真实 Android→Codex 复验：应用已开启 focus_reporting、Agent 正在观察/写入时切换 Desktop 窗口焦点，预期焦点协议仍送达且不出现 `manual_input_preempted_agent`；普通键入/粘贴、应用鼠标输入、真实窗口尺寸变化仍取消 Agent。该真实设备复验不属于本 executor 的操作范围。
