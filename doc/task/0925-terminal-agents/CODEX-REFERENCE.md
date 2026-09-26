# Codex 源码参考与 aTerminal 落地映射

- Status: Approved
- Updated: 2026-09-25
- Parent: [PLAN.md](PLAN.md)
- Source: `/Volumes/MacData/Data/DownloadCode/codex`
- Source HEAD: `4981037e99a322ee9cf29bc8730cb64d263fa00b`

本轮只读检查这份本地源码的相关实现和测试，未修改、编译或运行 Codex。下述结论针对该 commit，不把本地实现当成所有 Codex 版本的稳定 SDK。继续使用 Rig + aTerminal 自己的 Host/Broker，不把整个 codex-core/app-server 引为运行时依赖。

## 已核查的实现与采用方式

路径以下均相对于上述源码根；行号定位用于本次快照。

| 领域 | 源码/证据 | aTerminal 的采用方式 |
| --- | --- | --- |
| 任务入口与循环 | `codex-rs/core/src/session/turn_input.rs:1` 集中区分启动/追加/拒绝；`tasks/regular.rs:40` 持有取消 token，`session/turn.rs:532` 在模型步骤结束后判断 follow-up 与 pending input | 单一 Run admission，分别处理用户消息、PTY 状态和任务回报；用显式输入来源决定能否启动模型，步骤边界消费 mailbox |
| 请求绑定快照 | `core/src/session/step_context.rs:19` 同时持有不可变 settings、McpBinding 和 ToolRouter | 一个 RunSnapshot 绑定有效模型/强度、工具目录、连接、Skill 版本与权限；分析和主线从同一个快照构造请求 |
| 模型历史与来源 | `core/src/context_manager/history.rs:76` 使用 `Arc<Vec<ResponseItemEnvelope>>`，有独立 retained_context/history_version/user_message_revision；`context/internal_model_context.rs:64` 表达带来源的内部上下文 | 事件存档、运行事实、模型投影分别管理；PTY 状态不是普通用户授权；不可变快照与明确版本支持分析提交时保留后续输入 |
| 工具配对与稳定 ID | `core/src/context_manager/normalize.rs:18` 固定生成补充工具结果 ID；`history.rs:540` 删除旧条目时同步维护 call/output 配对 | 模型投影按完整消息单元变换；消息/工具结果 ID 首次生成后持久保存，恢复/重复投影不重新随机生成；未知 PTY 动作不伪造 succeeded/aborted |
| 缓存回归验证 | `core/tests/suite/prompt_caching.rs:133` 验证 tools 稳定；`:479` 的测试在 `:555` 比较后续请求的 cache key 和旧 input 前缀 | 检查真实序列化请求体：system、tools、参数和旧消息前缀相同，只有分析尾部指令新增；模型桩验证可缓存结构，不声称供应商实际命中 |
| 压缩与恢复 | `core/src/compact.rs:250` 从历史快照追加压缩输入并构造新 Prompt；`thread-store/src/local/model_context.rs:27` 从最近具备完整替换历史的压缩检查点恢复后续消息 | 明确压缩检查点、来源范围及新历史后缀；恢复只读最近可用投影检查点和后续事件，不每次展开全部 UUID 原文 |
| MCP 生命周期 | `codex-mcp/src/connection_manager.rs:83` 分离连接与调用超时，`:988` 取调用/服务器超时最小值；`connection_manager/tool_catalog.rs:47` 记录目录版本；`required.rs:13` 汇总必要服务器失败 | Registry 管配置，ConnectionManager 管连接，CatalogSnapshot 管本轮广告/调用的一致性；选中能力按需连接，非必要 server 故障不阻塞内置 Terminal |
| Skills 格式与渐进加载 | `skills/src/model.rs:10` 的 metadata/policy/dependencies；`parser.rs:66` 校验 name/description；`ext/skills/src/loader/discovery.rs:17` 有发现范围/数量/并发上限；`ext/skills/src/render.rs:17` 有目录预算和稳定排序 | 分离 SkillPackage、来源发现、目录投影、正文/资源读取和脚本执行；名称相同以来源区分，目录大小有上限，正文按需加载 |
| 模型目录与身份 | `model-provider/src/models_endpoint.rs:43` 限制刷新时间/响应大小；`models_identity.rs:1` 按路由与有效认证构建目录缓存身份 | 目录缓存不能只用 Provider 显示名作 key；绑定账号、协议、端点/目录地址、路由参数和凭据版本，修改端点/账号后失效，不落明文密钥 |
| 模型/强度交互 | `app-server-protocol/src/protocol/v2/model.rs:119` 返回支持的强度/默认强度，`:178` 带分页游标；`tui/src/chatwidget/model_popups.rs:234` 先选模型再选择其强度；`session_model_selection.rs:1` 区分 Session 选择和持久默认 | Desktop 统一返回可用强度和描述，CLI/App 使用同一份能力数据；交互式选择模型→强度，清晰显示全局默认/Session 覆盖 |
| 历史分页 | `thread-store/src/local/thread_history/read.rs:31` 的游标含 thread ID、位置和 scope；`:99` list_turns 先验证读取范围 | App 50 条分页游标包含账号/设备/Session、数据种类、边界和 generation，不能把某 Session/列表的游标拿去读另一个 |
| 配置管理 | `app-server/src/config_manager_service.rs` 统一验证配置修改，拒绝只读要求项，通过原子写入服务落盘；相关 service_tests 覆盖 expected_version | CLI 与 App 统一 ConfigService；内置只读规则在服务端执行，不能只禁用 UI 按钮；采用 revision 冲突检查和原子提交 |

以上源码目录中的 `core/...`、`skills/...`、`thread-store/...` 等简写均指 `codex-rs/` 下对应路径。`AGENTS.md` 中的一些路径描述已落后于当前目录，例如 MCP manager 实际路径为 `codex-mcp/src/connection_manager.rs`，查阅以实际源码为准。

## 对现有计划的具体补强

1. **输入来源与显示 role 分开。** ContextEntry 包含稳定 ID、origin、root_user_message_id、source artifact refs 与消息内容。只有 Host 接受的 UserMessage 可启动根 Run；PTYStatus、ObservationAnalysis、AgentReport、SkillResource 均不能因映射成 user role 就获得该权限。元数据由 Host 注入，不从终端文字或模型返回反序列化成授权。
2. **冻结实际执行能力。** RunSnapshot 不只保存 revision 数字，还持有配置/工具映射/Skill 包版本的实际不可变引用。模型看到的工具与执行选择一致；被撤销的权限立即生效，不能因快照引用存在继续执行。用户配置更新下一 Run 生效，不复制 Codex 在步骤之间动态切换模型的产品行为。
3. **区分三种历史状态。** 持久事件保留原始事实；retained facts 保存未决动作、当前目标/约束与最新状态；model projection 保存发送给模型的有界消息。追加事件序号、真实用户输入版本、投影压缩版本分别计数。分析提交只变换已捕获的目标记录，合并/保留后续消息；后台新增状态不应被旧分析覆盖，也不让持续状态更新造成无限重试。
4. **稳定而可验证的投影。** 工具输入/结果整体配对，原文替换不改变 call_id；映射结果和补充错误消息有持久稳定 ID，不能每次构造请求都产生新的随机 ID。缺失结果结合动作账本报告 cancelled/failed/unknown，不为满足消息协议重放 PTY 输入。
5. **目录与能力快照有预算。** Skills 元数据、MCP 目录、Provider 模型目录都有字节/条数/超时限制；稳定排序、按需加载。内置入口预留空间，省略的用户能力能通过搜索/分页发现；省略目录描述不等于停用/删除内置能力。按本项目实际 token 预算设上限，不照抄 Codex 所有默认数值。
6. **缓存需要请求级测试。** 在主线→读取→分析→原文卸载→继续的集成测试中抓取请求体，比较最长未改变前缀、工具 schema、配置与 ID。将预期压缩导致的前缀变化和意外重排区分；真实 cached usage 只作供应商返回的运行指标。
7. **启动/恢复只扫描必要状态。** 原子保存投影检查点及 covered_event_seq，再回放其后事件；原文/截图仍按 UUID 懒读取。App 分页游标带类型和 scope，清理 generation 变化明确失效；Terminal 内容锚点与 App 历史分页游标是两套不同合同。

## 明确不照搬的部分

- Codex 是完整产品，其 turn_input 有 User/Automatic/Recovery 启动种类，loop 还处理 hooks 等；aTerminal 仍只允许真实用户消息发起新工作，PTY 变化、恢复、MCP 通知不能触发模型。
- Codex 的 StepContext 允许在采样步骤之间变更设置；aTerminal 当前约定同一 Run 的模型、思考强度、工具配置固定，新设置下一用户 Run 生效。
- 本地 `compact.rs` 虽然从历史追加提示，但通过 `Prompt { input, base_instructions, ..Default::default() }` 重构 Prompt，不能据此断言它与主线 tools/config 完全一致。aTerminal 的记录分析复用主线请求构造器，不照搬这个压缩调用入口或远端专有 compact 协议。
- ContextManager 的 normalize 可以为缺少结果的工具调用补 `aborted`；aTerminal 的 PTY 写操作可能已经发生但没有回执，必须依据动作账本保留 unknown，不能机械补成 aborted。
- Codex Skills parser 有默认 name 和有限 YAML 修复逻辑；aTerminal 保持对新导入包要求明确 name/description 和可定位错误，不为“兼容”加入无证据的静默修复。
- Codex 的持久 JSONL rollout/SQLite 投影中的位置是不可变档案位置，不是可重绘终端行号。aTerminal 仍采用 tail 无锚点、search 必须开始锚点、首尾各 10 条非空原句与固定 ReadView，不能用 rollout 的位置设计替代 Terminal 内容搜索。
- Codex provider、UI、MCP 和 Skills 与其产品协议、登录、权限、插件体系有依赖；不把复制几个模块称为通用多供应商 Agent SDK，也不导入 Codex 特定模型/套餐/自动升级行为。

## 代码复用与验证范围

优先复用设计、边界合同与测试场景，在本项目窄接口中实现。确需摘取小段通用实现时，单独记录源 commit/路径、保留适用的 Apache-2.0/LICENSE/NOTICE 和其他文件声明，先检查依赖；不批量复制工作区或样例 Skills。参考阶段仅产出源码与计划映射；本项目实现没有复制 Codex 业务代码或把 Codex crate 加到 Cargo 依赖。

参考阶段只做文件/符号核查，不运行 Codex 测试。本项目实施后使用自己的模型桩、假 MCP 与隔离 PTY 验证上述不变量，实际结果见 [TODO.md](TODO.md)。
