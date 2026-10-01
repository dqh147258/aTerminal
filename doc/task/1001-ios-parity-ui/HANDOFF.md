# iOS 功能同步实现交付

## 状态与授权

源码及Review R1–R8修复完成；协调者现已授权以 `[未Review]` 标记提交生产源码、项目引用及本任务文档。独立最终 review 尚未完成；提交后冻结源码，不 merge，交协调者通过 Git 集成并执行专用模拟器验收。未启动/操作模拟器、共享服务或 Downloads。复用 main 未跟踪的 build/bindings、aTerminalCore.xcframework、fixtures 到本 worktree，没有修改 main 产物。

## 源码清单

- `apps/ios/aTerminal/AgentSettingsView.swift`（新增）：全屏路由、底部取消保存、供应商/模型/目录/绑定/读取、MCP/Skills 列表详情编辑与文件夹上传；ObservableObject 适配真实 ConfigurationSession。
- `apps/ios/aTerminal/SettingsDraft.swift`（新增）：Foundation-only ProviderDraft/ModelDraft/SettingsValidation/ConfigurationSession，真实生产使用的 async transport、identity/epoch、busy/serial/冲突刷新；DEBUG SettingsFixture。
- `apps/ios/aTerminal/WorkspaceScreen.swift`：全屏设置首页与图标信息行、独立 Global 入口、统一工作空间列表、账号设备来源返回，保留最近目录/终端操作。
- `apps/ios/aTerminal/AssistantModel.swift`：Global 多会话 RPC/分页列表/未读、scope 隔离草稿/图片/请求 ID、发送单飞、历史分页/完整消息/流式文本/证据；删除旧归档导入调用，保留原数据及底层 API。
- `apps/ios/aTerminal/ChatPanel.swift`：独立 Session/Global 聊天及 Global 列表、附件、证据、设置入口、历史/停止/滚动；无旧归档入口。
- `apps/ios/aTerminal/WorkspaceStyle.swift` / `LoginScreen.swift`：共用蓝色主题与 aTerminal 品牌；服务器摘要/编辑行为保留。
- `apps/ios/aTerminal/ChatStore.swift` / `PairingStore.swift`：fixture defaults/history/AgentCache/Keychain 隔离，普通命名空间不迁移、不清理。
- `apps/ios/aTerminal/aTerminalApp.swift`：fixture 真实网络边界阻断、DEBUG service-test 私有文件登录入口；真实账号正常路径保持。
- `apps/ios/aTerminal.xcodeproj/project.pbxproj`：仅加入两个 Swift 源文件引用，不修改签名设置。

## 功能矩阵

| 共用参考 | 实现与语义 | 本子任务证据 |
| --- | --- | --- |
| 登录例外 | aTerminal；服务器摘要/修改；账号密码字段保留 | Swift 类型检查与 App 构建；运行时由协调者验收 |
| 01 终端 | 原终端尺寸、输入、特殊键、最近目录；新增独立 Global | App 构建；未操作 PTY |
| 02 设置 | 全屏显示卡片、LLM/读取/MCP/Skills/账号信息行、固定恢复 | App 构建；UI fixture 已可用 |
| 03 LLM | 当前生效模型摘要、供应商/模型组、独立绑定入口 | 构建及配置确定性检查 |
| 04–05 供应商 | ID只读、Azure整体条件字段、启用、原对象/connection未知字段保留；空/纯空白key保留凭据 | 独立检查通过；非空key不trim，失败请求错误密钥遮蔽 |
| 06–07 模型 | 模型标识、可折叠高级能力、上下文/输出/采样/推理预算校验、绑定 | 独立检查通过 |
| 08–09 目录 | 搜索/分页、查询改变清cursor、同ID选择完整覆盖能力、返回后serial屏障 | 能力/取消屏障确定性检查；真实供应商目录待协调者验证 |
| 10 绑定 | Global/Session默认/当前覆盖；空当前覆盖继承；不改同模型推理与未知字段 | 独立检查通过 |
| 11 读取 | 1–100首尾行、TUI说明、保留reading未知字段 | 生产candidate合并；App构建 |
| 12–13 MCP | 内置只读、JSON导入/查看编辑/启停/确认删除、完整对象保留、修订冲突显式重试 | 校验与请求状态检查；真实RPC待协调者 |
| 14–15 Skills | Desktop完整目录安装、系统文件夹完整分块上传、SKILL.md单文件编辑/启停/删除 | 临时完整包检查含二进制哈希、层级/数量/大小/符号链接；真实完整资源RPC待协调者 |
| 16 账号设备 | 设置来源返回、当前/在线/离线设备、密码与退出原入口 | 构建；真实登录只通过专用fixture入口验收 |
| 17 工作空间 | 在线/结束会话统一列表，无旧手机归档或迁移 | 源码审查；原账号/历史文件未删除 |
| 18 Session | 当前终端路径、历史/完整消息/流式文本/证据、独立持久草稿与图片、停止 | 构建；真实消息/图片RPC待协调者 |
| 19–20 Global | 独立列表/新增/多会话/状态/未读、返回列表、Desktop上下文；agent_id真实路由 | 构建；新旧scope隔离；真实多会话待协调者 |
| 全部保存表单 | busy单飞、取消、固定底部、成功直接采用响应config/revision、失败保留非敏感草稿、冲突show后显式重试 | 独立检查9组通过；键盘/小屏/动态字体由协调者UI验收 |

## 自动化验证

- `python3 /Volumes/Code/public-worktree/aTerminal/1001-ios-parity-checks/scripts/check-ios-settings.py --source-root /Volumes/Code/public-worktree/aTerminal/1001-ios-parity-ui`：9/9 组 PASS，直接链接生产 SettingsDraft.swift。包含失败迟到/重连、冲突刷新途中重连、unknown字段、Azure空白、资源完整性、busy与响应快照。
- `xcrun swiftc -typecheck -D DEBUG ... build/bindings/ai_terminal_mobile.swift apps/ios/aTerminal/*.swift`：Debug/Release全量 Swift 类型检查通过。
- `xcodebuild -project apps/ios/aTerminal.xcodeproj -scheme aTerminal -configuration Debug -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' -derivedDataPath build/ios-parity-derived ARCHS=x86_64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO build`：成功。复用的 xcframework 仅含 x86_64 simulator，因此显式架构；未宣称 arm64 simulator 构建。
- 日志 `build/ios-parity-build.log`、`build/ios-parity-typecheck.log`；App `build/ios-parity-derived/Build/Products/Debug-iphonesimulator/aTerminal.app`。
- `git diff --check`：通过。

## 交接验收（协调者拥有设备）

测试签名、IDs、fixtures、私有登录文件契约详见 TEST_API.md。UI fixture 必须区分真实RPC，未保存截图。建议按20页参考检查小屏/键盘/动态字体和真实供应商/目录、MCP及完整Skill资源哈希；Global/Session各写不同草稿/图片并切换、重连，验证不互串；后台或切Desktop时迟到结果不覆盖当前页。

DEBUG `--service-test --local-login-fixture` 从 container `Documents/local-login-fixture.json` 读取 server/username/password/ca_pem，调用原 login 流程；文件由协调者写删。该入口未在本子任务登录或读取实际私有文件。

## 最终审查修复

R1/R2/R3/R4：失败和conflict show前后identity屏障、Azure空白、纯空白key保留、fixture真实服务边界；独立9组检查通过。R5/R6：cancel执行前与Global每页/创建绑定connectionEpoch，pending集合按scope恢复submitting，allowInput提交中禁用。R7/R8：drawer真正合并sessions/各Desktop快照/正常Agent archive持久索引；关闭/离线历史有独立AI历史入口；Session缺失/退出/Desktop离开只读并显示原因。

## 追加P2修复交付

新增正常Agent正文缓存搜索：元数据与当前账号scope白名单正文结果合并；原AgentCache数据库只读、当前generation、包括首页已淘汰后保留的非首页缓存，不接旧归档、不联网。250ms限频、后台SQLite读取、query/identity/serial屏障、取消传播；每scope3页和每页1MiB与Rust缓存限制一致。损坏DB只给搜索错误并解除busy，元数据搜索继续可用。

同批修复binding.reasoning：ModelDraft.candidate按新协议、能力、输出上限和采样验证每个指向当前模型的覆盖；兼容项保留，不兼容项只移除reasoning，其他字段/模型绑定保持。覆盖同ID目录与advanced能力编辑。

新增源码AgentCacheSearch.swift及pbx引用；修改AssistantModel/WorkspaceScreen/SettingsDraft。可重复验证：

```sh
xcrun swiftc -parse-as-library -module-cache-path /private/tmp/aterminal-settings-macos-cache apps/ios/aTerminal/AgentCacheSearch.swift doc/task/1001-ios-parity-ui/AgentCacheSearchChecks.swift -o build/agent-cache-search-checks
build/agent-cache-search-checks
xcrun swiftc -parse-as-library -module-cache-path /private/tmp/aterminal-settings-macos-cache apps/ios/aTerminal/SettingsDraft.swift doc/task/1001-ios-parity-ui/BindingReasoningChecks.swift -o build/binding-reasoning-checks
build/binding-reasoning-checks
```

两个新增生产direct检查通过；独立check-ios-settings.py更新后的10组通过（包括catalog cancel/read serial barrier）；x86_64 Debug App构建通过。fixture词与API见TEST_API.md，未运行sim。本轮按协调者授权追加[未Review]提交，不merge。
