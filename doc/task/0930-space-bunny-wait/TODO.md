# OpenRouter Space Bunny 与延时 MCP 功能验收 TODO

- Status: Completed
- Updated: 2026-09-30

## Checklist

- [x] 实现并审查纯延时 MCP wait 与循环等待说明
- [x] 受影响 Rust 回归、格式、clippy 和 Desktop build 通过
- [x] 通过私有凭据后端配置 OpenRouter profile 与默认绑定，并验证真实请求
- [x] 启动 Android 16 模拟器并连接更新的 Desktop 测试实例
- [x] 从 Android Agent 驱动指定目录 Codex 生成动态 SVG，保留真实 wait/Terminal 证据
- [x] 检查 SVG XML 与浏览器循环动画，记录验收结果并完成协调任务

## Verification evidence

- Desktop 测试账号配置 revision=8，profile=openrouter-space-bunny；global/session-default 均已绑定，既有绑定保存在忽略目录 `.local/space-bunny-wait/previous-bindings.json`。
- 真实 OpenRouter SSE 请求完成，返回 `SPACE_BUNNY_CONNECTION_OK`；证据 `.local/space-bunny-wait/live-preflight.json`。凭据通过 `--api-key-stdin` 写入现有私有后端，未写入项目文件或日志。
- Android 16 `aiterminal_api36_test` 在 `emulator-5586` 验证进程存活、ADB=device、sys.boot_completed=1；App 登录/进程重启恢复测试通过，`.local/space-bunny-wait/android-login/results.json`。
- 纯 wait 改动经过协调者代码审查，从 `worktree/0930-pure-wait-mcp` 的 `c314d91112b5f557dee2a1f6b16a7b08049b5f24` 快进集成到 `main`；历史提交保留 `[未Review]` 标记。worktree 上 24 + 35 + 5 项测试、doc-tests、fmt 和 clippy 已通过；Desktop build 进行中。
- Codex 命令依官方资料 `https://learn.chatgpt.com/docs/non-interactive-mode` 使用 `codex exec --sandbox workspace-write --skip-git-repo-check`，权限仅用于本次生成文件；供应商/模型沿用本机配置。
- Desktop build 通过；Android `:app:assembleDebugAndroidTest --offline` 通过，手机原始登录身份保留。
- 真实 Android 任务首次在终端回读分析校验暂停，后续从同一 Agent 输入框提交恢复消息，保持原 Codex 进程，最终 `OK (1 test)`；已保留初次失败及恢复证据，不把首次尝试标为通过。
- Codex 产物与实际 wait/工具链路见 `evidence/android-workflow.json`。Chrome 预览完成加载，两个时刻有 16507 像素变化，全部 13 个 SMIL 动画为 `repeatCount=indefinite`；见 `evidence/svg-verification.json`。
