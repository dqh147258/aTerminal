# 默认 Agent 终端稳定性与原路径恢复 TODO

- Status: Completed
- Updated: 2026-09-24

## Checklist

- [x] 修复坏帧错误粘连与 Resize 发布路径
- [x] 修复 CLI 对外部关闭会话的错误分类
- [x] 完成隔离 Agent 的回归和编译检查
- [x] 归档并切换默认旧 Agent，验证原命令和账号连接
- [x] 更新调试文档与验收记录并提交

## Verification evidence

- Rust 全工作区测试与 Clippy 通过；隔离 Zsh 的 `git status` 与外部关闭会话的 CLI 退出码 0 已验证。
- 默认 `.local/local-dev/agent` 的四条坏会话已私有归档并切换；原 CLI 命令执行 `git status` 后可继续输入。Android x86 真实局域网测试通过。
- 默认 Agent 停止后通过 `scripts/start-local-agent.sh` 恢复同一在线设备身份；完整记录见 `RESULTS.md`。
