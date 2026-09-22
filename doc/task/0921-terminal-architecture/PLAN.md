# AI Terminal 架构调研与实施方案

- Status: In progress
- Updated: 2026-09-22

## 目标与范围

实现 Server、Desktop Terminal、Android/iOS Mobile Terminal。桌面在现有终端中运行 `ai-terminal`，通过本机 PTY/ConPTY 执行真实 Shell；手机能监控、接管同一会话。Server 使用 Rust 和 Docker Compose；移动端使用共享 Rust 核心及各平台原生 UI。

已确认的用户决策：

- 优先强一致，接受明确的终端兼容范围。
- 后续内置 AI 是只读观察与终端操作助手，可读取当前系统权限允许的所有路径；不直接修改业务文件，通过 Codex 等其他工具执行任务，并可继续向其 TUI 输入。
- **第一版暂不实现 AI，只交付 AI 定义文档。** 第一版人工在手机上启动和操作现有 AI CLI，属于普通终端交互，可正常支持。

一致性指同一版本下的字符单元、颜色语义、光标、模式、行列尺寸一致；允许网络延迟、明确标注的离线旧画面和字体外观差异。断网时桌面继续运行，不能保证两块屏幕在物理时间上始终相同。

第一版不承诺任意宿主私有图形协议、无损接管现有 Shell 进程、无限历史、桌面关机后执行、多人同时写入。用户已批准按计划实施。

## 当前状态与证据

工作目录最初为空，没有 `AGENTS.md`、产品实现、测试和 Git 历史；`git status --short` 返回 `fatal: not a git repository`。调研阶段仅创建规划文档；执行阶段开始建立实现。

详细调查、证据和替代方案见 [RESEARCH.md](RESEARCH.md)，未来 AI 合同见 [AI-SPEC.md](AI-SPEC.md)。已读取官方协议、项目源码及平台文档：

- `portable-pty` 提供跨平台 PTY；Microsoft 明确提示 ConPTY 同步读写与关闭时的死锁风险。[S1–S2]
- `alacritty_terminal::Term` 提供 grid、damage、renderable content；终端状态同步仍需本项目实现。[S3]
- RFC 8831 支持可靠有序 DataChannel；同一 SCTP association 的通道共享拥塞窗口。默认 libjuice 不能保证 TURN/TCP/TLS 可达。[S5–S7]
- UniFFI 支持 Kotlin/Swift 并用于 Firefox；iOS/Android 后台执行有系统限制。[S10–S12]
- 当前 `libghostty-vt` 已可用，但官方仍声明 API 不稳定，不把旧的“尚不存在”判断作为选型依据。[S9]

P0/P1 基础实现、P2 安全远程基线、P3 本机直连/切换均已完成自动化验证；22 项 Rust 测试和 Clippy 通过。iPhone SE 模拟器已验证 Keychain 配对恢复、WebRTC 直连、读屏和真实输入；Android 双 ABI 构建通过但运行环境 offline。Compose Server 健康，20 对连接短测完成；证据见 TODO.md。发布门槛、公网网络矩阵及长期性能仍未完成。

## 方案与执行

2026-09-22 用户追加授权：本机 Docker 部署使用 7200–7210，固定地址 192.168.0.36，配好试用配置并用 iOS 模拟器测通。本次配置 HTTPS/WSS `192.168.0.36:7200`、回环健康入口 `127.0.0.1:7201`，以项目私有测试 CA 显式信任保留完整证书校验；复用现有 Server 数据卷和网络，保留可继续试用的会话/配对，验证读屏、输入、重启恢复。

该追加任务已完成：iOS 模拟器使用同一会话分别通过 direct 与 relay 输入验证，Keychain 恢复通过；当前保留正常只读试用视图，打开接管开关即可输入。23 项 Rust 测试、Clippy 与 Xcode 构建通过，详见 TODO.md 的 2026-09-22 记录。此完成状态不代表其余发布验收项已完成。

2026-09-22 后续用户要求“都关闭”并补齐本地调试文档：已停止项目容器、试用 Agent/Shell、指定 iOS 模拟器与窗口；保留数据和配对，新增可重复关闭脚本并归档失效会话 ID。当前运行环境为停止状态，完整重启/重建/验证/日志/关闭指引见 deploy/IOS-LAN-TRIAL.md。

执行批准：2026-09-21 用户回复“按照计划执行”。在说明 Windows 环境缺失、当前 P0 结果及后续门槛后，用户再次回复“继续”；据此继续 P1，Windows ConPTY/PSReadLine 验证延后为发布前必过项，不视为已通过。

推荐“桌面单一权威终端状态 + 本地增量绘制 + 手机状态副本 + 轻量 Server”。

| 组件 | 推荐实现及职责 |
| --- | --- |
| Server | Rust、Tokio、Axum、rustls、SQLite；设备鉴权、信令、WSS 密文转发；Docker Compose，coturn 可选 |
| Desktop Agent | Rust 后台进程；portable-pty、alacritty_terminal、会话生命周期、输入与尺寸控制 |
| Desktop CLI | Rust、crossterm；宿主内 raw mode、ANSI 增量绘制、本地 IPC；网络不进入本地输入关键路径 |
| Shared Core | Rust；协议、状态模型、同步、加密、传输，独立于桌面 PTY 依赖 |
| Android | Kotlin 原生页面、自定义终端 View；Rust `.so`、UniFFI；当前基线直接使用平台 View，未额外引入 Compose 运行时 |
| iOS | SwiftUI 页面、UIKit 终端视图；Rust `.a` 封装 XCFramework、UniFFI |
| WebRTC | 首选验证 libdatachannel C API/Rust 包装，关闭音视频；WSS/443 独立兜底 |
| 未来 AI | 只交付接口与权限定义；首版不实现模型调用、自然语言编排、工具适配器和自动监控 |

关键设计：

1. 桌面唯一解析应用 VT。桌面 CLI 和手机都消费同一版本化显示状态；应用查询回复由引擎唯一生成。
2. 一个会话一套 `rows × cols`。桌面默认拥有尺寸控制权；手机缩放/平移，显式移交后才能改变共享尺寸。
3. 多端可观察，单端可输入；本地可立即抢回。未来 AI 使用同一控制令牌，不另建输入后门。
4. 会话 epoch、状态 revision、输入序号、控制 fencing token 独立于网络路径。快照负责首次连接与恢复，输入在存活会话内去重，跨崩溃不承诺 exactly-once。
5. 最终产品优先 WebRTC 直连，按应用层 RTT、ACK 停顿与积压评估 WSS；中转不一定更快。
6. 统一端到端认证加密，Server 不持有终端明文。既有加密协议与库负责密码学，不自研算法。
7. 不实现通用多写入 CRDT、不自研 WebRTC 栈、不以屏幕视频流作为主数据协议。

执行细化：WSS 数据和 SDP/ICE 信令使用 Noise NKpsk0，WebRTC 使用经认证 SDP fingerprint 建立的 DTLS/SCTP，避免跨路径复用 Noise nonce。一个存活连接内通过请求 ID 和响应缓存处理直连超时重试；WSS 控制连接负责撤销/过期，断开会结束远程连接但不终止本地 Shell。TURN overlay 当前使用部署期凭据，短时凭据签发仍是发布加固项。

批准后的实施顺序：

1. **P0 技术原型。** 验证 PTY → 权威状态 → 宿主绘制；PowerShell/PSReadLine、Vim、Unicode、resize、终端查询；并验证 Android/iOS 的 Rust + libdatachannel + UniFFI 可构建和最小显示。
2. **P1 本地会话。** 后台 Agent、CLI attach/detach、控制令牌、统一尺寸、快照/增量、有限历史、进程生命周期与确定性测试。
3. **P2 安全远程基线。** Rust Server、设备配对、E2EE、WSS、双平台最小原生终端；先验证正确性。
4. **P3 WebRTC 优先。** STUN/ICE、可选 TURN、质量评估、双路径切换/去重/恢复、弱网测试。WSS 先实现是开发顺序，不改变最终直连优先策略。
5. **P4 首版发布。** 移动输入法/选择/后台恢复、兼容矩阵、低配基准、Compose 部署、五平台构建与打包签名；交付 AI 定义文档，不包含 AI 实现。

P0 未通过时先记录根因，再调整依赖或兼容合同，不以扩展页面代替基础验证。引擎替换、AI 提前进入首版等重大变化需重新审阅。执行清单见 TODO.md。

## 验证

关键自动化验证：VT 任意分块、UTF-8 边界、宽字符、备用屏、resize；快照/增量在重连、重复、乱序和路径切换后与权威状态 hash 一致；旧版本与过期控制令牌拒绝；输入不会重复写入，队列有界；CLI/网络断开不终止已 detach 的会话；ConPTY 关闭不死锁。

初始性能目标及可复现负载详见 RESEARCH：本地附加延迟 p95 ≤ 5 ms、p99 ≤ 15 ms；手机 120×40、30 次更新/秒时主线程帧工作 p95 ≤ 16.7 ms；Server 空闲进程 RSS ≤ 32 MiB，20 对中转连接、每对单向 64 KiB/s 时 ≤ 96 MiB。TLS/内核缓冲、TURN、容器和操作系统总内存分别报告。

环境检查：adb 无设备，devicectl 返回 No devices found，未发现可用 Windows/PowerShell 环境。本轮不运行手机真机测试。实施期先检查环境；有设备时按技能要求询问是否需要真机测试；不可用则另写 HANDOFF，给出宿主显示、输入法、移动后台/恢复等场景，不把手工步骤放进自动化测试或 TODO。

## 风险与回退

终端兼容性是第一风险：保证能力子集内的语义一致，不能承诺任意字体、Emoji 和私有图形像素一致。先通过嵌套绘制及 PowerShell 原型，再扩展产品。

网络中断时手机只是旧副本；远程恢复后重新校验控制权，本地不等待手机 ACK。拥塞时合并未发送的显示状态，必要时快照；不得丢弃任意 VT 字节或输入。

libdatachannel 带来 C/C++ 交叉编译和 MPL-2.0 义务；固定版本及工具链，提供 notices 与适用源码。直连路径未通过验证时可保留 WSS 可用版本，但不得宣称已完成最终 WebRTC 要求。

CLI 关闭可保留后台会话；机器睡眠/关机后不可继续执行。Agent 崩溃可能终止会话，屏幕快照无法恢复操作系统进程。

未来 AI 的“只读”限定于文件访问 API；终端操作仍有修改能力，必须经受控执行层委托外部工具。此边界在 AI-SPEC 中明确，首版不增加 AI 权限入口。

用户已明确一致性取舍及 AI 的范围/延期。硬件规格、模型供应商和首发系统最低版本可在对应阶段选定，不影响本次方案审阅。用户于 2026-09-21 明确回复“按照计划执行”，已批准整份实施计划。

## 未决问题、歧义与确认

None.
