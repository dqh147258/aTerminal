# Desktop 终端滚动、历史、清屏与颜色修复

- Status: Completed
- Updated: 2026-09-27

## 目标与范围

修复 Desktop CLI 的滚轮回看、终端输出历史访问和宿主颜色兼容，明确并验证进入/退出 aTerminal 的屏幕生命周期。参考 tmux、Alacritty 和 xterm 的实际实现，沿用现有 `portable-pty + alacritty_terminal`，由 Desktop Agent 唯一解析 PTY。

用户补充：“我手动拖进度条往上发现之前历史没了，不知道是不是因为没有做滚动支持导致的历史看不到”。本轮按终端输出回看问题处理：已复现历史仍在 Agent、Desktop 无法访问；aTerminal 在宿主备用屏中按单元重绘，宿主自己的滚动条不能读取 Agent 历史。修复提供 aTerminal 内部滚轮/分页浏览，不承诺把内部历史映射到 iTerm/Terminal.app 的原生滚动条。保留已有 10,000 行存活会话历史上限；本轮不引入进程重启恢复、磁盘录制或 Shell 命令历史配置变更。

## 当前状态与证据

仓库 `AGENTS.md` 为空，调研开始时工作区干净。源码和成熟方案来源见 [RESEARCH.md](RESEARCH.md)。

| 问题 | 已确认的证据 |
| --- | --- |
| 滚轮失效 | `render.rs::TerminalGuard::enter` 开启宿主鼠标捕获；`input.rs::encode` 在内层应用未启用 mouse mode 时返回 `None`；`managed.rs::run` 没有本地历史滚动分支，`--watch` 还会跳过全部普通事件。 |
| 历史无法回看 | `Engine::new` 设置 10,000 行历史；`snapshot` 只编码实时网格；`history` 只返回最多 200 行纯文本，Desktop UI 未接入。不能拿显示帧变化拼凑历史，否则会漏掉两帧之间滚出的行。 |
| 颜色异常 | `render.rs::set_style` 无条件发送 `38;2` / `48;2`。内层 PTY 声明 truecolor 是引擎能力，不能据此认定外层 Terminal 也支持。 |
| 进入时未清屏 | 代码实际已有 `1049h` 及首次绘制 `0m;2J`，不是完全没有清屏。`TerminalGuard` 位于 Create/Attach/Poll 等初始化之后；当前观测不能确认用户看到的残留发生在哪个阶段。 |

2026-09-26 使用现有 `target/debug/aTerminal` 做一次隔离 PTY 诊断：8×60，临时 state dir，子进程 `/bin/sh` 输出红色标记和 40 行编号文本；模拟宿主 `TERM_PROGRAM=Apple_Terminal`、`TERM=xterm-256color`、无 `COLORTERM`。结果：发送 6 次 SGR wheel-up 后没有任何绘制输出；History RPC 仍有 34 行且含 `ROW_000`；输出含 `1049h`、`2J`、truecolor，不含 `3J` 或 256 色 SGR。结束后只停止临时 Agent。此诊断验证 CLI 输出协议，未证明真实 Terminal.app 的视觉效果。

## 方案与执行

执行批准：2026-09-26 用户回复“按照计划执行”。

实施完成：内部带样式历史回看、鼠标/备用屏分发、宿主颜色降级与入口清屏恢复均已实现。69 项自动化测试通过（另 1 项真实模型测试按原条件忽略），5 组 Shell/Vim/失败路径 PTY 检查、Clippy、格式与构建通过；最后入口时机调整后又通过 14 项 CLI/PTY 检查。结果见 [RESULTS.md](RESULTS.md)。实际 GUI 确认与有活跃会话的正常 Agent 升级见 [HANDOFF.md](HANDOFF.md)，未自动停止用户会话。

推荐补全现有终端前端，不更换 VT 引擎。tmux 的服务端会话/历史与客户端显示分离、Alacritty 的鼠标/备用屏/回滚三路分发，与现有架构最接近。直接代理 PTY 到宿主会破坏唯一查询应答和重连一致性；引入 tmux 作为必需运行依赖会改变跨平台部署和会话管理，本轮不采用。

1. **宿主能力与屏幕生命周期。** 为 Renderer 显式传入宿主颜色能力，复用 crossterm 能力检测（`COLORTERM` / `TERM`），把 RGB 映射到宿主支持的 truecolor、256 色或基础 ANSI 色。Apple Terminal 标准环境使用 256 色；明确声明 truecolor 的 iTerm 保留 RGB。不要根据后台 Agent 的启动环境选择客户端颜色能力，不降低内层引擎能力。提取可测试的进入/恢复序列：交互终端确认后尽早建立 guard，进入备用屏时 reset、clear、home；错误与正常退出均恢复宿主。只清当前备用屏，不发 `3J` 删除宿主历史，不向子 Shell 注入 `clear`。保留首次全量绘制和 attach 时的会话画面恢复。
2. **独立历史视图。** 在 Engine/本地协议/Agent 增加只读、带样式的历史视口读取，保留宽字符、组合字符和颜色；CLI 独立保存回看状态。采用类似 tmux copy-mode 的时间点回看快照：进入回看时捕获有界历史和实时屏幕，分页返回，浏览期间持续输出不推动已读内容；返回底部/输入时恢复最新实时画面。每客户端至多一个回看快照，限制行数、单元数和单包大小，切回实时、detach、客户端过期时释放；缩放/主备用屏切换时退出旧回看视图。实时 Poll/Replica 的 revision/hash 与回看帧分别管理，翻页必须触发绘制，即使实时 revision 没有变化。保持旧 `History` 纯文本接口及移动端实时快照合同；初版新操作在 remote bridge 禁止；追加第 7 步经授权调整为配对与会话校验后的只读访问。存活会话 detach/reattach 后从同一 Agent 历史重建视图。
3. **输入路由。** 普通 Shell 的 wheel-up/down 每次滚动 3 行，Shift+PageUp/PageDown 按页浏览；应用请求鼠标事件时继续透传原鼠标协议；备用屏无 mouse mode 时仅在 `ALTERNATE_SCROLL` 开启时按成熟实现发送方向键（补充相应 input mode 位）。Shift 滚动作为本地回看覆盖，不误送应用；只读 `--watch` 允许本地浏览但不写 PTY。回看时隐藏实时光标；普通键入/粘贴先返回实时，再原样发送一次。Esc 只在回看模式退出回看；实时模式维持原透传语义。
4. **回归与交付。** 增加必要的 engine/协议/CLI 单测和隔离 PTY 集成验证，更新 Desktop 使用与历史保留边界。记录真实宿主视觉确认的完成情况；不将注入环境变量的 PTY 测试称为已测试 Terminal.app。实施已按用户批准完成，TODO 和 RESULTS 记录验证证据。

### 追加：内部滚动条与移动端历史一致性

追加已完成：用户确认内部滚动条并回复“继续”。右侧悬停/点击/拖动已实现，Android/iOS 从统一时间点历史副本加载更早页面。五包全量 83 项测试通过，追加绘制恢复回归后 CLI 13 项与 Desktop PTY 2 项通过；Android 三 ABI、APK/Lint 与 iOS 设备/模拟器共享库、模拟器 App 构建成功。旧 Xcode 构建目录受限后使用隔离目录成功构建。产物与 GUI/升级交接见 RESULTS/HANDOFF。

用户继续报告 iTerm/Terminal.app 原生右侧滚动条不可用，以及移动端终端历史长度与 Desktop 不同；在说明宿主原生控件的协议边界后，用户明确选择“接受 aTerminal 内部滚动条（推荐）”。这作为内部滚动条方案的追加执行授权；移动端长度差异按用户追加缺陷要求一并修复。保留上一阶段完成与测试记录。

补充证据：`remote.rs::read_history` 固定请求 `history_limit=200`，Android `terminalHistory` 与 iOS `readHistory` 只读一次、没有上一页。450 行隔离输出下同款 History 请求只返回 200 行（ROW_243..ROW_442）且 truncated=true。旧 History 只含离开实时网格的行；Desktop 浏览副本同时包含实时网格，因此还存在尾部范围差异。当前 Browser 已收到 scrollback_total，但未保存总长度，更没有绘制或命中测试滚动条。

成熟方案：[tmux pane-scrollbars](https://github.com/tmux/tmux/blob/master/tmux.1) 支持内容区字符滚动条、modal/auto-hide 覆盖模式；[iTerm tmux integration](https://iterm2.com/documentation-tmux-integration.html) 依赖 `tmux -CC` 专用集成，不能作为 iTerm/Terminal.app 通用原生控件接管接口。

追加实施：

5. CLI 内容区最右侧增加内部滚动条。悬停右边缘时显示，进入回看后保持可见；点击轨道/拖动滑块定位，返回实时或离开边缘后自动隐藏。采用覆盖方式，不修改 PTY 尺寸或共享画面；覆盖宽字符时在本地绘制副本上保持完整宽字符约束。备用屏应用鼠标区不被常规拦截，Shift 可明确进入本地浏览。只读模式同样可拖动。滑块位置由实际 offset/total 计算，边界、小窗口和拖出轨道有确定行为；内部滚动条使用字符单元精度，不承诺宿主原生像素精度。
6. 复用已有不可变 Scrollback 数据源，增加最大 200 行的历史分页与总行数元数据。Desktop/移动端统一读取“保留历史 + 捕获时实时网格”的时间点副本，按权威物理行计数；`has_more` 与真正丢弃的 `truncated` 分开。保留旧 History/read_history 兼容入口，新 UI 使用分页入口，不用重复动态 offset 请求拼凑持续变化的数据。
7. 将 Scrollback/ReleaseScrollback 作为经过配对与会话授权的只读操作开放给远程客户端，维持客户端视图 ID 隔离、资源限制和实时 Replica 隔离。查询不需要输入权；跨账号、旧 view ID、断线重连/会话切换严格校验，关闭历史面板释放副本，过期明确提示重新读取。
8. UniFFI 暴露历史页合同。Android/iOS 历史面板显示可继续加载更早记录的入口、已加载/总行数和实际裁剪提示；保留选择复制和既有显示风格。页按时间顺序拼接，防重复请求/乱序/旧会话回包；新增历史不会使已打开分页发生跳行。旧 Agent 不支持新分页时明确提示升级，不把 200 行当成全部历史。
9. 聚焦测试拖动/轨道定位、显示恢复、应用鼠标隔离、200/450/10000 行跨页一致性与持续追加、真实只读远程授权和会话切换。重建共享 FFI 与 Android/iOS 编译，执行可用的自动化逻辑与构建检查；设备 GUI 验证条件沿用原计划，不未经确认操控用户设备。

### Android 主画面滑动补充修复

已完成：Android 主画面带样式历史回看、反向返回实时、输入返回实时和鼠标滚轮均实现。用户操作系统重启后，于 2026-09-27 自行重启 Android 16 emulator-5586，重新安装最终 APK 并复跑真实隔离手势测试与 3 项绘制一致性测试，全部通过；共享核心只读历史 RPC 回归亦通过。模拟器保持运行，已重新打开正常 App，账号未清除。证据见 `.local/mobile-scroll-check/reboot/` 和 RESULTS。

用户继续报告移动端无法滚动，确认只测试 Android，并明确允许在 emulator-5586 安装测试构建与隔离验收。现场截图/UI 层级确认：17:14 安装的旧版主画面为 98×47 实时网格；纵向 ScrollView 的 scrollable=false，实际上下滑动没有历史请求，横向容器仍可滚动。此前补充的完整历史位于独立面板，没有接入主画面手势；这是遗漏的使用路径。

10. Android 主画面纵向手势达到阈值后进入只读历史视口；横向保留原平移，触摸点击输入、长按复制与键盘输入保持既有语义。历史手势不映射成 Shell ↑/↓，不会执行命令。
11. 在共享核心增加带样式历史视口 FFI，复用已授权 Scrollback RPC，校验 session/epoch/generation 与 cursor ID，返回 frame/offset/total；历史画面不写实时 Replica。Android View 同时保留实时帧与显示帧，回看期间实时增量继续消费；回到底部或键入恢复实时，resize/切换会话使旧视图失效。
12. 在专用 Android 测试会话验收实际纵向拖动可跨越实时网格读取历史、持续输出不拉回、反向返回底部、横向/输入/只读/晚到响应边界。用户已授权 emulator-5586，本次可执行设备验证；不清账号数据、不操作其他设备或停止用户 Agent。iOS 本轮无用户复现证据，不猜测更改其手势。

## 验证

关键自动化合同：

- 大量输出与丢帧后历史仍完整；彩色/中文/宽字符正确分页；上下界有界；回看期间追加输出不移动已捕获内容；浏览不改变权威 snapshot/hash、光标、输入模式或手机画面。
- wheel/Shift+PageUp/PageDown、SGR mouse、备用屏 `1007`、`--watch` 与输入返回底部；翻页时实时 revision 不变仍重绘；resize、备用屏切换、detach/reattach 无旧画面污染。
- 宿主颜色矩阵：Apple Terminal 标准环境只输出 256 色序列，truecolor 环境保留 RGB；基础 ANSI 降级；颜色量化选择和样式 reset 可确定验证。
- 入口确实 reset/clear/home；预置宿主主屏内容在退出后恢复；不删除宿主 scrollback；失败路径恢复 raw mode、鼠标捕获和光标；内层 `2J` 与明确清历史 `3J` 保持 Alacritty 既有语义。
- 历史只读操作不扩展远程输入/管理权限；客户端离开/过期释放回看资源；超过消息/缓存边界明确处理；不为回看阻塞持续 PTY 输出。

预计执行 `cargo +stable test --locked -p ai-terminal-engine -p ai-terminal-protocol -p ai-terminal-agent -p ai-terminal`，以及受影响包 `cargo +stable clippy --locked --all-targets ... -- -D warnings`、`cargo +stable fmt --all -- --check`。用独立 state dir 扩展 `scripts/test-host-terminal.py`，覆盖 `/bin/sh`、`/bin/zsh`、Vim、启动失败、颜色环境与滚动场景；测试不读取或修改用户 Shell 历史。

已确认本机安装 Terminal.app 与 iTerm.app，尚未确认可可靠自动控制与读取其画面。本轮不操作真实用户窗口；实施后若可自动验证，依技能先确认是否需要宿主 GUI 实测；否则在 HANDOFF 写下精确的两个宿主确认场景和预期结果。Windows 宿主不在当前环境，仅报告实际完成的验证。

## 风险与回退

带样式回看跨 Engine/本地 RPC/CLI，必须保持实时状态和浏览状态隔离。快照只在进入回看时建立，按页传输，避免每 3 ms 复制全量历史；通过缓存上限与生命周期回收控制内存。

新 CLI 连接旧 Agent 时应继续正常实时交互，对不支持的新回看操作给出一次明确提示，不能谎称历史为空或循环失败；完整滚动能力需要匹配版本。不要自动重启用户运行中的 Agent，因为其存活 Shell 和内存历史会因此丢失。构建与验证使用隔离 Agent，实际升级条件在交付时说明。

256 色宿主对任意 RGB 存在量化，这是能力边界；不承诺与 iTerm 每个像素一致。当前“未清屏”未在 PTY 层重现，入口调整后若真实宿主仍残留，应根据现场序列/画面继续定位，不追加猜测性的终端私有 escape。

提交前审查：用户授权 Review 并修正后提交，已完成两处滚动边界修复和历史响应 ID 校验；最终检查见 [REVIEW.md](REVIEW.md)。

## 未决问题、歧义与确认

None.
