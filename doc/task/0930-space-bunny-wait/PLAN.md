# OpenRouter Space Bunny 与延时 MCP 功能验收

- Status: Completed
- Updated: 2026-09-30

## 目标与范围

在 aTerminal Desktop Agent 添加并使用 OpenRouter `stealth/space-bunny-alpha`，绑定本地测试账号的全局与 Session 默认模型。保留现有 ModelScope profile。提供内置 MCP `wait` 工具，只做延时，说明 Terminal 任务需要时间，可循环等待并回读结果。启动 Android 16 模拟器，从 Android Agent 入口驱动已附着的 Desktop Terminal，在 `/Users/carl/Downloads/Temp2026/Temp09/test-0930` 启动 Codex，生成可循环播放的“鹈鹕骑自行车”动态 SVG。

OpenRouter 用于 aTerminal Agent；被其启动的 Codex CLI 沿用本机既有供应商配置。测试目录当前为空，不删除或覆盖其他目录内容。开发与测试使用 worktree-tasks 协调；不关闭用户现有 Terminal 或终止其任务。

## 当前状态与证据

- `crates/agent-runtime/src/config.rs` 已支持 OpenAI Chat 兼容协议、自定义 endpoint、模型能力与默认绑定；无需另写供应商适配器。
- 已读取 `https://openrouter.ai/api/v1/models`：目标模型存在，context_length=1000000，支持 tools、reasoning_effort 与图片输入。思考使用 `provider_default`，能力以真实目录声明为依据。
- `.local/local-dev/agent-next` 和 `~/.aTerminal` 现有配置只有 `modelscope-qwen`；环境未发现 OpenRouter key，需要用户给出密钥文件路径或先在设置内录入。
- `crates/agent-runtime/src/host.rs::terminal_tools` 的内置工具通过 `builtin.rs` 的实际 MCP 会话执行。现有 `builtin/wait-terminal` 在 `crates/desktop-agent/src/service/runtime.rs` 轮询 Terminal revision 并返回状态，不符合纯延时要求。
- `deploy/LOCAL-DEBUG.md` 有真实 LAN Server/Desktop/Android 登录与构建流程；`scripts/test-android-agent.py` 提供指定设备的 UI 验证基础。
- 当前主分支 `main` 无已有本地改动；目标测试目录存在且为空。模拟器状态检查在 sandbox 内受到 `ps` 权限限制，后续通过授权的 emulator skill 正常执行。

## 方案与执行

2026-09-30 用户明确回复“按计划执行”，并提供 OpenRouter 凭据；批准本计划全部开发、配置与模拟器验证步骤。凭据只通过私有后端使用，不记录其内容。

1. 通过 worktree 子任务实现 `wait(duration_ms)`，范围 1–30000 ms，在现有 MCP 工具目录提供给全局与 Session Agent。只异步等待并返回实际等待时长，不读取或操作 Terminal，不据此判断完成。沿用 Run 的取消与总时限；在默认 backend 与 Desktop Broker 将其归为只读工具。
2. 更新工具描述、Agent 指令及 `builtin/wait-terminal` 说明：Terminal 执行可能耗时；调用 wait 后读取状态/日志，仍在执行则重复，直到有可靠完成证据、被取消或 Run 预算耗尽。原来的等待状态变化工具保留。
3. 添加纯等待、参数边界、取消与 Run 时限的关键自动化验证；运行受影响 Rust crate 的测试、格式及 lint/build。审查子任务结果，按 worktree skill 的 Git 同步要求集成必要改动，再启动更新后的测试服务。
4. 使用 `https://openrouter.ai/api/v1`、`openai_chat` 和原始模型 ID 创建 profile，通过既有私有凭据后端保存密钥。context_window=1000000，max_tokens=8192，max_rounds=100，max_seconds=1800，reasoning=provider_default。保存既有绑定用于回退，切换本地测试账号的全局与 Session 默认到新 profile。
5. 通过 android-emulator-control 启动或复用 `aiterminal_api36_test`，只操作返回的匹配 serial。安装/复用本项目 Debug App，经现有 LAN Server 登录并连接更新的 Desktop Agent。创建独立且保持 Desktop 附着的 aTerminal 测试 Session。
6. 从 Android Agent 界面提交授权任务，要求 Agent 在指定目录启动 Codex 实现动态 SVG，并循环调用 wait、回读 Codex Terminal 直到有完成证据。记录真实模型调用、MCP 调用、手机截图、Terminal 证据与文件产物。
7. 校验 SVG 的 XML、鹈鹕/自行车内容及循环动画，在浏览器核对画面与动画随时间变化。汇总测试结果、可复用命令与保留的模拟器/Session。

完成记录：Rust 64 项测试、fmt、clippy、Desktop build 和 Android 测试 APK 构建通过；真实 OpenRouter 只读 wait 返回 elapsed_ms=26。Android 真实 UI 发起任务，Codex 仅启动一次生成 23465 字节 SVG，退出码 0；3 次 wait 返回 10002/20001/10001 ms。首次终端分析校验暂停后，以手机输入的继续消息恢复，最终 instrumentation `OK (1 test)`。XML 与全部 13 个无限循环动画校验通过，Chrome 两时刻截图有 16507 像素变化。证据与限制见 `RESULTS.md`，截图/JSON 保存在 `evidence/`。

## 验证

- Rust 回归须证明 wait 经过真实 MCP、只读授权可执行、时间确实流逝、无 Terminal 查询/写入、取消及时生效、参数与 Run 时间预算有界。
- `cargo +stable test --locked -p ai-terminal-agent-runtime -p ai-terminal-agent`，`cargo +stable fmt --all -- --check`，受影响 crates 的 clippy 与 Desktop build；按实际 crate 名称核对命令。
- 真实 OpenRouter SSE 工具循环必须使用 `stealth/space-bunny-alpha`；目录声明与 mock 测试不能代替线上验收。
- Android 验收应观察选择的新模型、任务提交/状态/结果和 wait 调用证据；确认 Terminal 的 cwd 为目标目录且 Codex 真实运行。
- 最终 SVG 必须是 Codex 在目标目录生成的文件；XML 可解析，浏览器中可见鹈鹕骑自行车且存在重复动画。模拟器验证已由用户明确要求，不另询问是否需要测试。

## 风险与回退

OpenRouter 调用需要有效密钥，模型当前免费仍须真实请求验证。长 Terminal 任务会消耗 Run 的时限和轮数；wait 单次最多 30 秒，不无限运行。更新 Desktop binary 若需重启旧 Agent，应先检查活动 Session，保留用户任务；可使用隔离 Desktop 测试实例。回退可恢复原默认绑定并使用旧二进制；不删除已有供应商或历史。

## 未决问题、歧义与确认

None.
