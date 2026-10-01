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
