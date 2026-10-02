# 当前恢复摘要

2026-10-02：用户已授权全部实现、明确不 Review 计划，三个问题已逐项回答。主计划为 In progress。协调者负责接口、Review与整合，六个 iTerm worktree 子任务正在执行；不能因协调者上下文恢复而重复创建。

- 任务：`aTerminal / 20261002-180220-295-agent-authorization`。
- 任务 CLI：`python3 /Users/carl/.codex/skills/worktree-tasks/scripts/worktree_tasks.py`。
- 名字、工作树、分支、当前 run、iTerm SessionID：见 [WORKTREES.json](WORKTREES.json)。Codex thread由各子任务报告绑定，不与SessionID混用。
- 主线基线：`f56461e`；规划文档提交 `3f8ebe0`。之后 main 只改规划文档和记录，无实现源码改动。
- 权威合同：[CONTRACT.md](CONTRACT.md)。已通过 subtask send 通知全部实现者：旧 allow_input=true 映射 ask，不保留危险动作直通；对话full跨Run且动态可关闭；严格cwd/参数/来源版本匹配；原始输入没有可靠上下文时不提供always。
- RPC已统一，can_mutate和永久不可用原因补充已转 runtime/两端；Full启用释放有效approval，questions仍等待真实回答。
- 策略模块API与三个工具helper hook见合同；runtime唯一拥有host.rs/store.rs/service/runtime.rs等共享父文件。纯策略只新增authorization.rs，工具执行者新增host/tools.rs/store/tools.rs/service/runtime/tools.rs并拥有shell.rs；不要互相覆盖父文件。
- Android拥有apps/android，iOS拥有apps/ios。独立review唯一拥有 `crates/desktop-cli/examples/account_demo.rs` 与 `scripts/test-android-agent.py` 共用fixture（初始prompt误写server目录，已纠正并通知）。
- 模型选择按worktree skill：六个复杂子任务均 `gpt-6-astra / high`。原始prompts/launch JSON在 `/private/tmp/aterminal-agent-authorization`。
- 最近已消费 inbox cursor=124；下一次 `task receive --timeout 0 --json` 自动读新消息，必要可 --after 124 重放。消息需要按具体内容处理，不对每个running状态自动回复。
- 可集成的早期提交：policy `5eb063c`（11策略测试，preview过度隐藏已派followup修正）；toolset `c29f404`（7 owned模块，runtime已合并，仍待接线/测试）；review fixture `b1eaf82`（account_demo --authorization-test 与脚本 runner，静态检查已过，真实权限行为未验）。不要把这些早期提交当成最终完成。
- root最近通知全部执行者：safe需要实际程序解析身份，不能只按bare命令名判断（aliases/functions/PATH）；runtime的shell字段与toolset dialect/host_input_boundary必须对齐；长审批增加approval_details完整脱敏详情，UI取齐后一次/永久。runtime负责stream.rs真实grant出站can_mutate。
- 移动设备时段：Android 18:40左右独立包 com.yxf.aterminal.authorizationfixture 已安装现有emulator-5586进行本地UI，暂不并发启动另一套Android前台测试；真实RPC类 AgentAuthorizationRpcUiTest、结果authorization-ui-results.json。iOS build-for-testing与主机控制器测试初过，XCTest和真实RPC仍待验。
- 18:22之前一次coordinator send被自动审批额度服务拒绝（并非不安全判定），18:37用户继续后重试成功。全部未送达消息已补发，当前没有该审批阻塞。
- 环境：adb只发现现有 `emulator-5586`；未确认物理设备。iOS沙箱服务访问受限，构建/模拟器可能需require_escalated，不视为测试通过。保护用户现有账号、终端、服务和旧worktree。

下一步：持续接收子任务早期API/设计问题并同步。策略/API稳定后通过Git合并接入runtime，工具helper同时接线；各端完成后集成main，由独立review做最终diff审阅与隔离真实RPC验收，执行计划High必要检查。用于集成源码提交保留worktree skill Pending Review前缀；Review通过需如实记录。完成后仅满足完整整合/主线测试/工作树干净等条件才清理新子任务资源，不能把完成状态当作自动清理授权的全部条件。
