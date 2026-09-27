# 工作区提交前审查

- Date: 2026-09-27
- Result: 已修正发现的问题；未发现剩余阻断提交的问题。
- Authorization: 用户要求审查整个工作区，发现问题修正后提交。

## 发现与修正

1. **Android 同一次拖动进入历史时可能跳行。** `TerminalScrollView` 原先只在 ACTION_DOWN 和历史滚动期间更新 lastY；先平移超高实时网格、再进入历史时，会把之前平移的距离重新计入历史。现在在普通平移与阈值判定期间也更新基准。设备回归用 1000px 内容/300px 视口，先平移 200px，再移动 10px，确认只增加 1 行历史。
2. **迟到历史视口可能恢复旧尺寸画面。** `read_history_viewport` 原先只校验会话 generation，未校验较新实时快照的 dimensions_epoch/备用屏模式。现在拒绝已失效的历史响应并释放副本；Android 绘制入口另校验 rows/cols，覆盖核心返回后、UI 回调前发生 resize 的窗口。Rust 覆盖尺寸/模式变化及订阅暂时落后的合法响应，Android 验证旧历史帧不会覆盖 resize 后画面。
3. **历史分页未校验响应 view ID 与请求一致。** 增加非零请求 view ID 与响应 ID 的一致性检查，避免跨副本拼接文本或定位错误视口。

## 审查范围

Desktop 入口/退出、颜色量化、输入与滚动条；权威引擎和有界历史副本；本地/远程 RPC 与只读配对授权；移动端实时 Replica 隔离、分页与 Android 手势/生命周期；Android/iOS 界面；回归脚本与文档。保持既有未提交改动，未引入部署或用户会话重启。

## 验证

- 五包 Rust 全量：85 passed，1 ignored（原有真实模型凭据测试）。
- 五包 Clippy `--all-targets -- -D warnings`、fmt、diff check 通过。
- 5 组 host PTY（sh/256、zsh/RGB、Vim/256、ANSI、启动失败）通过。
- Android 三 ABI 共享库、APK、测试 APK、Lint 构建通过。
- emulator-5586：TerminalScrollTest 通过；DisplayConsistencyTest 5 项通过，包含本次新增的两个边界测试。正常账号保留。
- iOS 设备/模拟器共享库、XCFramework 和模拟器 App 编译通过；未声称完成 iOS 真机手势验证。

本轮日志和设备报告：`.local/review-terminal/`（Git 忽略）。产物为最新 Android APK 和 `/tmp/aterminal-review-xcode/Build/Products/Debug-iphonesimulator/aTerminal.app`；生成产物与本地测试配置不提交。
