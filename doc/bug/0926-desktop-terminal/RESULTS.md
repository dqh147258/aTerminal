# 实施与验证结果

2026-09-26，macOS，本仓库 `stable` Rust toolchain。

## 实现

- CLI 交互入口在启动/连接 Agent 前建立 terminal guard，reset/clear/home 当前备用屏，正常退出及失败恢复鼠标、raw mode、光标和主屏，不清宿主 scrollback。
- Renderer 按调用客户端的颜色能力输出 RGB、256 色或基础 ANSI 色；回看帧按 hash 判断重绘，不再依赖实时 revision 变化。
- Engine 暴露不可变带样式历史副本与分页，保留既有实时 Snapshot、History、Alacritty 清屏和备用屏语义。回看不移动 authority viewport。
- 本地 RPC 增加 Scrollback/ReleaseScrollback，ID 按客户端隔离，远程通道拒绝；副本有行数、单元数、内存、读取者数、单包和 30 秒失联回收约束。
- Desktop 滚轮/Shift+分页/只读浏览、输入返回实时、Esc 退出回看、mouse mode 透传和备用屏 alternate-scroll 分发完成。旧 Agent 一次提示，资源繁忙可重试。

## 自动化

`cargo +stable test --locked -p ai-terminal-engine -p ai-terminal-protocol -p ai-terminal-agent -p ai-terminal`：69 passed，1 ignored。忽略项为原有 `modelscope_live_terminal_agent`，要求真实模型凭据，与本修复无关。包含 10 项 CLI 单测、11 项 Agent 集成、2 项新增 Desktop PTY 集成、2 项原有 PTY、22 项 Agent 单测、18 项 engine、4 项 protocol。

最后调整 guard 到 Agent 初始化之前后，重新执行 CLI 单测和两组 PTY 集成：14 passed。其余模块未改动。

`scripts/test-host-terminal.py target/debug/aTerminal` 五组通过：`/bin/sh --colors 256`、`/bin/zsh --colors rgb`、`--vim --colors 256`、`--colors ansi`、`--startup-failure`。验证输出、颜色编码、清屏归位、备用屏恢复、canonical/echo 恢复；不加载用户 Zsh 配置或写用户历史。

新增双层 PTY 场景实际驱动 CLI 和后台：80 行彩色中文输出；SGR 滚轮和分页回到首行；后台追加输出不移动阅读位置；键入只转发一次；主备用屏切换退出旧回看；detach/reattach 历史仍在；`--watch` 不写入；宿主 resize 恢复实时；启动失败恢复屏幕。旧 Agent 模拟测试确认只发一次不支持的回看请求，且不发重启或关闭操作。

10,100 行输出后的边界检查：保留 10,000 行，早期裁剪符合现有引擎合同；80 列回看副本约 31.6 MiB，debug 构建本机单次捕获约 222 ms。这是进入浏览时的一次有界复制，翻页只传视口；不是性能承诺或零延迟声明。

受影响包 `cargo +stable clippy --locked ... --all-targets -- -D warnings`、`cargo +stable fmt --all -- --check`、`git diff --check` 通过；`cargo +stable build --locked -p ai-terminal --bin aTerminal` 构建交付二进制。

## 验证边界与升级

自动化模拟宿主环境并用终端引擎解码输出，不声称已完成真实 Terminal.app/iTerm GUI 视觉验收。GUI 场景及安全升级步骤见 [HANDOFF.md](HANDOFF.md)。未重启仍有活跃会话的正常 Agent；原启动脚本已指向新构建，内部滚动需匹配版本的后台才能生效。


## 追加完成：内部滚动条与移动端完整分页

用户确认采用内部滚动条，并回复“继续”。已增加最右列悬停显示、轨道点击、滑块拖动及越界夹紧。回看期间保持可见，底部释放后恢复实时；只修改显示副本，不改变 PTY 尺寸和权威网格。宽字符覆盖/恢复与同 revision 重绘均有回归。

移动端原根因已确认：`read_history` 固定最近 200 行、原生页面无分页入口。新 `read_history_page` 使用和 Desktop 相同的冻结历史副本，按 200 行分页，含捕获时当前屏幕；返回独立的总数、下一页、has_more 与实际裁剪标记。Android/iOS 增加“加载更早记录”“读取最新历史”和已加载/总数显示。远程 Scrollback/ReleaseScrollback 经过现有配对、账号与会话授权开放只读，视图 ID 仍按客户端隔离。异步分页不写入实时 Replica，也不会获得输入权。释放操作带 view ID，旧面板迟到的关闭动作不会删除新副本。

追加验证：

- 五个受影响 Rust 包全量 83 passed、1 ignored（既有真实模型测试）；最后补充隐藏滚动条恢复原字符的回归后，CLI 单测 13 passed、Desktop PTY 2 passed。
- 真实 PTY 拖动最右滑块到顶/底、拖出轨道、悬停后点击轨道均通过；原有键入、只读、resize、备用屏切换回归通过。
- 200、450、10100 行分页与 Desktop 同一副本首行/总范围匹配；分页中追加输出无重漏。真实加密远程只读配对读回 HISTORY_000..HISTORY_449 共 450 行，顺序与数量完全一致，仍无法输入。
- Clippy（五包、all-targets、-D warnings）、fmt、diff check 通过。Desktop binary 重建完成。
- Android arm64-v8a/x86_64/x86 三个 Rust WebRTC 库、UniFFI、Debug APK、测试 APK、Lint 全部构建通过。
- iOS 设备 aarch64 与模拟器 x86_64 共享库、XCFramework、Swift/模拟器 App 构建通过。原 `build/xcode` arena 写入失败，改用 `/tmp/aterminal-history-xcode` 后成功；没有更改用户 Xcode/模拟器全局状态。保留本地开发 Server URL 和公共 CA 构建配置。

产物：`target/debug/aTerminal`；`apps/android/app/build/outputs/apk/debug/app-debug.apk`；`/tmp/aterminal-history-xcode/Build/Products/Debug-iphonesimulator/aTerminal.app`。未安装到用户设备，未重启正常 Agent，GUI 手势确认见 HANDOFF。


## Android 主画面直接滑动历史修复

用户明确要求主画面可上下滚动读旧输出，并授权 `emulator-5586` 安装与实际验收。现场旧版 98×47 网格全部位于容器内，UI hierarchy 纵向 `scrollable=false`，主画面没有历史请求；横向仍可滚动。这是此前仅补独立历史面板留下的路径缺口。

现增加 `read_history_viewport` FFI 读取带样式冻结视口，不污染实时 Replica。Android `TerminalScrollView` 区分纵向历史拖动、现有超高网格平移和横向平移，兼容触摸与鼠标滚轮；`TerminalScrollback` 合并正在读取期间的手势并丢弃迟到响应；`TerminalView` 保留实时帧与回看帧两套状态，回看期间继续消费增量。输入、尺寸变化与会话切换返回实时，长按复制当前显示内容。实现时还修正了 Kotlin receiver 作用域中误读 `View.overlay` 的条件，显式使用 Activity 的弹层状态。

最终设备自动化：`scripts/test-android-scroll.py --serial emulator-5586 --output .local/mobile-scroll-check/verified` 成功，使用真实加密远程连接与隔离 PTY。实际拖到 `UI_LOG_000`、后台追加 `__LIVE_APPEND__` 时阅读不动、向上拖回实时、横向平移、输入返回实时且 `__TYPED_ONCE__` 仅执行一次、鼠标滚轮回看全部通过；正常账号 preferences 完全一致。截图 live/history/returned 与 results.json 保存在该本地目录，已视觉检查。`DisplayConsistencyTest` 3 项设备绘制一致性回归通过。

共享核心单测 11 项、真实只读历史视口 RPC 集成（含旧会话拒绝、实时 Replica 不改变）通过；Clippy/fmt/diff check 通过。Android 三 ABI WebRTC 库、FFI、APK/测试 APK/Lint 构建成功，最终 APK 已安装 emulator-5586。只操作该模拟器，正常 Agent 未重启。为保持共享 FFI 一致，iOS 设备/模拟器库与 XCFramework 亦重新打包；未改动或宣称验收 iOS 主画面历史手势。


## 2026-09-27 操作系统重启后复验与收尾

用户授权继续，并允许自行重启虚拟机。使用 android-emulator-control 启动原 Android 16 AVD（aiterminal_api36_test / emulator-5586），保留数据；启动脚本将普通 ramoops 日志误报为失败，但后续进程身份、ADB device 与 sys.boot_completed=1 三项检查确认成功，未重复启动或改动兼容参数。

复跑 `scripts/test-android-scroll.py --serial emulator-5586 --output .local/mobile-scroll-check/reboot` 通过。报告确认 main_view_history、continuous_live_replica、horizontal_pan、typing_returns_live_once、mouse_wheel、user_account_preserved 全部 true；已检查历史截图确实显示 UI_LOG_000。设备 DisplayConsistencyTest 3 项通过，共享核心真实只读 RPC 集成 1 项通过，fmt/diff check 通过。测试临时 Agent/账号与 adb reverse 均清理，最终 APK 保留安装，原用户账号未清除，模拟器保持运行并重新打开正常 App。

本轮未重新部署重启后正常业务 Server/Agent；隔离验收不依赖其运行状态。Android 主画面修复已完成；iOS 主画面手势未在本轮修改或设备验收，独立历史分页保持此前实现。

2026-09-27 提交前工作区审查已完成：修正 Android 拖动跨网格跳行、迟到历史视口恢复旧尺寸，以及响应 view ID 校验；85 项 Rust 测试、5 项 Android 设备边界/绘制测试、完整手势测试与两端构建通过。详情见 [REVIEW.md](REVIEW.md)。
