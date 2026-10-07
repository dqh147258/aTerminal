# iOS 远程屏幕交接

局部实现已完成并审阅差异。`WorkspaceScreen` 右侧“远程屏幕”进入全高显示器列表，包含名称、真实尺寸和主屏标记；未连接时可进入现有设备选择。选屏后 JPEG 等比例 fit 展示，可切换、返回列表或关闭，错误支持重试/刷新显示器。

`RemoteScreenModel` 持有唯一请求槽位和可跨线程失效的请求票据；请求完成后一秒继续刷新。切换、关闭、scene 非 active、连接代次/设备/账号变化停止刷新并丢弃过期响应。已进入同步 FFI 的请求不能从 Swift 中断，保留槽位直到 native 返回，期间不追加请求。`TerminalModel` 的现有串行后台 worker 在 FFI 前后及返回主线程前验证连接；JSON/base64/ImageIO JPEG 解码均在后台。最大请求宽度固定为 1600。

账号连接复用 owner/device/generation fence；`legacyConnect` 无账号配对连接使用每次连接生成的 UUID channel fence，不要求终端选择或控制权，也不调用 account.export。账号变化和 pause 清除此配对标识。

错误提示优先匹配 unknown_screen/Wayland/busy/timeout，最后处理通用录屏权限提示；已用带有 `allow screen recording` 外层 context 的组合错误回归验证，避免将 timeout 错报为缺少权限。

验证等级 Medium，已通过：

- `python3 scripts/check-ios-remote-screens.py`：直接编译运行生产解析/刷新模型，覆盖列表/帧 snake_case、主屏和原始尺寸、空态/坏数据、JSON/文本/base64/JPEG/像素边界、中文错误与未知详情、配对连接资格、单请求、切换/返回/关闭/重开、后台/恢复、过期响应、错误重试、设备/账号/server/断连及 dispatch 前关闭；并检查实际 pbxproj source wiring。
- `python3 scripts/check-ios-reconnect.py`：现有 Foundation 重连/所有者/实际控制权及 source wiring 回归。
- 新增模型、fixture、面板与 WorkspaceStyle 使用主 checkout **真实旧生成绑定**只读通过 x86_64 iOS 17.5 Simulator SDK typecheck。不拷贝或伪造生成绑定。WorkspaceUITests 使用同 SDK 的 XCTest framework 单独 typecheck 通过。
- 全部 iOS Swift `-frontend -parse`、`plutil -lint apps/ios/aTerminal.xcodeproj/project.pbxproj` 和 `git diff --check` 通过。

完整 App source typecheck 使用当前旧绑定时仅报告 `RemoteTerminal` 缺少 `remoteScreensJson` 和 `remoteScreenFrameJson` 两项，与分工中的待同步接口一致；不宣称完整 build 通过。最终 App build、真实网络/屏幕录制/配对采集、实际 Simulator UI 执行及真机验证未执行，按分工交协调者整合绑定后验证。

新增两个 UI 测试（仅导航 fixture，不代表真实采集）：

- `aTerminalUITests/WorkspaceUITests/testRemoteScreenListSelectionSwitchReturnAndClose`
- `aTerminalUITests/WorkspaceUITests/testRemoteScreenDisconnectedDeviceEntry`

隔离 Simulator 可使用 `079C5369-F052-45A8-A767-70B1A9FA6707`。前者启动 `--workspace-fixture --remote-screens-fixture`，通过独立后台绘制的 JPEG 测试列表、主屏、尺寸、fit 查看、切换、返回和关闭；后者启动 `--workspace-fixture --devices-fixture` 测试未连接设备入口。没有启动 Simulator、复制凭据、改写/清除正常 App 数据、修改 Rust/Android，未 merge 主 checkout。后续全任务 review 由协调者执行，提交仍保留 Pending Review 标记。
