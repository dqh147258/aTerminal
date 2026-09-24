# 默认 Agent 终端稳定性与原路径恢复

- Status: Completed
- Updated: 2026-09-24

## 目标与范围

修复默认 `.local/local-dev/agent` 的 CLI 异常退出，让用户原命令直接使用新版稳定 Agent。继续保留移动端 AI 占位与 Desktop/Mobile 并行输入行为。旧 Agent 中的独立 PTY 无法跨进程迁移；切换前保存可读取状态，不动项目文件或其它服务。

## 当前状态与证据

`target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent"` 仍连接 09:19 启动的旧 Agent（PID 22111），该进程运行的是 `5cf2475` 以前的实现。2026-09-24 只读 RPC 证实其中四条 `running` 会话全部携带 `invalid or oversized terminal frame`，而其最后已发布快照都能通过 `validate()`；`service.rs` 把任何一次候选快照失败写入持久的 `info.error`，因此客户端再附着仍立即退出。四个 Zsh 当前均无子进程。已把四条最后画面与 200 行历史存到权限受限的 `.local/local-dev/legacy-session-archive-20260924`。

另在真实并行输入验收中，移动端关闭会话时 Desktop CLI 把 Poll 竞争产生的 `session did not respond` 报为 Agent 连接中断并异常退出。`Operation::Resize` 也绕过候选快照校验直接发布，可能使客户端收到无效帧。

## 方案与执行

用户明确要求自行调试并修正，授权完成本地恢复。执行步骤：

1. Agent 只发布校验通过的候选快照；瞬时无效候选保留上一帧并记录一次诊断，后续有效帧可继续推进，不把显示错误写入终端进程的致命 `info.error`。Resize 同样经过发布校验。
2. CLI Poll 出错时只读查询会话列表；确认目标会话已被关闭则正常退出，仍存在或 Agent 不可达则保留原连接错误。
3. 在隔离 Agent 上运行帧、外部关闭、`git status` 和移动并行输入回归。然后停止默认旧 Agent（四条坏会话将结束），以新二进制在同一状态目录启动，验证原命令、账号在线状态和 Android 经 `192.168.0.36` 访问。
4. 更新本地调试文档，清理临时诊断工具，提交代码和验证记录。

执行已完成：默认状态目录已运行新版 Agent，原命令、Android 真实服务与重启后账号恢复均通过。细节见 `RESULTS.md`。

## 验证

Rust 单元/集成测试、格式和 Clippy；独立 PTY 的真实 `git status`、CLI 外部关闭；默认状态目录的启动/附着；Android x86 设备连接真实本地 Server。iOS UI/共享移动代码没有改动，前轮 iOS 真实服务验收仍有效。

## 风险与回退

切换旧 Agent 会结束四条已报错的 Shell，不能恢复其进程内状态；已确认没有前台子进程，并保存最后有效画面与历史。旧账号凭据在 OS vault；若新 Agent 在当前运行环境无法读取，则使用既有本地测试账号的私有文件存储重新登录，并核对手机可见的 Desktop 设备。测试用的 `Terminal Fix Desktop` 独立 Agent 保持可用作回退。

## 未决问题、歧义与确认

None.
