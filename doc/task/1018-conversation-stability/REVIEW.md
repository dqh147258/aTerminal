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

重连继续使用已有工作区，反馈集中在状态提示；iOS 将会改变布局的顶部 inset 改成小型 overlay。模型自动重试显示尝试次数，不重建对话页面，也不把固定延迟伪装成实时倒计时。原生截图由 UI 回归产生并留在 CI artifact，不以设计稿代替运行结果。

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

PR 的 `Approval and Reconnect Regression` 增加 Android 新的 12 项 conversation instrumentation、已有阅读/详情/工作区回归及 JVM 测试，并新增独立 iOS 模拟器 job，执行 conversation、authorization 和相关 workspace UI 测试。`Verify Terminal` 继续执行 Linux/macOS/Windows、移动端原生构建和 WebRTC 检查。CI 的最终状态以对应提交的运行结果为准。
