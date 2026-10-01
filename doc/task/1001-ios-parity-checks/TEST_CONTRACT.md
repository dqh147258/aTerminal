# iOS 测试接口建议

协调者已认可标识命名并转给实现者，最终 API 稳定后测试跟随。标识不能包含 API key、endpoint、账号或原始配置 JSON。

| 区域 | 建议 accessibilityIdentifier |
| --- | --- |
| 工作空间 | 保留 `workspace.drawer/settings/chat`、`session.select.<id>`；新增 `workspace.global`、`workspace.search`、`workspace.close`；统一列表无 segmented selector |
| Global | `global.list`、`global.create`、`global.select.<id>`、`global.back`、`global.title`；Session/Global 使用同一 `chat.draft/send/stop`，页根 `chat.session`/`chat.global` |
| 设置首页 | `settings.home`、`settings.close`、`settings.llm/reading/mcp/skills/account`、`settings.reset`；保留显示滑块 accessibility label |
| 配置通用 | `settings.page` accessibilityValue 为非敏感 route；`settings.back/cancel/save/error/busy`；父页对应根 `settings.llm.page` 等；错误内容不含秘密 |
| 供应商 | `provider.select.<id>`、`provider.add`、`provider.id/protocol/endpoint/apiVersion/key`；已有 ID 只读；API version 整体条件显示 |
| 模型 | `model.select.<id>`、`model.add`、`model.id/provider/name/context/maxTokens/temperature/topP/advanced/reasoningMode/reasoningValue`；能力 `model.tools/vision/levels/budgetMin/budgetMax/adaptive/disabled` |
| 目录 | `catalog.open/search/submit/next/cancel`、`catalog.select.<id>`；查询改变时 next 隐藏；选择后回模型表单 |
| 绑定/读取 | `bindings.open/global/sessionDefault/current`；`reading.head/tail` |
| MCP/Skill | `mcp.import/json`、`mcp.select.<id>`、`skill.install/folder/id/path/body`、`skill.select.<id>`；详情通用 `extension.edit/toggle/delete`，确认删除 `extension.delete.confirm` |

## 建议注入点

1. 配置生产逻辑提取为 Foundation-only draft/store（可直接链接 standalone Swift checks，或 Xcode test target 直接访问），以 async transport closure 发起真实 action JSON；提供目标身份/连接 epoch supplier。draft 合成和迟到屏障必须生产实际使用，测试不另写 reducer。
2. `--workspace-fixture --settings-fixture` 只在 DEBUG 下用虚构 snapshot。transport 支持 script：成功、延迟、一次失败、revision conflict+show；记录非敏感 action/count，供 UI 观察 busy 和取消后迟到结果。支持 `--settings-fixture-scenario=save-failure/save-delayed/conflict/catalog`。UI fixture 的 credentials、defaults、history、AgentCache 全部隔离，不读写正常身份。
3. fixtures 有两个 provider（OpenAI/Azure）、两个模型、当前终端 + Global 两条消息/两份草稿、分页目录。unknown 字段 sentinel 放在 root/provider/connection/model/capabilities/binding/reading/MCP/Skill。fixture transport 不能网络调用。
4. deterministic 检查捕获 action JSON 并断言：空 key 的 secrets map 不含条目；candidate 保留 unknown；成功采用返回 snapshot；busy 单飞；失败非敏感 draft 保留；conflict 只 show，不自动再次 replace；取消/切页/目标身份或连接 epoch 变化后旧结果不能写入新页面。
5. catalog 测试通过受控 continuation 顺序返回 provider A/query A 与 B/query B 响应，断言迟到结果丢弃；同 ID catalog selection 也覆盖完整能力；翻页不带旧 query cursor。MCP/Skill transport contract 与完整资源保留应直接检查 production command/payload，资源实际哈希由协调者隔离 RPC 验证。

当前共享 Rust 使用 deny_unknown_fields，真正未知 sentinel 只能测 Swift 的 forward-compatible 合成，不发送真实 RPC。真实 RPC 应使用当前 schema 中 UI 未暴露字段，如 catalog_url/secret_ref/max_rounds/max_seconds/streaming/temperature/top_p，以及扩展凭据引用。

## 初始重大差异/风险

- iOS `ChatPanel` 有旧归档入口，`AssistantModel.legacy()` 会主动 importLegacy；需删除 UI/调用而保留用户文件。
- 旧 `AgentSettingsView` 无独立 binding 页面，Azure 非 Azure 保存仍可能发送残留 api_version，provider connection / reading 重建会丢 unknown。
- 旧配置 `perform` 只比 target，缺少页面/请求 serial 与 reconnect epoch；busy 无入口 guard；save 后再次 load，未采用返回 snapshot，也未支持 conflict 显式重试。
- UI fixtures 使用隔离 defaults，但普通 `AssistantHistory` cache；避免对用户历史产生 fixture 写入。
- 旧 LiveServiceUITests 会强制 logout，失败 assertion 拼接 login.error label；本任务将改为专用身份 fixture 且禁止输出秘密，不在本任务运行 simulator。
