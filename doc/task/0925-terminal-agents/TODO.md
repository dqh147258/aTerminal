# Desktop 双层 Agent 与可回读终端记忆 TODO

- Status: Completed
- Updated: 2026-09-26

## Checklist

- [x] 验证并锁定 Rig/rmcp 正式版、Rust/平台与 Provider 合同；验证原子 ReadView 和内容锚点
- [x] 统一 CLI、目录迁移和 ConfigService；完成 Provider/Model Profile、向导、目录及思考强度配置
- [x] 实现 SQLite 原文/事件/上下文投影、UUID、动作账本及有界分页
- [x] 实现 TerminalBackend、状态观察、授权输入、内容锚点读取和离屏 PNG
- [x] 实现内置与用户 MCP、Codex Skills 加载和受管动作
- [x] 接入单 Session Rig Loop、同配置分析屏障、引文校验、分层压缩、取消恢复
- [x] 实现全局与 Session Agent、mailbox、用户 Run 内委托和共享预算
- [x] 实现历史清理 CLI、持久保留策略与引用/游标失效
- [x] 接入加密 RPC/mobile-core、Android/iOS 对话及分页缓存、迁移和配置表单
- [x] 更新文档并完成自动化、Rust 全工作区及 Desktop/Android/iOS 构建验证

## 第一阶段已验证拆分项（历史记录）

以下保留第一阶段基础验证记录；完整产品接线和本轮验证见后文。

- [x] 锁定 Rig 0.42.0 / rmcp 3.4.1，使用现有 ring TLS 实现并通过 Rust 1.94.1 编译。
- [x] 验证六种 Provider 实际请求体的 Terminal 工具配对与分析尾部追加；验证 OpenAI Chat SSE 和取消入口。
- [x] 验证内存 MCP initialize/list/call/shutdown，并关闭 HTTP session 失效后的请求重发。
- [x] 实现不可变 ReadView 和 tail/search/screen 引擎接口，验证锚点、空白、UTF-8、重绘、resize 和 alternate screen。
- [x] 实现 ConfigService 修订冲突、完整候选校验、多文件恢复日志与账号范围隔离；接入独立配置 RPC 和只读设备校验。
- [x] 实现 ~/.aTerminal 默认目录发现、活跃旧实例复用/锁保护、显式旧目录迁移及账号 vault 引用保留；不删除源目录。
- [x] 实现 CLI sessions/daemon 兼容命令和 config/providers/models 基础管理，Provider 密钥写入后不回显。
- [x] 实现 SQLite UUIDv7/BLOB/事件/动作账本基础、用户前置快照去重、50 条 keyset 分页、引文校验及 pin/清理游标失效的底层逻辑。
- [x] 在隔离临时目录验证 CLI Provider → Model → global 默认绑定 → 回读/校验流程，无真实模型调用。

## 当前实现与验证边界

- Desktop 已通过 Agent v1 接入持久 AgentHost、真实内存 MCP、TerminalBackend 与 actor。新配置直接控制新 Runtime；旧 Assistant 默认关闭，显式兼容开关为 `AI_TERMINAL_LEGACY_ASSISTANT=1`。
- 已接入全局/Session Agent、同 Run mailbox、同根追加委托、共享预算、后台权限检查、即时人工抢占、动作账本和取消。PTY/子任务回报、清理、重启不启动模型。
- 用户 MCP 与 Codex Skills 已接入固定入口、版本冻结、资源分页、受管脚本、凭据撤销、分块上传、编辑/启停/删除；内置条目由二进制发布并生成只读身份镜像。
- 原文、截图、完整工具参数/结果、分析与投影已持久化。读取分析失败可在下一真实用户消息后恢复，不重复读取；原文回读带完整 hash/分片 hash，图片实际进入视觉模型内容块。
- Shell integration 显式启用，Unix 已使用临时 HOME 和真实 bash/zsh PTY 验证退出码及 cwd；PowerShell hook 和 Windows ACL 仅实现，未在 Windows 运行。
- 历史清理固定批次水位，跳过活动 pin 后继续扫描；账号默认保留策略可被 Session 覆盖。复合索引随来源清理失效，派生索引不计入用户历史条数。
- Android/iOS 已开放全局/Session 对话、发送/追加/停止、证据/图片、50 条历史、最多三页缓存、独立旧归档和模型/思考强度/MCP/Skills 设置；旧归档按消息流式导入 SQLite，不上传为模型历史。
- 本机验证范围是 macOS、Android 三 ABI 原生库及 APK、iOS 设备/模拟器原生库和模拟器 App 构建。Linux/Windows、真机交互与真实供应商推理/缓存没有实跑，详见 HANDOFF.md。这些项目不被记为通过。

## Verification evidence

- 2026-09-25：锁定 `rig-core =0.42.0`、`rmcp =3.4.1`；Rust `1.94.1` 隔离依赖编译通过。
- `cargo +stable test --offline -p ai-terminal-engine`：12 项通过（原子视图、锚点边界/歧义、空白、UTF-8、alternate screen/resize/repaint）。
- `cargo +stable test --locked --offline -p ai-terminal-agent-runtime --test model_boundary`：3 项通过；本地 HTTP 桩捕获六种协议实际序列化请求，Terminal 工具配对后的分析保持原前缀/工具/配置；OpenAI Chat SSE 解析和取消门槛通过。
- `cargo +stable test --offline -p ai-terminal-agent-runtime --lib`（配置模块加入前）：4 项通过；真实内存 MCP initialize/list/call/shutdown，禁用 HTTP session 失效后重发。
- TLS 适配：Reqwest 0.13 使用 `rustls-no-provider` 并显式安装现有 `ring`，避免与既有远程栈混用默认 provider。
- `cargo +stable test --locked --offline --workspace`：首次完整回归 83 项通过；包含隔离 PTY、加密 relay/账号撤销，以及配置 RPC 密钥不回显测试。随后补入首次配置冲突保护测试，第二次完整工作区回归 84 项全部通过。
- `cargo +stable test --locked --offline -p ai-terminal-agent --lib`：修正真实旧版 0644 账号标记的迁移后，14 项通过。
- `cargo +stable build --locked --offline -p ai-terminal`：通过。
- `cargo +stable fmt --all -- --check`、`git diff --check`：通过。
- 临时 CLI smoke：providers add/show（假密钥 stdin）、models add/set-default、拒绝未知 reasoning level、config show/validate、sessions list 全部通过；测试 daemon 已关闭。

- 后续存储加固：`agent-runtime --lib` 10 项通过，新增已有目录权限不被修改的回归，并加入 SQLite schema version/原子初始化与错误数据显式拒绝。
- 后续身份加固：`ai-terminal-agent --lib` 16 项通过；新建账号 vault 引用在同一目录不同路径写法之间保持稳定。
- 后续 HTTP 约束：`model_boundary` 4 项通过，新增重定向不会重新发送模型上下文；禁用自动重试。
- 最新受影响包 `clippy --all-targets -- -D warnings` 通过。新配置 RPC 对畸形请求使用不包含原文的错误码，避免解析错误回显密钥字段；对应隔离 daemon 集成测试 1 项通过。

## 本轮追加验证（2026-09-26）

- 持久 Agent 端到端：真实隔离 PTY + 内存 MCP + 本地 SSE 模型桩，验证读取→同配置尾部分析→一次输入→回复、原请求去重、UUID 回读与关闭 Session 后保留记忆。实际捕获请求比较旧消息前缀和模型/工具配置。
- Host 合同：重启与被动事件模型调用数不增长；下一用户消息恢复未完成分析且读取次数仍为 1；同根委托追加沿用 task ID，停止根任务取消子任务但不取消独立 Session 任务。
- MCP 合同：假 stdio 服务真实 initialize、两页目录、call、shutdown 与 stderr 脱敏；假 HTTP 服务验证图片载荷与仅一次调用；会话失效不重发、模型重定向不重发仍有回归。
- Skills 合同：frontmatter/openai.yaml 显式策略、调用名称边界、UTF-8 资源分页、路径穿越/内容或 manifest 篡改拒绝；分块上传重试幂等、跨 owner 拒绝、冲突块拒绝、一次修订发布及 builtin 拒绝。
- Store 合同：`allow_input` 是请求去重身份的一部分；全局可读取所属 Session 原文；256 条全部 pin 的批次不会遮挡后续候选；账号默认策略继承与 Session off 覆盖；0/49/50/51 条大记录保持有界并可无损回读。
- Mobile cache 合同：最多三页、generation 单调失效，旧响应不能恢复已删除缓存；10,001 条旧归档流式导入、重复导入、50 条分页和畸形输入事务回滚通过，源文件保留。
- `cargo +stable test --locked --offline --workspace`：最终完整回归 **104 项全部通过**（包含真实隔离 PTY、本地模型/MCP 桩，不使用真实供应商）。
- `cargo +stable clippy --locked --offline --workspace --all-targets -- -D warnings`、`cargo +stable fmt --all -- --check`、`git diff --check`：通过。
- `python3 scripts/build-artifacts.py android ios --toolchain stable`：通过 Android arm64-v8a/x86_64/x86、iOS aarch64 device/x86_64 simulator 原生构建、UniFFI 绑定、Android debug/测试 APK 与 lint、iOS 模拟器 App 构建。后续原生 UI 修正另以 Gradle assemble/lint 和 xcodebuild 复验通过。
- 构建中旧目录大小写残留导致 Xcode PCH 缓存路径不匹配；清理可重建缓存后成功。代码盘接近满载时只清理了 Rust 增量缓存。没有停止、迁移或部署用户当前终端。

## 完成记录

- 全部实施步骤及本机可执行的自动化/构建已完成。最终原生 UI 表单修正再次通过 Android assembleDebug/lintDebug 与 iOS xcodebuild。
- `PLAN.md` 已标记 Completed；使用方式更新至 `deploy/ASSISTANT.md`，旧 AI 定义注明被替代。
- 不触发远程 CI，不安装/启动设备，不部署或重启用户现有 Desktop，不提交工作区。未执行的平台/真机/供应商验证在 HANDOFF.md 单独列出，不属于已通过结果。

## ModelScope 真实供应商验收（2026-09-26，用户已授权）

- [x] 使用用户提供的凭据配置当前 Desktop 的 ModelScope Provider；Rust 目录请求确认 `Qwen/Qwen3.8-27B` 存在。
- [x] 加入可重复运行、默认忽略的真实供应商测试，覆盖隔离 PTY 工具循环、分析、UUID 回读、去重与 usage。
- [x] 根据实际结果配置模型与默认绑定，完成回归并更新真实供应商验证边界。

- 真实 ModelScope 测试显式运行通过：7 次 HTTP/模型调用，约 71 秒；两次读屏及分析、一次实际写入、请求重试零新增调用、关闭会话后 UUID 回读通过。
- 根据真实 SSE 修复“元数据被当作正文证据”的分析指令与错误反馈；严格证据校验保持。确定性新回归通过。
- `CARGO_INCREMENTAL=0 cargo +stable test --locked --offline --workspace`：**105 项通过，1 项真实供应商测试默认忽略**；显式 live 测试 **1 项通过**。全工作区 clippy、fmt、diff check 通过。
- 默认 Desktop 在确认 0 会话/0 活动 Agent 后载入修复，ModelScope Provider 与 `modelscope-qwen` 全局/Session 默认绑定已回读确认（revision 4）。
- 完整结果和复跑命令见 [LIVE-MODELSCOPE.md](LIVE-MODELSCOPE.md)。之前“真实供应商未实跑”的记录仅描述上一阶段；其他供应商/真机/Linux/Windows 边界仍保留。

## 锚点首尾与 TUI 策略修正（用户追加授权）

- [x] 首 10 / 尾 20 默认与可配置项，CLI/App 编辑及 Run 配置冻结。
- [x] 独立展示首尾与去 TUI 搜索锚点，统一覆盖文本/记录引用/候选；原文与分页保持无损。
- [x] 更新搜索/分析提示与合同，验证 TUI 动态变化、全 TUI、重复内容、过滤后空锚点和配置边界。

- [x] 用户已指定并授权 `emulator-5586`：用隔离服务/账号/PTY 在 Android 16 验证读取配置和 Agent 面板，不清除已有应用数据。

### 锚点修正最终证据

- 默认 10/20、1–100 配置范围、20 行搜索、跨变化 TUI、全 TUI/空锚点拒绝、重复匹配、原文不变、分类原句/完整性与配置恢复回归通过；常规全工作区 **110 项通过，1 项 live 默认忽略**。
- ModelScope live 显式 **1 项通过**：6 次实际请求、一次写入，识别并从搜索锚点移除 `sh-3.2$`，原文仍保留。
- Android assembleDebug/测试 APK/lint、iOS 模拟器 xcodebuild 通过；全工作区 clippy/fmt/diff check 通过。
- 用户提供的 `emulator-5586` 上仪器测试 **1 项通过**，包括配置保存、加密 RPC、PTY 读取、历史、原文对话框及 TUI 排除；截图复核后为输入框补充常驻首尾标签并复验通过。详见 [ANDROID-AGENT-UI.md](ANDROID-AGENT-UI.md)。
- 当前默认 Desktop 在空闲时载入新版本，实际读取配置为 `head_lines=10, tail_lines=20`，revision 5；已有 ModelScope 凭据和默认模型绑定保留。

## 移动端登录持久化（2026-09-26，用户追加授权）

- [x] 两端立即清除登出凭据，阻止旧任务写回/恢复身份，保留加密恢复及离线身份。
- [x] Android 隔离账号登录、真正进程重启、离线启动、登出未完成时重启、重新登录自动化通过。
- [x] Android 构建/lint 与 iOS 模拟器构建，记录验证边界。

- `emulator-5586` 六阶段跨进程登录测试全部通过（各阶段不同 PID）；离线请求失败后身份保留，网络注销未开始时本地已清除，旧请求写回被拦截，退出后重启保持未登录，重新登录成功。
- 原有 AutoConnectTest 1 项通过：Activity 重建恢复与自动连接、手动选择后刷新保持行为通过。主账号及连接偏好未改变。
- Android debug/测试 APK/lint、iOS x86_64 模拟器构建、Python 编译及 diff check 通过。iOS 本轮未运行登录生命周期仪器测试。
- 详情与复跑命令见 [MOBILE-LOGIN.md](MOBILE-LOGIN.md)。

## 构建默认登录地址（用户追加授权）

- [x] 两端构建参数、默认展示/修改地址及持久化，更新本地构建入口。
- [x] Android 地址编辑与登录回归、iOS 构建验证。
- [x] 安装本地包，用已有本地账号登录 emulator-5586，普通重启恢复并保持 App 已登录。

- Android 六阶段回归通过，覆盖 BuildConfig 默认地址、折叠编辑、保存覆盖值及跨进程持久化。
- 普通登录 UI 使用内置本地 URL/公共 CA 成功登录 `aiterminal_local_test`，独立进程恢复相同身份通过。App 已以普通 launcher 参数启动并保持登录，当前 0 台 Desktop 在线。
- Android assemble/lint、iOS build-for-testing、最终 Info.plist 地址与 CA 校验通过。登录页及已登录界面截图已复核。详情：[LOGIN-SERVER.md](LOGIN-SERVER.md)。

## 自动连接与首帧进入首页（用户追加授权）

- [x] 两端连接准备状态，首帧就绪后进入首页，无设备/会话/失败明确结束准备。
- [x] 心跳去重及断开时自动发现，晚启动 Desktop/新终端可自动显示。
- [x] Android 首次绘制与恢复/晚启动回归、两端构建验证、更新当前模拟器并验证实际终端。

- 三种 AutoConnectTest（已在线、Desktop 后上线、Terminal 后创建）全部通过；登录首次进入和重建恢复均有 OnPreDraw 首帧断言。六阶段跨进程登录回归通过。
- 真实环境另发现模拟器自身 TLS 转发超时，独立 HTTPS 客户端与 ADB 隧道对比定位；保留数据重启后原 LAN 地址恢复，最终恢复原编号 emulator-5586。
- 当前真实 Terminal 两次冷启动自动打开通过，1894 / 1751 ms，App 已保留在连接状态；没有向用户 Shell 输入内容。
- Android assemble/lint 与 iOS build-for-testing 通过；iOS 未做运行验证。详情：[AUTO-CONNECT.md](AUTO-CONNECT.md)。

## 移动端输入与布局（用户追加授权）

- [x] 两端默认输入、单次图标特殊按键浮窗、较小图标与 6 字号 / 0–100% 不透明度。
- [x] 横屏隐藏顶部栏并迁移侧栏功能，旋转保持连接；Agent 全屏。
- [x] Android 隔离 PTY 自动化/截图验证、两端构建并恢复实际已登录工作空间。

- 用户进一步确认：首次进入不显示系统键盘，点击 Terminal 后才显示；此行为已加入实际 IME 可见性断言并通过。
- 输入/布局真实 PTY 测试 1 项、已有 Workspace UI 4 项、Agent 功能端到端 1 项、自动连接首帧回归 1 项、实际账号冷启动恢复 2 项通过。
- 横屏 Agent 输入法遮挡已复现并修复：禁用 IME 抽取页面，输入/发送并排，空间不足时操作折叠入菜单。截图与发送按钮可见性断言通过。
- Android assemble/lint 与 iOS build-for-testing 通过。最新本地包已安装，emulator-5586 保持原有账号/终端连接且系统键盘未自动弹出。iOS 本轮仅构建验证，详见 [MOBILE-INPUT-LAYOUT.md](MOBILE-INPUT-LAYOUT.md)。

## Terminal 状态去重（用户追加授权）

- [x] 后台采样、用户和委托快照统一语义比较并复用既有状态；忽略采样时间、Shell 报告时间和人工输入计数，保留抢占检查。
- [x] 验证跨来源/重复输入/委托/重启/全局概览去重，A→B→A、真实状态变化与账号范围隔离，完成构建和检查。

- 完整 Rust 回归 **112 passed，1 live ignored**；clippy `--workspace --all-targets -- -D warnings`、fmt 与 diff check 通过，新 Desktop 可执行文件构建完成。
- 去重只影响新增事件，不自动删除历史；后台采样与用户/委托快照共用最近状态比较，已有记录引用保留并按 Run pin，重启仍有效。
- 当前 Desktop 有用户运行中的 Shell，未停止或重启；新逻辑在下一次 Desktop Agent 启动后生效。

## 当前 Desktop Provider / 模型补齐

- [x] 检查发现原有 ModelScope 配置位于默认 `~/.aTerminal`，实际 `.local/local-dev/agent-next` 配置为空；未找到用户所说的 OpenRouter 凭据或模型 ID。
- [x] 沿用已获授权的 ModelScope 凭据，通过配置 RPC 写入当前 Desktop，模型 `Qwen/Qwen3.8-27B`，全局及 Session 默认均为 `modelscope-qwen`，revision 4；配置校验及回读通过，用户 Shell 未重启。
- OpenRouter 若需切换，仍需用户提供模型 ID 和 API Key / 本地密钥路径。未声称已配置 OpenRouter。

## Agent 焦点、状态显示与能力验证（用户追加授权）

- [x] 原实现复现定期历史刷新把草稿焦点转移到 TextView；改为仅滚动坐标，不转移焦点。
- [x] 多轮刷新检查焦点、草稿、选区与随后 IME 输入；旧历史同一终端重复状态折叠，跨消息/页边界及 A→B→A 回归，共 2 项通过。
- [x] Android 构建/lint、iOS build-for-testing、生产 Agent 面板模型桩端到端通过；真实 ModelScope PTY 流程通过（6 次调用，约 113 秒）。
- [x] 用户指定目录新建 Terminal 并启动 Claude，原文确认停在目录信任页，保留进程；附着窗口及模型超时辅助收尾如实记录。最终 Agent completed。详见 [AGENT-FOCUS.md](AGENT-FOCUS.md)。
