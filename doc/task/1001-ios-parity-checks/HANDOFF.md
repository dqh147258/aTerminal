# 验收状态与执行边界

矩阵和测试接口见MATRIX.md / TEST_CONTRACT.md。已独立review生产89fc011并确认R1–R8修正；R9搜索/R10能力改变后binding override两个P2已转实现者，不阻塞首轮fixture XCTest。测试按协调者明确授权先以[未Review]提交，源代码后续修复由协调者另集成。主checkout构建、实际simulator XCTest、同编号截图和隔离真实RPC由协调者执行；本任务UI只完成离线typecheck，不宣称实测通过。

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

下一提交：opt-in LiveServiceUITests MCP/Skill production表单round-trip，fixture新增mcpId/skillId/skillPath，缺失skip；只操作协调者预生成的两个UUID，host负责配置/资源哈希、失败cleanup。现有真实登录由协调者在专用设备完成，测试不读凭据/登录/退出。
