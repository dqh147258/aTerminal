# 生产测试接口（2026-10-01）

- `apps/ios/aTerminal/SettingsDraft.swift` 只 import Foundation；可直接与 standalone Swift main 编译，无 Rust/SwiftUI 依赖。
- `ProviderDraft(id:item:)` / `value(previous:)`，字段 id/protocolName/endpoint/apiVersion/key/enabled。
- `ModelDraft(id:item:)` / `candidate(_:editing:)`，`resetCapabilities()`、`select(_ catalogRow:)`。caps 和 capabilitiesFromSnapshot 用于完整能力覆盖与冲突刷新。
- `SettingsValidation`：id(_:kind:editing:items:)、endpoint、absolutePath、reasoning、mcp、skillFiles。SettingsFailure 提供 LocalizedError。
- `@MainActor ConfigurationSession(identity: () -> String, transport: ([String:Any]) async throws -> [String:Any])`，实际生产 SettingsStore 使用。identity 含 scope key + TerminalModel.generation；`load()` / `save(_:secrets:done:)` / `run(mutation:operation:done:)` 返回可 await 的 Task?，busy 时返回 nil；`call`、`close`；只读 snapshot/busy/loaded，error 和 changed 回调。
- 成功直接采用返回 snapshot；revision 错误 show 一次后保留表单，等待显式二次 save；close/identity epoch 改变丢弃回调。
- TEST_CONTRACT 建议 IDs 已基本采用；设置页 back=`settings.back`，catalog 返回也使用同一 back；page accessibilityValue 为中文标题。`settings.cancel/save/error/busy` 与 provider/model/catalog/bindings/reading/extension 字段采用建议值。
- `workspace.global`、`global.list/create/select.<id>/back`；`settings.home/llm/reading/mcp/skills/account/reset/close`；`chat.session/global/draft/send/stop`。
- DEBUG settings fixture 正在补齐，未稳定前不运行 UI。Workspace fixture history 已隔离至 FixtureAssistantHistory。

## 可用 fixture 与私有登录入口

- Foundation 文件已稳定为 `SettingsDraft.swift`（与之前通知一致，不另复制实现）；包含生产 `ConfigurationSession`。DEBUG 独立链接也可。
- `--workspace-fixture --settings-fixture` 已可用，另加 `--show-settings` 直接设置首页；scenario flags 为 `--settings-fixture-scenario=save-failure/save-delayed/conflict/catalog`，delay 为 3.5 秒。两供应商、两模型、MCP/Skill 与 unknown sentinel；无网络 transport。
- UI fixture Defaults 为 dev.aiterminal.ui-fixtures，history/cache/draft 为 FixtureAssistantHistory，Keychain service 为 dev.aiterminal.account.fixture / dev.aiterminal.pairing.fixture；不触碰正常身份。
- 真实专用账号 DEBUG 启动：`--service-test --local-login-fixture`。读取 App container `Documents/local-login-fixture.json`，对象字符串字段 server/username/password，CA 字段 ca_pem（兼容 ca）。不从参数传密钥，无内容日志；读取/登录失败仅通用状态。使用既有 TerminalModel.login 自动连接唯一在线 Desktop，账户持久化至 integration Keychain。私有文件由协调者写入和删除，不纳入 Git。

## Review R5–R8

- cancel 在Task执行前检查页面epoch与connectionEpoch，RPC传 expectedEpoch；pending集合统一释放，切scope后从当前scope pending恢复submitting。
- Global list/create 的每次请求与成功/失败回调都检查connectionEpoch；WorkspaceScreen generation变更同步AssistantModel，重连清busy与confirmed状态。
- allowInput随submitting禁用，避免旧send确认后遗留新requestID的草稿。
- Workspace真正合并当前sessions、各Desktop sessionSnapshots及持久化正常Agent archive索引（仅当前账号）；正常历史入口 `session.history.<sessionID>`，主行仍为 `session.select.<sessionID>`。
- `--workspace-fixture --settings-fixture` 提供关闭 `fixture-closed` 与离线 `fixture-offline` 行，history按钮为 `session.history.fixture-closed` / `session.history.fixture-offline`；两个都只读。`fixture-session` 保持可发fixture消息。
- AssistantModel.writeReason 对当前和历史 Session 检查设备身份、会话存在/exited/desktopAttached；Global独立；显示只读原因且禁止不适用send/stop。
- caPemPath 是独立UITest fixture字段，由验收脚本读取公钥后通过 launchEnvironment `AI_TERMINAL_TEST_CA_PEM` 提供；现有 DEBUG service-test login 已支持该环境值，无需修改测试owned源码或在App中读取host路径。
