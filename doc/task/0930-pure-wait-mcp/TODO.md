# 纯延时 MCP wait TODO

- [x] 实现 wait 工具目录、只读分类与 Broker 异步等待
- [x] 更新等待循环说明与部署文档
- [x] 验证真实 MCP、参数、无 Terminal 访问、只读授权、取消和 Run 总时限
- [x] Rust fmt、指定 crate 回归与 clippy 通过
- [x] 检查 diff，准备仅含本任务文件的 [未Review] Git 提交

## Verification evidence

- 新增 5 个 wait 用例通过，覆盖全局/Session 真实 MCP 只读调用、Broker 无 Terminal Host、参数边界与非法值、Run 取消、模型先消耗 600 ms 后共享 1 s 总时限。
- `cargo +stable fmt --all -- --check` 通过。
- `cargo +stable test --locked -p ai-terminal-agent-runtime -p ai-terminal-agent` 通过：24 个 Desktop 单元测试、35 个 runtime 单元测试、5 个 model_boundary 集成测试；doc-tests 通过。
- `cargo +stable clippy --locked -p ai-terminal-agent-runtime -p ai-terminal-agent --all-targets -- -D warnings` 通过。
- Desktop build、代码审查、集成和真实 OpenRouter / Android 验收由协调者负责；没有其他实现阻塞。
