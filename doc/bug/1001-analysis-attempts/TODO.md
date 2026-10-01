# 失败分析诊断 TODO

- [x] 持久化有界失败回复与每次 subtype，复用既有 record/history 合同
- [x] 验证两次失败重启可回读、UTF-8 上限、分析工具禁用和恢复不重放写操作
- [x] agent-runtime 完整测试、fmt、clippy 与最终 diff 审查
- [x] 记录诊断命令和验证证据，完成 Executor 上报

`cargo +stable test --locked -p ai-terminal-agent-runtime`：36 unit + 5 model_boundary 全通过。原始暂停仍保持 `observation_analysis_pending`，未接受无效分析；新增/扩展回归验证 invalid JSON、无效 TUI 行、违规工具、无依据事实四种 subtype，持久回读/访问隔离及有界回复；原写入与 read 恢复后都仅执行 1 次。

最终 fmt --check、clippy --all-targets -- -D warnings、git diff --check 通过。首次 clippy 的 sliced_string_as_bytes 建议已按安全字节切片修正，再跑完整 crate 测试通过。源码修改仅 `crates/agent-runtime/src/host.rs`。

后续授权：协调者报告本轮两次实际空 TUI 分析暂停并审查诊断 diff，要求提交。已用真实 TUI 行加强恢复回归；针对性回归、完整测试与 lint 检查后，提交 Host 和本目录必要文档，subject 使用 `[未Review]`；不 merge，不操作业务服务。
