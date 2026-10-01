# 验收状态与执行边界

最新独立源码审查已完成，无剩余源码阻塞：包括ae9b4cf原生code editor/clear、0e6980c identity drawer、b2d3f3a独立高级参数Button及main b4f8fa2清空helper。冻结main590a4f9全部production Swift与实现worktree一致，实际main UITest SDK离线typecheck通过。测试提交7e55a62/b62dd8a均已交协调者，未由本任务merge。完整simulator、20页截图及真实RPC由协调者执行；本任务只读其结果。

可集成文件：`apps/ios/UITests/WorkspaceUITests.swift`、`apps/ios/Tests/AgentSettingsChecks.swift`、`scripts/check-ios-settings.py`，以及本验收目录文档。production源码/pbx未改；未commit/merge。

## 纯 fixture

`WorkspaceUITests` 只用 `--login-fixture` 或 `--workspace-fixture`；设置/Global用 `--settings-fixture`。场景 `--settings-fixture-scenario=save-failure` 一次replace失败；`save-delayed`延迟3.5秒。测试添加 `ui-provider`/`ui-mcp`，endpoint为 `https://fixture.invalid/v1`，不使用正常凭据。所有fixture模式禁止normal AgentCache、migration、网络；defaults/history/keychain和生产账号隔离。fixtures由实现者生产DEBUG注入提供，测试不另实现业务reducer。测试覆盖独立Global/Session/第二Global草稿、目录分页/query cursor清理、binding当前覆盖回继承、读取边界、MCP确认删除/Skill编辑、设置来源返回、失败draft/busy、大字体/键盘/横屏固定actions。

## opt-in live

`LiveServiceUITests`仅在 `AI_TERMINAL_IOS_FIXTURE=/absolute/private/path.json`时启用；缺失时跳过。协调者使用全新专用iPhone15 `12E9EB2C-B331-4B83-9451-25797C989E2B`和 `--service-test`隔离身份，私有登录先由协调者完成；预先连接disposable PTY，terminal预置唯一typingMarker。JSON最小字段 `{ "session": "disposable-session-id", "typingMarker": "unique-fixture-marker-at-least-16-bytes", "caPemPath": "/absolute/public-ca.pem" }`，CA可省略，旧caPem纯公钥内容兼容。代码不读取password/username，不登录、不退出、不清空身份、不创建/关闭会话。先匹配marker，再匹配session row，选择后再次匹配marker，之后才可输入。

只跑 `-only-testing:aTerminalUITests/WorkspaceUITests` 是 fixture UI；只跑 `-only-testing:aTerminalUITests/LiveServiceUITests` 是真实 PTY/read-only 设置路由。真实 MCP/Skill 配置操作另由协调者隔离 UUID RPC 数据路径执行，不能把 fixture save 当真实 RPC。

## 已执行

- `xcrun swiftc -module-cache-path build/ios-parity-checks/swift-cache apps/ios/aTerminal/ChatStore.swift apps/ios/Tests/ChatStoreChecks.swift -o build/ios-parity-checks/chat-store-checks` 后运行：PASS，账号/服务器/设备/会话隔离、事件 cursor/dedup、请求边界、最近终端隔离。
- `xcrun swiftc -frontend -parse apps/ios/UITests/WorkspaceUITests.swift`：PASS，仅 syntax。
- `python3 scripts/check-ios-settings.py --source-root /Volumes/Code/public-worktree/aTerminal/1001-ios-parity-ui --ui-typecheck`：PASS，直接链接实际SettingsDraft.swift，10组host logic检查及完整UI XCTest的iOS Simulator SDK typecheck通过，没有launch设备。新增catalog cancelRead迟到success/error barrier。可在Git集成后省略source-root检查本checkout；结果与源码SHA位于 `build/ios-parity-checks/settings/verification.json`。
- runner Python语法 `python3 -m py_compile scripts/check-ios-settings.py`：PASS。
- `git diff --check`：PASS。

20页永久参考已逐页查看，fixture-01至20截图路由见SCREENSHOTS.md；关闭/离线history只读新测试与Global列表global.back、Skill编辑返回详情合同已同步。没有merge；未触碰设备/服务/用户身份。

首轮7e55a62；第二b62dd8a含live extension、R9/R10回归及初始输入helper。本分支helper停留b62dd8a供历史记录，运行/整合始终保留协调者main最终helper，不复制文件。最新正规.clear按钮验空，缺少clear的单行输入用右端坐标+UTF16长度delete并验空，TextView缺clear失败中止。R12 owner、正文搜索/绑定补充checks已通过。

只读设备日志：native-editor两项fixture UI通过；fixture-final先14项通过，fixture-retest的Catalog/ClosedHistory/Login三项再通过，合计17个不同fixture testcase全部PASS。新增正文only搜索测试也通过，20页capture留在协调者xcresult附件。live-final真实Desktop路由已PASS；MCP/Skill UUID与资源/原配置hash仍由协调者独立验证，不能用fixture结论替代。

opt-in `LiveServiceUITests/testDisposableMcpAndSkillProductionFormsRoundTrip` 需要fixture可选 `mcpId`、`skillId`、`skillPath`。ID为纯UUID或合法字母/数字/-/_前缀+带连字符36位UUID，二者不同；Skill path为本轮Desktop完整临时包POSIX绝对路径。字段缺失/无效在launch前skip。安装前assert两个UUID都不存在，避免覆盖；MCP先disabled HTTP http://localhost:9/mcp，编辑call_timeout_ms=12345并读回，enable/disable/确认delete；Skill安装完整目录，只edit SKILL.md为固定有效frontmatter，读回、disable/enable/确认delete。只操作这两个UUID；不读credentials或配置dump、不send Agent、不运行terminal command、不登录/退出。host由协调者观察revision、非Markdown资源哈希与原配置完整性，失败兜底cleanup。真实测试由协调者执行，本任务只离线typecheck。
