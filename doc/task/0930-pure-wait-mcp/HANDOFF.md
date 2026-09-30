# 纯延时 MCP wait 交接

Rust 实现与自动化验证由本子任务完成。协调者负责代码审查、集成、Desktop build 与已批准的真实 OpenRouter / Android 模拟器验收；此交接不阻塞实现交付。

- 新工具 `wait({"duration_ms":1000})` 从全局和 Session 工具目录经过既有真实 in-memory MCP Gateway 到 Desktop Broker，只返回实际 `elapsed_ms`。
- 明确整数范围 1–30000，缺失、错误类型、额外字段和越界报错。默认 TerminalBackend 和 Broker 都按只读工具授权；不查询/操作 Terminal，不形成完成判断。
- 取消当前 Run 会结束等待；等待使用 Run 剩余总时限，模型先前耗时不会被重置。旧 `builtin/wait-terminal` 执行分支未修改。
- 工具描述、等待 Skill、Agent INSTRUCTIONS 和 `deploy/ASSISTANT.md` 已说明 wait → 读取状态/输出 → 未完成继续循环，遵守取消/预算，不从静默或提示符猜完成。

自动化证据见 `TODO.md`。代码尚未经过协调者审查，commit subject 使用 `[未Review]`；没有合并、重启服务、调用真实供应商、修改 Android 或读取用户凭据。

真实验收建议：在更新后的全局和 Session Run 观察 `wait` 工具调用与 `elapsed_ms`，等待后回读 Terminal 证据；在一轮等待期间停止 Run，应及时显示 cancelled。耗尽总时限应停止循环。Codex 的应用任务状态仍可为 unknown，完成必须来自任务产物与可靠输出证据。
