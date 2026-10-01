# 实现与验证结果

日期：2026-10-01。工作树：`/Volumes/Code/public-worktree/aTerminal/1001-recent-terminal-cwd`。实现验证阶段未 commit/merge；协调者已 review 并授权本轮以 `[未Review]` subject 提交任务文件用于正式测试同步，由协调者合并；未写主 checkout 的 bindings/build，未重启服务、未操作模拟器、未改 Downloads 或 TerminalAgentWorkflowUiTest.kt。

## 行为和数据

- Desktop 成功创建后记录已 canonicalize 的目录；独立后台线程每轮间隔 2 秒采样现有 `process::cwd`（PID/start identity 校验 + OS cwd），只在目录或所属账号变化时更新次序，不从终端文本、OSC、客户端猜测或 `SessionInfo.cwd` 的旧初始值采纳运行时 cwd。OS 无法确认身份/不支持查询时跳过；仍可记录成功创建目录。
- `data/recent-directories/<blake3(owner)>.json` 按 Desktop state dir 与账号隔离。无账号旧配对使用空 owner 的独立文件。最多 20 项，规范路径去重、最近使用在前；0700 目录、0600 原子 JSON。关闭会话后记录保留；不做频次排序。短于采样周期的瞬时 cd 不保证捕获。
- 创建仍是 `Create.cwd` → Desktop 校验目录 → PTY `CommandBuilder.cwd`；不拼接 Shell 命令。空字符串继续使用 Desktop 进程 cwd。不存在、非目录、失效选择或 PTY 启动失败通过既有 error 返回，不创建替代目录或回落默认。
- Android 新建会话使用 NativeUi 的 Palette / settingsRow；iOS sheet 使用 WorkspaceStyle / FieldShell / PrimaryButton。均有默认目录、最近选择、原路径手填、取消、busy、读取失败提示、创建失败保留草稿。保留完整路径空格/特殊字符，选择后仍需点创建。创建成功后 List 刷新失败不会误报创建失败。
- 最近目录只在本次创建界面内存显示，不在手机另存跨账号缓存。关闭/连接身份改变时忽略过期回调。

## 协议兼容

仅新增 `local::Reply.recent_directories`（repeated string，protobuf tag 16），List 填充；没有新 Operation、没有改 Request/Create 字段。新 mobile-core 的 `recent_directories()` 发原 List：旧 Desktop 无字段时得到空列表，仍可默认/手填创建；旧手机忽略新字段。旧协议兼容有编解码测试。UniFFI 新增方法需要协调者正常重生成 Kotlin/Swift 并同步 native library，生成产物不提交。

## 验证证据

| 检查 | 结果 |
| --- | --- |
| `cargo test --locked -p ai-terminal-agent -p ai-terminal-protocol --lib` | 27 Desktop + 5 protocol 全部通过 |
| `cargo test --locked -p ai-terminal-agent recent_directory_create --lib`（补充最后检查） | 真实 PTY cwd、Shell cd 采样、错误身份/已退出进程拒绝、特殊字符路径、失效/非目录拒绝、默认 cwd、idle MRU 不重排通过 |
| `cargo test --locked -p ai-terminal-mobile --lib` | 12 项全部通过 |
| `python3 scripts/prepare-bindings.py` | 本工作树 Rust mobile/bindgen 编译与 Kotlin/Swift 生成通过 |
| `env JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' ANDROID_HOME=/Users/carl/Library/Android/sdk ./apps/android/gradlew -p apps/android :app:compileDebugKotlin --console=plain` | BUILD SUCCESSFUL；只有现有 API 弃用及同类 resize 弃用提示 |
| `xcrun swiftc -typecheck -swift-version 5 -target arm64-apple-ios15.0-simulator -sdk /Applications/Xcode.app/Contents/Developer/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk -module-cache-path /private/tmp/aterminal-recent-cwd-swift-cache -I build/bindings -Xcc -fmodule-map-file=build/bindings/ai_terminal_mobileFFI.modulemap build/bindings/ai_terminal_mobile.swift apps/ios/aTerminal/*.swift` | 全部 App 源码类型检查通过，无输出错误 |
| `git diff --check` | 通过 |

真实 cwd 测试在默认沙箱的首次运行失败，原因已复现为 `/bin/ps: operation not permitted`，并非产品回归。经自动许可在沙箱外对本工作树执行同一隔离测试后通过；没有放松产品进程身份校验。测试只创建临时 state dir 和测试 Shell，正常路径显式关闭并 join recorder。

未执行 Android/iOS 安装、设备/模拟器视觉验收或完整 mobile native 平台构建；这些按协调者指令交由主 checkout 统一完成。

## 主 checkout 功能验收（2026-10-01）

已通过 Git 集成并构建 Android 三个 ABI、Debug/AndroidTest APK 和 Lint；iOS Debug 模拟器 App 编译链接通过，未安装或执行 iOS UI 测试。

Android 16 `emulator-5586` 的正常账号与真实加密 RPC 验收通过，见 [完整报告](evidence/full-final/results.json)。选择已被删除的最近目录后，Desktop 返回 `working directory is unavailable`，输入与按钮保持可重试且没有创建新 Session；两次选择有效目录创建后，`SessionInfo.cwd` 与真实 OS cwd 都为 `/Users/carl/Downloads/Temp2026/Temp10/test-1001`，MRU 中只有一项且位于首位。取消不创建，会话与账号身份保留，测试仅关闭自身创建的两个会话。

实际截图发现并修复了固定 80% 弹窗空白、IME 覆盖部分按钮触摸区、以及收起 IME 后高度预算依赖受限 Dialog 几何的问题。最终使用 Activity WindowMetrics/Insets 与标题/页脚独立测量，短列表保持 WRAP_CONTENT、长列表滚动，保留既有主题与标准按钮；监听器随 dismiss 解绑。[普通画面](evidence/full-final/recent-directory-normal.png)、[IME 画面](evidence/full-final/recent-directory-ime.png)。完整按钮 bounds 验证通过，收起后的 visible height 恢复到 2138。

失败尝试保留在 evidence 各目录，最终报告未覆盖它们。`functional-only` 单独完成了不含 IME 的真实创建逻辑验证；`full-final` 同时覆盖创建与 IME。验收器另修正启动连接等待、内容 ScrollView 定位、以及 Account HTTP 后保存轮换令牌，保留正常身份。

720 × 1280、density 320（360 × 640 dp）、font_scale=1.3 的同一完整真实 UI 测试也通过，见 [小屏大字体报告](evidence/small-large-font/results.json)。测试后恢复原 1080 × 2340 / density 440 / font_scale=1.0 及原硬件键盘 IME 设置；移除测试专用已失效目录的 MRU 项，只保留真实目录与原会话。字符动画 Session 保持运行。

## 文件清单

- `crates/desktop-agent/src/recent_directories.rs`：新增持久化/规范路径/上界/MRU 与关键测试。
- `crates/desktop-agent/src/service.rs`：验证创建 cwd、按 owner 记录、List 回复、独立采样器及真实 PTY 回归。
- `crates/desktop-agent/src/lib.rs`：注册新模块。
- `crates/desktop-agent/src/state.rs`：现有原子私有 JSON 写方法开放为 crate 内复用。
- `crates/protocol/src/local.rs`：兼容字段及新旧 Reply 编解码测试。
- `crates/mobile-core/src/remote.rs`：最近目录 FFI 读取方法。
- `apps/android/app/src/main/java/com/yxf/aterminal/MainActivity.kt`：最近目录创建 UI 与连接/失败处理。
- `apps/ios/aTerminal/aTerminalApp.swift`：异步最近目录读取、创建完成回调和失败处理。
- `apps/ios/aTerminal/WorkspaceScreen.swift`：共享风格创建 sheet。
- `doc/task/1001-recent-terminal-cwd/`：PLAN / TODO / RESULTS / HANDOFF / checkpoint。
