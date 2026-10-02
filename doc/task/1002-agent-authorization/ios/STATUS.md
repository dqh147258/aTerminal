# iOS 实现与验证

状态：Blocked（执行授权仍有效，协调者要求停止进一步产品修改/重跑，等待新 UI 生命周期失败的处理决定）。复用父任务 PLAN/CONTRACT 和用户全部实现授权；测试 High，物理设备不在验收范围。

实现入口：`AssistantModel.swift` 使用 Foundation-only `AgentAuthorization.swift` 读取 Desktop 权限、分页 pending/rules 和完整审批详情；CAS 修改、幂等答复和撤销均重新读取服务端结果。`ChatPanel.swift` / `AgentAuthorizationView.swift` 展示一次/永久/拒绝、问答、规则与当前对话完全授权。旧草稿 allowInput 不升级或发送为 full，新 send 显式 permission_mode。

最终协议：永久授权案例为独立原生 `run_program` 的 `/usr/bin/tee`、精确 args/stdin；交互 PTY 卡展示 can_always=false 原因。管理和纯问答依据真实设备 can_mutate，不把终端控制/attachment当用户设备权限。长详情完整取齐且 fingerprint 一致后才允许 once/always，并发失败仍可 deny。规则撤销成功后释放旧幂等 ID，重新授权的同 ID 规则能再撤销。

当前证据：`python3 scripts/check-ios-authorization.py` 通过（恢复、full 跨刷新、CAS、once/deny/answer/revoke、重新授权再撤销、网络重试 ID、设备 readonly/缺 can_mutate、账号/连接 epoch、legacy fail-closed、详情完整分页和错误 fingerprint/ACK）。Python runner 编译与 diff 检查通过。

早期 iOS simulator build-for-testing 通过；专用 SE 模拟器 `079C5369-F052-45A8-A767-70B1A9FA6707` 的原生 once/自由答复/键盘发送、full 开关/永久规则撤销及详情失败 deny 单项曾通过。完整 7 项套件未通过，大字体滚动仍需复测；随后出现 CoreAnimation render IPC 阻塞，采样保存 `build/ui-hang-sample.txt`。这些是部分证据，最新 Swift/UI/RPC 测试源仍需最终构建。

真实加密 RPC：`AuthorizationRpcUITests` 与 `scripts/test-ios-authorization.py` 已准备。使用同 main 构建的 account_demo --authorization-test、独立私有账号/Desktop/状态目录，不与 Android 共用；实际点击前观察 marker 未出现，结束后 runner 独立精确核验 PTY/native marker 行数。尚未运行，不将本地 UI/主机桩计作真实 RPC。

当前等待 root 释放 Xcode 窗口；Review 独占大型 Cargo。之后 Xcode jobs<=2、parallel-testing-disabled，并只操作本任务专用 Simulator。复用未跟踪 build 中 FFI/xcframework/fixture 资产，未修改 Rust、其他端或共享计划。

最新有效结果与恢复入口见 [HANDOFF.md](HANDOFF.md)。真实完整 UI/RPC 尚未通过，不以早期9项或部分marker代替。
