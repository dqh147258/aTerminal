# iOS 远程屏幕

状态：Completed（2026-10-07，局部实现与分工内验证完成）。协调者分配的功能、局部实现、Medium 验证及工作流内提交已获授权；无新增范围或待确认决策，不再委派。完整 App build 和 fixture UI 执行属于已分配给协调者的整合阶段，见 [HANDOFF.md](HANDOFF.md)。

在 `WorkspaceScreen` 右侧工具条增加“远程屏幕”，复用全高面板和 `WorkspaceStyle`。当前 Desktop 的真实显示器列表包含名称、尺寸和主屏标记；未连接时提供选择设备入口。选屏后等比例 fit 展示，每次完成后约一秒继续获取 JPEG，可切换、返回列表、关闭；加载、空态、错误和重试均可见。仅依赖 RemoteTerminal 连接，不依赖终端选择或控制权。参考 Sirix 的选屏和 contain 展示体验，不修改其代码。

`RemoteScreenModel` 管理列表、帧和单个请求槽位，关闭、scene 非 active、设备/账号/连接代次变化使请求票据立即失效；已经进入同步 FFI 的请求等待返回后丢弃，不排队堆积。TerminalModel 的后台串行 worker 在 FFI 前验证票据及连接代次，并在后台解析 JSON、base64 和 JPEG。接口按已给定 Swift 合同调用，不改生成绑定、Rust 或 Android。

必要验收：Swift 语法检查、project wiring/diff 检查、使用实际生产模型的 host checks（JSON、非法 JPEG、单请求、切屏/关闭/后台/设备/账号变化、过期响应、错误重试）；少量 fixture UI test 覆盖列表→选屏→切换→返回/关闭及未连接设备入口，并做 Simulator SDK typecheck。完整 app build / UI test 执行由协调者在绑定同步后安排；当前机器 x86_64、iOS17.5、隔离 Simulator 079C5369-F052-45A8-A767-70B1A9FA6707 可复用。无真机测试：此次分工明确最终 Simulator build 归协调者，未提供可自动运行的物理设备及真实屏幕采集服务。fixture 不证明真实采集。

完成后审阅全部所辖 diff，提交 `[Pending Review, 1007-remote-screens/ios]`，交接 commit、验证命令及整合限制，不 merge 主 checkout。

实施补充：按协调者补充合同校验 JSON ≤164KiB、JPEG ≤120KiB、base64 ≤160KiB、帧宽高 ≤1920、列表 ≤64、ID ≤128 UTF-8 字节、名称 ≤256 UTF-8 字节，拒绝控制符和不匹配的实际 JPEG 类型/尺寸。已知升级/录屏权限/显示器断开/busy/timeout/Wayland 错误映射为中文可操作提示，未知详情保留。现有 `legacyConnect` 配对渠道新增每次连接 UUID fence，无需账号导出、deviceID 或终端/控制权；账号渠道继续校验 owner 和连接代次。
