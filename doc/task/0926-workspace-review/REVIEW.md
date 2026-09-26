# 工作区审查记录

- Status: Completed
- Updated: 2026-09-26

范围：本次全部未提交的 Rust、Android、iOS、脚本与文档改动。用户已授权审查、修复问题并提交。

已确认问题：`AgentHost::submit` 在检查运行中已撤销的写入授权之前调用 `Store::accept_user_authorized`，被拒绝的追加消息仍会进入持久历史。修复需在去重之后、数据库写入之前验证新消息，保留成功请求重试的幂等性。

验证：完整 Rust 测试、clippy、fmt；Android debug/测试 APK 与 lint；iOS 模拟器 build-for-testing；脚本语法和 diff 检查。使用隔离测试环境，不重启现有 Desktop 或终端。

修复结果：新增消息准入检查在存储事务内、幂等检查之后运行；撤销授权后拒绝的消息不入历史，满队列下成功请求重试仍返回 duplicate。新增确定性回归测试覆盖重复拒绝、历史序号不变和已接受请求重试。另修正 iOS 字号范围文案为 6–24 pt。

验证结果：

- `cargo test --locked --workspace`：113 passed，0 failed，1 ignored（真实供应商测试）。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`：通过。
- `cargo fmt --all -- --check`、`git diff --check`：通过。
- `./apps/android/gradlew -p apps/android :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug --offline`：通过。
- `xcodebuild -project apps/ios/aTerminal.xcodeproj -scheme aTerminal -sdk iphonesimulator -configuration Debug -derivedDataPath /tmp/aterminal-review-xcode ARCHS=x86_64 CODE_SIGN_IDENTITY=- build-for-testing`：通过。默认双架构构建因本机现有 Rust 模拟器库缺少 arm64 失败；按项目本机构建脚本使用的 x86_64 架构通过。原 build/xcode 目录写入受限，使用独立临时目录。
- 本次修改的 8 个 Python 脚本均通过 AST 语法检查。

边界：未复跑移动端设备交互、Linux/Windows 或真实模型调用；上述构建结果不代表这些环境通过。
