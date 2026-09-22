# 账号登录与终端输入调研

调研日期：2026-09-22。阅读本地源码、GitHub 官方仓库源码与标准原文；没有将搜索摘要当作实现证据。星数来自当天 GitHub API，仅用于满足成熟/高星项目筛选，不代表性能测量。源码链接指向当前分支，后续可能变化。

## 结论

建议保留现有原生 UI 与桌面权威状态架构，吸收 VS Code 的事件驱动/流控、xterm.js 的输入回显优先与变化行渲染、Mosh 的状态更新合并。账号登录管理身份和设备归属，设备认证与端到端加密在后台完成；用户不再复制配对 key。

SSH 不会消除 RTT；VS Code 编辑文件时的本地文字呈现也不等同于远程 PTY 回显。可借鉴的是完整输入/输出链路，不能仅把传输换成 SSH 或把 View 换成 xterm.js 就宣称解决卡顿。

## 开源对照

| 项目 | 星数 | 核实到的实现 | 对本项目的启发 |
| --- | ---: | --- | --- |
| [VS Code](https://github.com/microsoft/vscode) | 192,764 | `terminalInstance.ts` 监听 onData/onProcessData；xterm 写入完成后 acknowledgeDataEvent；`terminalProcess.ts` 按高低水位暂停/恢复 PTY 输出 | 推送输出、持续消费、有界流控；不逐次拉取屏幕 |
| [xterm.js](https://github.com/xtermjs/xterm.js) | 21,210 | WriteBuffer 对用户输入后的首批回显立即解析；普通写入有时间预算；RenderDebouncer 合并行范围并用 requestAnimationFrame 绘制 | 降低交互优先路径等待，批量输出让出主线程，保留脏区到绘制端 |
| [Mosh](https://github.com/mobile-shell/mosh) | 14,505 | SSP 分别同步输入和当前屏幕状态；可跳过中间显示帧；有带确认和纠错的预测回显 | 更接近本项目的状态副本模型，可借鉴最新状态优先；暂不复制完整 SSP/预测器 |
| [Termux](https://github.com/termux/termux-app) | 61,214 | TerminalView 的 InputConnection 处理 commit/composition；TerminalSession 使用两个 IO 队列连接 PTY 与终端 | Android 输入法适配与 IO 解耦；Termux 本身不是远程网络性能对照 |
| [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) | 1,701 | iOS TerminalView 实现 UITextInput/marked text、输入 delegate，支持样式字体集合和变化行渲染机制 | 更小但直接适用的 iOS 参考；不为 UI 优化引入第二套权威 VT 状态 |
| [OpenSSH portable](https://github.com/openssh/openssh-portable) | 4,014 | 客户端事件循环处理输入/网络；SSH channel data + 窗口调节提供流式传输 | 不需为每个按键等待一条应用层回复；TCP/SSH 仍有流控与拥塞等待 |
| [Tailscale](https://github.com/tailscale/tailscale) | 36,738 | CLI 登录/退出/状态与设备网络身份分离，支持通过登录 URL 完成认证 | 借鉴“账号登录后设备可发现”的体验；不假设其实现等同 RFC 8628 |

### 一手来源

- VS Code：[终端输入与写入 ACK](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/terminalInstance.ts)、[PTY 流控](https://github.com/microsoft/vscode/blob/main/src/vs/platform/terminal/node/terminalProcess.ts)、[type-ahead 预测与纠错](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminalContrib/typeAhead/browser/terminalTypeAheadAddon.ts)、[Remote SSH 文档](https://code.visualstudio.com/docs/remote/ssh)。只依据公开终端源码，未声称完整 Remote SSH 扩展实现全部开源。
- xterm.js：[WriteBuffer](https://github.com/xtermjs/xterm.js/blob/master/src/common/input/WriteBuffer.ts)、[RenderService](https://github.com/xtermjs/xterm.js/blob/master/src/browser/services/RenderService.ts)、[RenderDebouncer](https://github.com/xtermjs/xterm.js/blob/master/src/browser/RenderDebouncer.ts)、[流控文档](https://xtermjs.org/docs/guides/flowcontrol/)。所读版本解析时间预算为 12ms；这是其实现参数，不是本项目应照搬的目标。
- Mosh：[官方机制说明](https://mosh.org/#techinfo)，包括屏幕状态同步、拥塞下跳帧及预测 epoch。Mosh 的预测比无条件 local echo 复杂，不应在密码/TUI 中直接显示尚未确认的原始输入。
- 原生输入：[Termux TerminalView](https://github.com/termux/termux-app/blob/master/terminal-view/src/main/java/com/termux/view/TerminalView.java)、[TerminalSession](https://github.com/termux/termux-app/blob/master/terminal-emulator/src/main/java/com/termux/terminal/TerminalSession.java)、[SwiftTerm iOSTerminalView](https://github.com/migueldeicaza/SwiftTerm/blob/main/Sources/SwiftTerm/iOS/iOSTerminalView.swift)。SwiftTerm 当前分支还含 Metal 渲染相关机制，不据此认定本项目必须上 Metal。
- SSH：[RFC 4254 §5](https://www.rfc-editor.org/rfc/rfc4254.html#section-5)、[OpenSSH clientloop.c](https://github.com/openssh/openssh-portable/blob/master/clientloop.c)。数据在窗口许可下流动，不是“一个字符一个同步 RPC”。
- 账号：[Tailscale CLI](https://tailscale.com/kb/1080/cli)、[RFC 8628 Device Authorization](https://www.rfc-editor.org/rfc/rfc8628.html)、[RFC 8252 Native Apps](https://www.rfc-editor.org/rfc/rfc8252.html)。RFC 8628 适用于备选第三方登录路径，内置账号首版不强行加入浏览器授权步骤。

## 本地链路与证据

当前回显路径：输入/发送按钮 → 平台串行 worker → Rust 锁内阻塞 request → WSS/WebRTC → bridge → 本地 IPC → PTY 写队列 → Shell/终端程序输出 → Agent 发布状态 → 手机下次 Poll → Replica → 完整 RenderFrame → 全屏绘制。

| 位置（项目根目录相对路径） | 确认事实 | 影响与证据边界 |
| --- | --- | --- |
| `apps/ios/AITerminal/AITerminalApp.swift:16,84,112,198` | worker 串行；50ms refresh；TextField 是页面 @State；发送成功后才清空 | 本地编辑可能触发无关画面更新；网络 ACK 慢时输入框清空也慢，需要分开测 |
| `apps/ios/AITerminal/TerminalView.swift:14,23,25,35` | updateUIView 无条件赋值；screen didSet 失效布局/绘制；每个 cell 多次访问计算字体/字宽 | 静态源码明确有多余工作，实际调用次数/耗时尚待 Instruments |
| `apps/android/app/src/main/java/dev/aiterminal/app/MainActivity.kt:20,117,174,184` | 单线程 scheduled executor，fixed delay 50ms，更新后全屏 invalidate/onDraw | 请求耗时计入刷新周期；Android 没有 iOS 的 SwiftUI 状态机制，不能套用同一根因 |
| `crates/mobile-core/src/remote.rs:47,98,187,314` | 持锁 block_on；Poll/Input/History 共享 Connected；输入序号来自上一条回复 | 串行等待与 RTT 放大；仅改平台线程数无法突破协议限制 |
| `crates/mobile-core/src/lib.rs:48` | delta 应用后重新 clone 全屏 cells 到 FFI Record | 网络省下的增量不等于 FFI/UI 增量；120×40 就是每更新 4,800 个 cell |
| `crates/remote/src/channel.rs:54,429,445` | 单 pending/单 cached 回复；单 request 等待；直连超时 500ms 后重试 | 单请求设计限制连续输入；500ms 是异常回退门槛，尚未证明卡顿时发生 |
| `crates/desktop-agent/src/remote_bridge.rs:114` 与 `service.rs:41` | 每次 clone Client 的 stream 为 None，新建 TCP | 可消除连接开销；不能在没有测量时称它是主要瓶颈 |
| `crates/desktop-agent/src/service.rs:115,240,435,532` | IPC 已 TCP_NODELAY；约 4ms 发布门槛、仅保存 16 个快照；Poll 才返回 delta/snapshot | 更新快于客户端时可能丢失基线并回全量，需要统计 fallback；不猜测 Nagle 问题 |
| `crates/server/src/lib.rs:50` 与 `crates/device-security/src/lib.rs:12` | pairs 数据模型；Invitation 含 relay token、PSK、桌面公钥和可选 CA | 账号改造必须调整归属和身份信任，不能只加登录 UI |

首要区分两种表现：**输入框文字本身卡顿**优先查主线程重绘/布局；**提交或功能键回显慢**优先查 worker 排队、request/ACK、Poll、网络路径与渲染。两者可能同时存在。

在无其他排队的近似情况下，当前单次输入回显包含输入往返、等待下一次 Poll 和 Poll 往返；实际取决于 PTY 输出到达时机，不可将固定公式作为测量值。单在途设计下若每键一个请求，理想吞吐上限约为 1/RTT：100ms RTT 时约 10 次/秒，持续输入会排队；目前普通文字是整段提交，这一推论主要适用于连续功能键及未来实时键入。

## 推荐取舍

先消除无变化重绘、缓存字体度量、复用 IPC，并取得对照 profile；随后用主动状态推送和有界异步输入消除轮询及逐请求等待；最后让 FFI/原生 View 保留脏区。字体缓存本身不能解决公网 RTT，推送本身也不能解决 UI 主线程卡顿。

继续桌面权威状态模型更接近 Mosh，适合多端查看与恢复；VS Code/xterm.js 常见的 VT 字节流/客户端解析并非本项目现有合同。完整改成 SSH + 双端模拟器会触及状态一致性、恢复与终端查询，应另行评估，不是此次性能修复的必要前提。

预测回显后置：VS Code 已有统计阈值、排除程序、预测撤回与匹配逻辑；Mosh 也有确认机制，均不是简单提前画字符。先降低可避免的本地/协议开销，再决定是否需要承担预测复杂度。

账号上以同账号自动发现替代邀请流，同时保留每设备密钥与撤销能力。不要向所有手机分发管理员 token，也不要把 Noise PSK 上传到 Server 再声称与旧邀请信任模型完全相同。

仅借鉴机制；若实施时复制或引入上游代码，逐项核对许可证并更新 THIRD_PARTY，不把高星视为许可证授权。

## 本轮验证范围

已核对上述代码与官方来源，完成 GitHub 星数查询。Python HTTPS 调用遇到本地 CA 配置错误后改用正常证书验证的系统 curl 成功读取；没有关闭 TLS 校验。

没有延迟实测、真机 profile 或代码修复结果。现有历史仅提供连通/输入 marker 证据，不能据此宣布性能达标。具体基准、实现顺序与验收门槛见 [PLAN.md](PLAN.md)。
