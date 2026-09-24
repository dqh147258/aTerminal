# iOS 交接（2026-09-23 第二轮完成）

## 第二轮最终结果（覆盖下方首轮记录）

- 最终 UI 为局部半透明设置/聊天浮窗，原生终端保持可见、原始列宽横滑、字号实时预览；新增原生边缘侧滑菜单、按服务/账号恢复最后终端、历史在线/已关闭/离线/待确认状态。
- AI 接入 allow_input/monitor/cancel，默认允许本条操作与监控；每请求事件游标持久化去重。关闭浮窗继续监控，切终端/后台停止旧 UI 轮询，恢复按原 ID查询。取消等待 stopped，不发送Ctrl-C。
- 修复键盘变化时编辑器结构切换丢焦；测试通过滚动读取LazyVStack中的历史事件，不再要求离屏元素存在。接管测试点击开关本体，保留真实产品控制权逻辑。
- **真实服务验收：`build/ios-live-final-resume.xcresult`，1 test / 0 failure / 0 skipped，119.198秒。** 使用iPhone15 `AC1104EB-5507-4260-8D39-DB2BD68771C7`、独立账号服务、真实Desktop PTY和明确标识的确定性模型，非线上模型供应商验收。
- 已逐项通过：UI登录/加密直连/120列Shell接管、独立IOS_PTY_OK行、横滑offset>100且列数仍120、字号24预览、80/120列切换、AI_DEVICE_OK和AI_DEVICE_DONE独立输出行、input/output/observation事件、关浮窗继续监控、重启App恢复终端及活动监控、取消并确认stopped、sleep30后真实Ctrl-C、独立IOS_CTRL_C_OK行、关闭主Shell后历史已关闭、退出临时账号。
- 本次专用主Shell已关闭，已通知协调者可停止iOS测试服务。再次验收必须创建新fixture/session，不复用已关闭会话。
- **小屏回归：`build/ios-floating-final-layout.xcresult`，4项全部通过，0 skipped。** 覆盖登录/显隐、设置持久化/重置、边缘菜单、局部浮窗、稳定横屏键盘、后台busy/过期回调。
- `build/chat-store-checks`通过账号/服务/设备/会话隔离、最近终端隔离、事件游标与去重、monitor/input/cancel JSON及Unicode/UTF8大小边界。
- 最终Release日志：`build/ios-floating-final-release.log`；最终截图目录：`build/ios-floating-final-screenshots/`，含13张真实流程截图和SE键盘/设置图。
- 早期`ios-live-service.log`为env未传导致skipped，`ios-live-complete.log`为被中断，均不是通过证据。最终日志确认不含测试密码。
- 真实测试需本地adhoc simulator签名：`CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=-`，否则Keychain拒绝保存。scheme通过`AI_TERMINAL_IOS_FIXTURE` build setting给runner传私有文件路径，密码仅在运行中读取。专用Keychain/UserDefaults/IntegrationAssistantHistory与产品数据隔离，未卸载或清数据。
- 无需重建Rust；集成须匹配协调者修复Close/Watch边界后的原生库。未测试用户真机、真实音频识别质量或线上模型供应商；这些不影响已完成的真实PTY/确定性模型编排验收。

复现真实验收：使用新的独立fixture，运行原scheme，指定上述iPhone15 UDID与 `-only-testing:AITerminalUITests/LiveServiceUITests AI_TERMINAL_IOS_FIXTURE=/absolute/path/to/new/private/account-fixture.json ARCHS=x86_64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=- test`，检查0 skipped。

以下是首轮历史记录，范围与验证边界以本节为准。

## 实现

- 原生 SwiftUI 登录、工作台、抽屉搜索、设备连接、会话创建/切换/关闭、终端历史、账号改密/撤销/退出；保留 UIKit Rust renderer、加密通道、输入队列与现有测试入口。
- 最终 CSS 的全屏设置/聊天；设置范围 12–24 px / 60–96%，默认 16/88，AppStorage 持久化。标题、设置标签、消息、输入区和页脚实色，空白底层使用透明度。
- assistant 使用协调者生成的真实 UniFFI API。同一串行 worker 上复核服务、账号、设备，再用显式 session ID 请求；切换/退出使 UI epoch 失效。请求先持久化，未知结果仅 poll，不自动重发；显式“附带当前终端”默认关闭。
- 聊天存于 Application Support/AssistantHistory，服务/账号目录和设备/会话文件均以结构化值 SHA256 命名，完整文件保护、排除备份。历史离线可读、可搜索、可复制，跨设备继续对话直接定位目标，已关闭会话只读。
- Speech/AVFoundation 原生语音仅填草稿；取消恢复原草稿，离开页面失效权限回调并停止录音。未授权/不可用显示真实状态。
- 后台断开会增加 generation，清连接 busy；旧成功/失败回调均忽略。账号操作由独立 accountBusy 保持串行，防止退出途中新登录与旧回调交叉。

## 验证和产物

- Debug 和 Release simulator 构建通过，架构为提供的 x86_64 Rust simulator 库，独立 DerivedData 为 `build/ios-workspace-derived`。Release 仅有既有静态库四条 duplicate debug-map object 警告，没有 Swift 警告。
- `apps/ios/Tests/ChatStoreChecks.swift` 已通过：服务/账号/设备/会话隔离、边界不可混淆、待查询 ID 持久化、默认不发送屏幕、4000 Unicode scalar/16000 UTF-8 字节/最多12条前文、9000字节历史回答不截断、响应解析。
- `build/ios-workspace-acceptance.xcresult` 四项 XCUITest 全部通过，0 failures：登录校验/密码显隐、显示设置重启持久化/恢复默认、抽屉/长文本/小屏键盘/稳定横屏、后台 busy 清理/过期失败回调。横屏输入与关闭按钮已截图验证，发送按钮坐标断言在键盘上方。
- 截图：`build/ios-workspace-screenshots/`，含 iPhone SE 登录/抽屉/设置/聊天/键盘/稳定横屏及 iPhone 15 终端/聊天安全区。UI 样本明确标识本地验证，未伪造连接或模型成功。
- Debug `--workspace-fixture`、`--login-fixture` 不读取账号或聊天文件；偏好使用独立 `dev.aiterminal.ui-fixtures` suite。`--long-chat` 仅是内存布局样本，Release 不包含这些入口。

复现构建（先同步主仓库 `build/bindings` 与 `build/AITerminalCore.xcframework`）：

```sh
xcodebuild -project apps/ios/AITerminal.xcodeproj -scheme AITerminal -configuration Debug -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' -derivedDataPath build/ios-workspace-derived ARCHS=x86_64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO build
swiftc -module-cache-path build/swift-check-cache apps/ios/AITerminal/ChatStore.swift apps/ios/Tests/ChatStoreChecks.swift -o build/chat-store-checks
build/chat-store-checks
```

## 验证边界

没有使用用户真机、生产账号或模型凭据，也没有对真实 Shell 执行输入。本次 UI 自动化使用独立本地布局 fixture；真实登录/设备授权、在线终端/控制权、模型成功/失败/断网和系统麦克风音频质量需要集成环境验证。Rust 协议/模型测试由协调者负责。

集成后建议在授权测试账号下验证：连接 Desktop 后创建会话、接管/释放、发送文字和功能键、读取历史及关闭；AI 配置前显示不可用，配置后显式附带屏幕发送并查询完成；请求期间换会话/换设备/退出账号不串线，未知结果只查询；两账号历史互不可见；语音允许/拒绝/取消均只影响草稿。

未提交、未合并、未关闭或删除 worktree/终端。仅修改 `apps/ios/**`、`scripts/generate-ios-project.rb` 和本目录。
