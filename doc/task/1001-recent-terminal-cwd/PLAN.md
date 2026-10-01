# 移动端最近 Terminal 工作目录

- Status: Completed
- Updated: 2026-10-01

## 目标与范围
Desktop 持久化最近真实终端目录，Android/iOS 创建会话时可选；保留原手填及默认创建协议。最多 20 项，去重、最近使用排序，不做频次排序。不操作服务、模拟器、Downloads，不改 TerminalAgentWorkflowUiTest.kt，不 commit/merge。

## 当前状态与证据
`local::Request.cwd` 已支持创建目录，`service::dispatch(Create)` 直接给 PTY cwd；`SessionInfo.cwd` 是初始目录，不能作为运行时 cwd。`process::cwd` 通过 PID 身份验证后读 OS cwd。Android MainActivity 当前要求手填；iOS WorkspaceScreen 支持留空默认。最新 Android 主题在 NativeUi.kt，参考 0930-app-settings-redesign/UI_REFERENCE.md；iOS 使用 WorkspaceStyle。

## 方案与执行
本轮任务明确授权完整实现，并明确允许记录授权后直接实施，无需重复许可。协调者随后确认已审阅范围：20 项、去重、最近排序、按账号隔离与 Create.cwd 直传均在授权内；构建只使用本工作树，主 checkout 构建和模拟器由协调者统筹。
1. Desktop 在成功启动时记录已解析目录；后台以有界频率采样身份验证后的 OS cwd，仅变化时更新最近次序。按账号隔离，私有原子文件保存，最多 20 条。
2. List Reply 新增可选 repeated recent_directories 字段；旧端忽略字段，新 mobile 对旧 Desktop 得到空列表。沿用 Create.cwd，无新增 shell 命令或协议操作；Desktop 验证目录并返回错误。
3. mobile-core 提供最近目录接口。Android/iOS 创建界面复用现有主题与信息行，保留手填、取消和失败草稿；异步加载按连接身份保护。
4. 增加持久化、去重/上界、隔离、校验、协议兼容等关键测试，并运行相关 Rust 测试及移动端可行静态/编译检查。

## 验证
执行 Rust 相关 crate 的定向测试及编译；生成 UniFFI 并检查 Android 编译、Swift 语法。协调者独立 review、同步并完成模拟器 UI 验收；HANDOFF 列明场景与未执行边界。

完成结果：Desktop/protocol/mobile-core 与 Android/iOS 实现完成，Desktop 27 项、协议 5 项、mobile-core 12 项测试通过；真实 cwd 补充回归通过；Android Kotlin 编译与 iOS 15 simulator target 的全部 Swift 源码类型检查通过。详见 RESULTS.md / HANDOFF.md。实现验证阶段未 commit/merge、未操作现有服务或模拟器。协调者现已确认代码及验证已 review，并明确授权本轮以 `[未Review]` subject 提交约定 9 个源码和必要任务文档用于主 checkout 正式测试；本执行者不 merge。

## 风险与回退
OS cwd 不可用平台仅记录成功创建目录，不将终端输出或客户端猜测视为真实 cwd。最近目录可能后来删除，创建必须失败且保留选择。新增 protobuf 字段可被旧端忽略；回退代码不需迁移旧数据。

## 未决问题、歧义与确认
None.
