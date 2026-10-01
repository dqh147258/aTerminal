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

## 追加正文搜索与binding兼容性

- 新生产文件 `AgentCacheSearch.swift` 只依赖 Foundation/系统 SQLite3；`AgentCacheSearch.matches(path:scopes:query:cancelled:) throws -> Set<String>`。实际 schema 来自 crates/mobile-core/src/agent_cache.rs：只读 pages/cache_generations join，绑定scope参数，当前generation，每scope最多3页、单页1MiB上限；不读取 legacy/imports、不创建搜索索引。可直接 host Swift 链接生产文件。
- `@MainActor AgentSearchSession(delayNanoseconds:search:)` 实际生产使用，默认250ms debounce；`update(identity:query:scopes:refresh:)`、`contains(scope:identity:query:)`、`cancel()`；只读 request/matches/busy/error 与 changed 回调。AssistantModel transport 在 Task.detached 上读库、取消传播；query/identity/serial屏障。
- UI仍用 `workspace.search`，增加 `workspace.search.busy/error`。fixture `--workspace-fixture --settings-fixture --show-drawer` 输入 `cedar-body-only-731`（也可 `星河缓存检索`），只应命中 `session.history.fixture-closed` / `session.select.fixture-closed`；标题、路径、device均不含该词。打开闭历史后正文也包含该词。快速换成无命中词/清空/关drawer，不应被旧结果覆盖。
- 模型candidate每次验证所有指向当前模型的binding.reasoning，按新protocol/caps/output/sampling保留兼容override，只删除不兼容reasoning，保留unknown binding字段和其他模型绑定。
- 直接生产回归：`AgentCacheSearchChecks.swift`（scope、generation、非首页缓存、Unicode/大小写、无legacy、损坏DB、debounce、late query/identity、cancel/活动失败）；`BindingReasoningChecks.swift`（同ID目录、advanced等级、兼容high保留、无效low移除、未知字段、其他模型、协议、budget/output）。文件在本任务目录，可由checks子任务直接引用生产helper复用断言。

## 首轮sim AX标识小修

已移除`@ViewBuilder content`上的`settings.<section>.page/settings.form`中间标识，保留实际顶级容器`settings.page`与所有leaf IDs。设置顶级、设置首页、聊天和Global列表实际VStack均显式`accessibilityElement(children: .contain)`，不合并子控件；`provider.add/model.select.<id>/bindings.open/global.create/chat.draft/settings.reset`仍直接标在控件上。`chat.draft`仍为普通单行TextField，无axis改变。x86_64 App构建通过；运行时AX由协调者重跑确认，未在本子任务操作sim。

## R12：volatile sessionSnapshots owner

- 生产 `ChatStore.swift` 新增 Foundation-only `OwnedSessionSnapshots<Session>`：`bind(to: ChatIdentity?)`、`record(_:device:owner:) -> Bool`、`snapshots(for:)`。owner包括server/account；不同owner读取为空，解绑/换owner清内存，旧owner回调写入被拒绝。
- `TerminalModel.sessionSnapshots`仅提供当前identity的filtered getter。`loadDevices`发布新username/server前先bind新owner并清旧当前展示、推进generation；connect/create/close/refresh回调同时捕获并检查owner。密码变更成功/self revoke成功/logout绑定nil，仅清内存，不删除持久历史。
- 直接生产检查：链接`apps/ios/aTerminal/ChatStore.swift`与本任务`SessionSnapshotOwnerChecks.swift`。覆盖同owner刷新保留、A→B、同名不同server、nil解绑、迟到旧owner写入、重登录不复活volatile snapshot，并验证临时持久ChatStore历史不变。既有ChatStoreChecks也通过。

## 原生代码编辑器与常规清空控件

- MCP/Skill代码区现在是原生UITextView（局部UIViewRepresentable），仍用`app.textViews["mcp.json"]` / `app.textViews["skill.body"]`。原生selectAll、长按菜单、复制/剪切/粘贴/撤销未覆盖；不自动becomeFirstResponder、不修改TerminalView。value是纯文本，无placeholder。
- 标题栏普通清空按钮：`mcp.json.clear` / `skill.body.clear`；非空显示，空时消失，busy继承父禁用；显式清空结束IME组合并将光标移到0，防组合文本回填。
- SettingsView.field的普通右侧44pt清空按钮为`字段ID + ".clear"`（如provider.endpoint.clear、model.name.clear、reading.head.clear）。非空显示，readonly/busy继承父禁用，颜色WorkspaceStyle.muted；并非DEBUG/test控件。SecureField provider.key不是field helper，本轮不新增其清空按钮。
- 验空合同：UITextView清空value为空字符串；SwiftUI TextField空时AX value可能是placeholder，可结合对应.clear按钮消失及placeholderValue判定，再点击输入控件输入。测试helper仍由协调者维护。
- UIKit仅在外部Binding文本不同且没有markedText时赋值，保留并按UTF16边界clamp selection；普通delegate输入只更新Binding，不回写UITextView。显式clear事件是唯一主动结束组合和重置选区的路径。禁止autocorrect、spellChecking、smartQuotes、smartDashes、smartInsertDelete；默认多语言键盘、动态等宽字体、蓝色主题surface/边框、12pt边距。
- Swift类型检查与最终x86_64 Debug构建通过；原生编辑/清空实际smoke由协调者接手，尚未在本子任务实测sim。
