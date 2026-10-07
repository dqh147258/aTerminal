# 远程屏幕完整任务审查与修正

原始目录 `/Volumes/Code/My/aTerminal`，原始分支与集成目标 `main`，基线 `a3a61cfd4b7b66e19d4dc2003f394d8a9b7d4c19`。管理任务 `20261007-163526-008-remote-screens`，三个子任务均使用 `gpt-6.1-sol`，没有新增第四个任务。

用户明确确认当前连接电脑的全部显示器，并授权按计划直接执行。此记录在保留的原始 checkout 中，位于待清理工作树之外。

审阅了全部既有任务提交及从基线到集成结果的完整差异：

| 提交 | 内容 |
| --- | --- |
| `db1b46870eef8eba21cc7c565a0fdd9cde1193e7` | Android 原生入口、列表、查看与生命周期门控 |
| `ae607d50344e4771bfa4a4653a9432a948f26239` | 协议、Desktop 采集、异步加密 RPC、mobile-core 与 Linux CI 依赖 |
| `7f11b865285b9b909edb32e8fb0cd3c284713e11` | iOS 页面、模型、配对渠道及检查 |
| `e8f8b6bdeff3e636b7d17742e6db64c255b23754` | iOS 完整错误链的分类顺序修正 |
| `e651dd4` | core 集成；无冲突解决改动 |
| `d2682de` | iOS 集成；无冲突解决改动 |

本次后续修正的生产代码、测试、使用说明、依赖记录和本文也检查了提交前差异。没有改写子任务历史中的 Pending Review 标记；子任务通过测试不等同于代码已审查。

## 确认的问题与修正

1. 采集若直接走同步 stream RPC 会阻塞终端。core 使用独立异步屏幕槽位和独立本地 Client；加密 relay 测试阻塞模拟采集期间，终端 List/History 仍及时回复。采集不持有 Host/会话锁，超时后仍保留全局采集许可，避免堆积不可取消的工作。
2. 屏幕错误的 anyhow context 经 RPC Display 序列化会丢失原因。core 在屏幕 RPC 边界保留完整链，测试覆盖 timeout、permission 和 Wayland 原因传播。
3. iOS 初稿只允许账号连接，阻止了仍可使用的配对连接。以配对 UUID 和连接代次固定渠道，配对渠道无需 account.export，账号渠道保留账号/服务地址/设备校验；模型检查覆盖配对访问与渠道切换。
4. Android 初稿图像边界偏宽，已按协议收紧至 JPEG 120 KiB、base64 160 KiB、JSON 164 KiB 和帧边 1920。两端均在后台核对真实 JPEG 类型、实际尺寸与元数据，失败不持续重试。
5. Windows 条件分支只有 Linux 修改的 mut 变量会触发 unused_mut；core 改为按平台定义尺寸。Linux 用原始 RandR 像素尺寸，避免逻辑缩放尺寸的取整误差。
6. 完整错误链含通用录屏提示时，Android 会将实际 timeout 当作权限问题。优先识别具体 busy/timeout/unknown/Wayland，再处理权限；新增 JVM 回归验证复合错误链。iOS 同类修正在 `e8f8b6b` 中。
7. iOS 真正运行时，外层 VStack/ZStack 的 accessibilityIdentifier 覆盖了“选择设备”和图像标识。移除不需要的容器标识，保留独立控件标识。原先两项 UI 流程均失败，修正后设备入口和图像读取恢复。
8. iOS 原生 Menu 的“切换显示器”在实际 XCTest 运行中没有可点击位置，三次 AX 点击均失败。使用已有 ToolButton 返回显示器列表，与 Android 流程一致；真实 UI 测试重新选择外接屏、返回和关闭均通过。

审查了能力协商、只读配对授权、会话/控制权独立性、单请求与过期响应、后台取消、内存/帧/重放预算、采集进程期限、绑定签名和原生打包。未发现仍待修正的确定缺陷；没有以猜测修改其他功能。

## 验证与证据

Medium 验证，以下命令在原始集成 checkout 执行：

- `cargo +stable test --locked --no-default-features -p ai-terminal-agent -p ai-terminal-mobile -p ai-terminal-protocol --lib`：Desktop 70、mobile 15、protocol 6 通过，2 项依赖真实桌面的 ignored 检查在 core 子任务单独通过。日志 `build/remote-screens-rust-tests.log`。
- 同组包 `cargo +stable clippy --locked --no-default-features ... --all-targets -- -D warnings`、`cargo +stable fmt --all --check` 通过。日志 `build/remote-screens-clippy.log`、`build/remote-screens-fmt.log`。core 默认 WebRTC 编译也通过。
- `python3 scripts/prepare-bindings.py --toolchain stable` 生成真实 Kotlin/Swift 绑定。`scripts/build-mobile.py` 构建 Android arm64-v8a/x86_64/x86 和 iOS x86_64 Simulator/aarch64 设备库，均启用 WebRTC。本机 stable 正是 Rust 1.94.1，复用其已安装移动 target，没有修改全局工具链。
- Gradle `:app:testDebugUnitTest :app:assembleDebug :app:assembleDebugAndroidTest -PauthorizationUiFixture=true`：15 项 JVM 通过并生成隔离 fixture APK。首次 offline 构建缺少新增 org.json 测试依赖，正常下载后再 offline 构建通过，不更改全局仓库配置。
- Android 16、只读/无窗口 `aiterminal_api36_test` / `emulator-5586`：独立包 `com.yxf.aterminal.authorizationfixture` 的 `RemoteScreensUiTest` 7 项和既有 `WorkspaceReconnectTest` 6 项，共 13 项通过；包含实际原生库的新接口调用与 JNI/绑定一致性。日志 `build/remote-screens-android-ui.log`。正常 Android App 未安装、清数据或替换账号；正常包仅构建，见 `build/remote-screens-android-normal-build.log`。
- Android 模拟器启动脚本误把正常 ramoops 日志判作启动失败；随后 status 已核对同一进程、AVD、ADB device 与 boot_completed。没有重复启动、更改 AVD 或清数据。
- `python3 scripts/check-ios-remote-screens.py`：生产解析/生命周期/配对/错误检查与 Xcode source wiring 通过。日志 `build/remote-screens-ios-checks.log`。
- `scripts/package-ios.py --simulator-target x86_64-apple-ios --replace` 后，xcodebuild 在隔离 iOS 17.5 Simulator `079C5369-F052-45A8-A767-70B1A9FA6707` 完整构建并执行两项新 WorkspaceUITests，最终 2/2 通过。结果 `build/remote-screens-ios-ui-final.xcresult`、日志 `build/remote-screens-ios-build-final.log`。导出并查看 `build/remote-screens-ios-fit-view.png`，等比例图像、两侧留白和顶部按钮布局正常。fixture 图像不代表真实采集。
- core 子任务真实 macOS 检查：4 个显示器元数据；主屏 JPEG 1200×675、72,976 bytes、0.91 秒，成功解码并符合预算。测试只记录尺寸、大小和时间，不提交屏幕像素。
- Desktop CLI 构建 `cargo +stable build --locked -p ai-terminal --bin aTerminal`；不重启当前用户的 Desktop Agent 或结束 Shell。

必要检查全部通过。最终提交后做必要的集成核验，最终测试提交哈希与结果写入 task complete 记录及最终交接；本文不引用自身提交哈希作为审查依据。

## 已知支持边界

这是持续刷新的只读 JPEG 查看，没有新增远程鼠标/键盘控制。两端需与新版 Desktop 配合，旧版 Desktop 会明确提示升级。macOS 需允许 Desktop 录屏；Linux 当前仅支持 X11，Wayland 明确提示暂不支持。源屏幕超过 16M 像素时返回大小限制，原生库内部复制不等同于严格进程 RSS 上限。

未做物理手机、Linux/Windows 实际桌面或公网 WebRTC 画面验收，不把构建、模拟器或 fake/fixture 结果描述为这些验收。原生显示器 ID 可因重启/热插拔变化，此时刷新列表重新选择。当前没有基于实际失败证据的未决 bug finding；上述未测场景保留为验证边界。

## 集成与清理

源分支/提交为 `worktree/1007-remote-screens/android` / `db1b4687`、`worktree/1007-remote-screens/core` / `ae607d5`、`worktree/1007-remote-screens/ios` / `e8f8b6b`，均已通过 Git 合并到原始 main。清理前再次检查各源提交是最终 main 祖先、工作树无改动、没有其他任务占用；验证结果、生成绑定/原生库和本任务截图/日志保留在原始 checkout。子工作树的临时静态检查替代签名、公开依赖解包和编译缓存无需保留，不作为产品或测试包。

只在最终提交验证通过后关闭三个子任务的 Codex/辅助 iTerm pane，用普通 git worktree remove 和 branch -d 清理，再仅 rmdir 已空的 `1007-remote-screens` 目录；保留 project `.worktree-tasks` 历史和原始 main。
