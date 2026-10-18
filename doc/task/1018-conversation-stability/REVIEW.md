# 对话稳定性修复与提交前 Review

## 范围与问题记录

审查基线：`72c56913d2ad3c7fd2fd99bb69778fa9644b3c45`（`main`）。
审查覆盖模型流读取与 Host 执行循环、事件/全文/检查点存储、Android 与 iOS 的发送、历史、草稿、详情和重连 UI，以及既有回归工作流。
先创建以下 Issue 并写明原因、修正步骤和验收场景，再执行修正。

| Issue | 发现的问题 | 本次修正 |
| --- | --- | --- |
| [#8](https://github.com/dqh147258/aTerminal/issues/8) | 模型临时失败直接终止；流中断丢失已显示正文；不完整 finish reason 可能被视为成功 | 有界且可取消的重试；输出后禁止自动重放；保存中断正文；完整性检查和安全错误码 |
| [#9](https://github.com/dqh147258/aTerminal/issues/9) | Android 迟到回执、关闭窗口、历史换代和详情回调不一致 | 不可变待确认消息、按请求 ID 确认、独立草稿存储、代际准入、详情恢复、只读重试队列 |
| [#10](https://github.com/dqh147258/aTerminal/issues/10) | iOS 授权轮询/发送锁住本地输入；带图草稿逐字同步保存；重连读状态退化 | 编辑与提交权限分离、后台去抖保存、不可变待确认消息、详情阅读缓存与批准验证分离、live/history 交接 |
| [#11](https://github.com/dqh147258/aTerminal/issues/11) | 原生对话窗口层级拥挤，重连反馈干扰阅读 | 分角色消息卡、多行输入区、附件管理、代码区、复制操作、非阻塞状态提示与重试状态文案 |
| [#12](https://github.com/dqh147258/aTerminal/issues/12) | 合法长回复撞到 24 KiB 事件限制和 1 MiB 检查点限制 | 消息、全文预览和检查点预算协调；UTF-8/转义安全分页；持久化与重开回归 |

## 模型重试与完整性

每个模型请求最多 **3 次实际尝试（首次 + 2 次重试）**。只有尚未收到任何文本、推理或工具输出的临时失败可以自动重试。HTTP 408、429、500、502、503、504、529，以及可识别的超时、连接错误和零输出截断属于该范围。认证、参数、响应格式等失败直接报告。

退避为 500 ms、1 s，各加 0–255 ms 的按 run 分散的抖动。有效 `Retry-After` 更长时尊重其时间；超出 run 剩余时间则终止重试。单次尝试最长 120 秒且受剩余 run 时间限制，每次实际请求都计入既有 round/token 预算。等待期间可取消，实际尝试前重新核验授权。

重试仅重新发送同一份不可变模型请求，不返回到已执行工具之前。已产生任何模型输出后不自动重放。`Length`、`ContentFilter`、未完整结束的其他 finish reason、缺少结束事件及无有效正文/工具的响应不会进入成功执行分支。兼容没有提供 finish reason、但有完整终止事件的现有协议适配器。

流失败后，Host 将已显示的正文前缀（既有 live 上限为 16,000 UTF-8 字节）保存为独立 `interaction`，使用 `source=model_interrupted`、`state=interrupted`、`interrupted=true`。它不伪装成最终 assistant 完成记录。历史分页已有的 `partial` 字段继续只表示“需要读取全文”，与中断状态分别处理。供应商原始错误正文、私有 URL 和凭据不进入客户端错误提示。

## 消息投递、离线编辑与重连

“已派发的消息 A”与“正在编辑的草稿 B”分别保存。A 的文本、附件、权限模式和请求 ID 在发送前形成持久化快照；用户继续修改 B 不会改变 A。成功回执只确认原请求 ID，不能清除 B。结果不确定时，用户可查看并手动重试原消息，沿用其请求 ID 与内容；服务端既有幂等账本负责识别重复投递。

客户端自动重试限制在明确的只读操作白名单，最多 3 次，带递增退避和抖动；认证失败、取消、连接 epoch 变化后停止。发送、图片上传、授权回答、取消和配置写入不走通用自动重试。Android 将读取工作与发送/上传队列分开，全文读取去重并拒绝过期分页结果。

| 操作 | 重连时的行为 |
| --- | --- |
| 编辑多行草稿、清空草稿、添加/移除本地附件 | 保持可用，发送中的原消息单独保留 |
| 阅读/滚动/复制已载入的对话或详情 | 保留原内容及阅读位置 |
| 编辑仍有效问题的本地答案 | 保持可用，不因权限轮询短暂锁定键盘 |
| 发送、停止、提交答案或批准操作 | 需要可用连接和当前有效权限 |
| 获取新的历史、全文或详情 | 等待连接恢复或有界读取重试，不清空已显示内容 |

Android 草稿、阅读位置和 pending 消息迁入独立的小型 preferences 文件，历史只在内容变化时保存；兼容既有快照，避免每次输入重新编码历史或连带写入大历史 XML。iOS 普通输入按 350 ms 去抖，在串行后台队列编码和写入；关闭、切换、后台和发送等生命周期边界执行同步 flush，旧排队写入不能覆盖新草稿或恢复已确认的消息。

## UI 改动

Android 与 iOS 保留原生界面。正文按用户、Agent、操作和中断回复区分角色；输入区拆成多行编辑区和操作栏；附件可独立管理；正文与代码采用不同排版，常用复制操作更容易访问。iOS 新正文使用动态字体，新图标按钮提供至少 44 pt 触摸区域。

重连继续使用已有工作区，反馈集中在状态提示；iOS 将会改变布局的顶部 inset 改成小型 overlay，Android 复用当前页头已有的字幕位置，不新增会挤压正文的横幅行。模型自动重试显示尝试次数，不重建对话页面，也不把固定延迟伪装成实时倒计时。原生截图由 UI 回归产生并留在 CI artifact，不以设计稿代替运行结果。

原生截图复核后，两端聊天阅读面改为不透明底色，避免底下的终端和账号页面透入正文；其他浮窗仍保留各自的透明度设置。iOS 隐藏编辑器系统滚动背景，只对页头、角色等次要标签限制最大字号，正文、代码和输入保留完整 Dynamic Type。键盘弹出时收紧装饰间距，将对话/历史切换收进可达的操作菜单，保留原编辑器和焦点。离线指示依据当前对话目标显示。Android 共用一处连接状态，工具详情打开后优先使用前景页头的状态位置，保留独立的业务错误提示。

## 提交前交叉 Review 及修正

运行时、Android、iOS 分别实现后，再进行跨实现者审查和主审集成检查。Review 中补充修正了：

1. **历史 generation 增加不等于当前详情已删除。** Android 保留当前详情并等待新历史重新确认；存在的详情可以恢复，避免永久禁用。
2. **缓存正文与批准依据具有不同生命周期。** iOS 同 scope 重连保留已读详情，清除批准验证标记；恢复连接后重新核验权限及完整详情，跨 scope 清理缓存。
3. **live 清空与历史到达之间存在异步空档。** 两端在可显示的持久正文接替之前保留 live；iOS 按 run/root 和 UTF-8 正文前缀核对，分页预览不冒充完整替代。
4. **迟到的旧 generation 不能覆盖已确认的新 generation。** state/history 均参与准入，连接/页面 epoch 改变后废弃旧回调。
5. **长历史放大逐字保存成本。** Android 进一步分离 preferences 文件，消除小草稿改动连带整份历史写盘。
6. **JSON 转义也占用预览预算。** Store 按编码长度裁剪正文和语义字段，控制字符不会把小预览放大到帧限制之外。
7. **测试入口也经过复核。** 离线 CI 只选择不依赖真实账户/Desktop 的 fixtures；旧 UI 断言更新为实际授权控件并保留只读验证；工具详情截图纳入固定前缀的证据收集。

消息事件允许最多 3 MiB JSON，为既有 4 MiB 全文记录预留 Host 元数据；模型流预算再预留 64 KiB，检查点采用显式 16 MiB 上限。常规控制事件仍维持小限额。测试验证越界替换不会破坏已有检查点，全文分页、scope 隔离与 retention 机制保持有效。

## 验证记录

提交前本地验证环境：Linux、Rust 1.94.1、Android API 35 / Java 17、Swift 6.4.1（Foundation）。

| 验证 | 结果/说明 |
| --- | --- |
| `cargo test --locked --workspace --exclude ai-terminal-bindgen` | 293 passed，0 failed，21 ignored；覆盖 Desktop 集成、真实 HTTP/SSE、MCP、模型/工具边界与存储重开 |
| `cargo clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings` | 通过 |
| `cargo fmt --all --check`、`git diff --check` | 通过 |
| `python3 scripts/test-host-terminal.py target/debug/aTerminal` | 通过：真实 PTY 输入/输出、清屏、备用屏幕和终端状态恢复 |
| Android `clean`、`assembleDebug`、`assembleDebugAndroidTest`、`testDebugUnitTest` | 最终 Review 后完整重建通过；23 个 JVM 测试全部通过 |
| iOS `check-ios-chat-delivery.py`、`check-ios-authorization.py`、`check-ios-reconnect.py` | 最终 Review 后三套 Foundation 行为检查全部实际运行通过，包括代际竞态、Unicode 交接与授权详情恢复 |
| Python / 工作流语法 | 通过 |

Rust 的 21 个 ignored 来自原有显式账户/RPC fixture、真实模型凭据和图形桌面场景；本次未将它们改成通过。Android 本地验证包括编译及 23 个 JVM 测试，不等同于设备 UI 执行；Linux Swift 测试不等同于 SwiftUI/Xcode 编译或 iOS 模拟器测试。

PR 的 `Approval and Reconnect Regression` 增加 Android 新的 13 项 conversation instrumentation、已有阅读/详情/工作区回归及 JVM 测试，并新增独立 iOS 模拟器 job，执行 conversation、authorization 和相关 workspace UI 测试。两端当前原生选择分别为 50 项和 17 项。`Verify Terminal` 继续执行 Linux/macOS/Windows、移动端原生构建和 WebRTC 检查。CI 的最终状态以对应提交的运行结果为准。

## PR 原生 CI 与截图 Review 跟进

创建 PR 前已完成上述代码交叉 Review 和本地验证；PR 创建后继续检查真实原生 CI 与截图，并在每次后续提交前再次 Review。以下结果明确绑定对应旧提交，不能代替本轮 UI 调整及新增测试的执行结果。

| 提交与运行 | 已核对的实际结果 |
| --- | --- |
| `7b1d5de`：[原生回归第一轮](https://github.com/dqh147258/aTerminal/actions/runs/39099028980) | iOS 原生 16/16 通过，iOS state 四套脚本通过。Android 49 项中 48 项通过；旧授权测试只模拟一次全文读取失败，与新只读重试的预期冲突 |
| `65729ce`：[原生回归第二轮](https://github.com/dqh147258/aTerminal/actions/runs/39100168397) | Android 49/49 通过，原生截图成功导出；iOS state 四套脚本通过。iOS 原生 15/16 通过，唯一失败定位到测试读取输入中间 AX 状态 |
| `65729ce`：[Verify Terminal（PR）](https://github.com/dqh147258/aTerminal/actions/runs/39100168275) 与 [Verify Terminal（push）](https://github.com/dqh147258/aTerminal/actions/runs/39100166423) | 两条运行全部 10/10 jobs 通过，包括 Linux/macOS/Windows 测试与 Clippy、真实 PTY、移动端原生构建、WebRTC 检查 |

后续 Review 的具体修正：

1. **授权失败场景仍需验证完整重试耗尽。** Android fixture 连续拒绝三次只读全文请求，再由用户显式重试恢复；完整详情、指纹和批准回执断言保持严格。
2. **截图必须来自实际已提交的原生帧。** AGP 在测试后先卸载 fixture 导致证据丢失，测试命令改为完成固定前缀截图导出后再卸载隔离包。随后截图发现 `waitForIdleSync` 仍可能读到上一帧：新 helper 在可见视口检查和 frame commit 后使用 PixelCopy，检查消息卡实际像素，不以重新绘制 View 生成截图。
3. **重连提示不能移动阅读区。** Android 原先插入横幅使正文纵向移动约 42 px；改为现有字幕槽，非零滚动位置、消息和输入坐标在断线与恢复后保持一致，工具详情中的状态也必须属于当前前景页面。对话中的重复通用断线提示合并。
4. **大字号和键盘需要原生画面验证。** iOS 第一轮截图显示透明背景、编辑器黑底和装饰文字无限放大；本轮新增正常字号与 AccessibilityXXXL 的正文缩放、页头上限、44 pt 控件、键盘下正文视口和草稿保持断言，并输出四张明确命名的截图。iOS 15 使用只定位当前编辑器的局部背景适配，不改变文本、选区或 first responder，也不使用全局 UIAppearance；现代模拟器不代表已覆盖 iOS 15 的实际表现。
5. **输入事件提交不等于 AX 已更新。** 第二轮 iOS 失败发生在首次输入 `original`、发送前。xcresult 显示合成调用返回后仍在交付原生按键事件，首次 AX 快照值为 `or` 且保持焦点；同一原始录屏随后显示完整 `original`。测试只增加最多 5 秒的只读完整值等待，输入仍仅一次；清空也用相同严格等待。不改生产自动纠正、不重打字符、不接受前缀。
6. **已有错误和读取取消也不能造成重连闪动。** Android 已显示连接读取错误时，共享状态出现后保留该行原空间，只隐藏内容；原先隐藏的行不会新增。iOS 授权读取的 `CancellationError` 与真实协议/授权失败分别处理：仍使旧权限失效、保留已读详情，但不短暂插入通用取消错误提示。对应坐标与 Foundation 回归验证这些边界，实际错误继续显示。

本轮 UI 修改和输入/帧同步修正均已交叉审查。新增原生测试的结果及最终截图以 [PR #13 检查页](https://github.com/dqh147258/aTerminal/pull/13/checks) 对应最终提交为准，PR 描述同步记录最新验收结果。
