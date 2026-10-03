# 移动端最终审查

2026-10-03，root 接管后端 Review 转交的必要移动事项。源码审查已完成下列定位；最终 UI/RPC 结果仍待执行者报告，整体状态以父 PLAN 为准。

Android R20（`c7cd11a`、`86a906d`）：撤销成功验证响应规则 ID 与 revoked 字段后结束本次幂等操作；网络结果未确认时保留原请求 UUID。由于真实 AgentPanel 路由会给同一 JSONObject 添加 version/agent_id，nonce key 必须在发送前取得，不能在成功回调重新计算。最终代码捕获原 key，第二次授予同 rule ID 后使用新 UUID 撤销。实际 UI/RPC 必须观察两次规则消失与第三次审批，不能只检查按钮点击。

Android R21（`c7cd11a`、`b7bf40a`）：MainActivity/AgentPanel 的 Agent 管理只绑定连接、账号、Desktop、对话代次与服务端 can_mutate；closed/exited/detached 不再作为管理权限。settings、answer、stop、只读任务可操作，真实 PTY 写入继续由 Host 最终检查。实际关闭临时 Session 的 UI/RPC 检查仍需通过；已有本地 detached UI 不能当作真实 detached 传输证据。

iOS 生命周期（`d4f9021`）：question/details 的 sheet 从 LazyVStack 卡片迁至稳定 ChatPanel；AssistantModel 保存 scope/pending 草稿与 context 代次。打开时校验目标快照，任务开始前复核旧 presentation，账号/范围/连接变化清展示。失败、关闭与 cancelled 保留未消费草稿；完整成功 pending 分页中消失才作为 consumed 清除（Store.pending 保留所有非 consumed 状态）。主线 Foundation 行为检查通过，真实键盘与完整 RPC 尚待执行。

测试模型压缩（`dd8fa86`）：确定性授权 fixture 在选择旧 scenario 前识别 Application compression stage，返回 JSON summary/stop；不修改 Host、权限策略或生产模型。实际 HTTP 的无 scenario/有旧 scenario 两例通过，均无 tool_calls，旧步骤未推进，见 [fixture-compression.json](evidence/fixture-compression.json)。两端完整 UI/RPC 将使用该 example。

压缩后的当前步骤：第二轮 Android 在 cwd-back 处进入 completed 而没有审批，定位为 fixture 未读取 Host 已保留的 Current task constraints；没有工具执行或权限绕过。后续 fixture 仅解析 Host 标准压缩消息中的 retained_facts JSON 数组，选择最新真实 AUTH_REVIEW 或 authenticated delegated，不从 lossy summary/Archive 取旧任务、不使用外部计数推进。实际 HTTP 五例覆盖无/有旧任务压缩、当前任务、委托任务、空约束，确认选择当前且不重放摘要旧任务，见 [fixture-retained-constraints.json](evidence/fixture-retained-constraints.json)。

审查未发现本轮修复扩大调用权限或改变 Terminal 的 cwd/PTY 语义。Android 新 IME 回归（`f49d052`）暂时开启 software keyboard 并在 finally 恢复原设置；专用授权包继续隔离正常账号。最终 main 必要检查、实际 marker 与原失败证据仍需在交付前归档。

关闭 Session 的测试服务生命周期：第三/四轮 offline/channel closed 的明确根因为 account_demo 每2秒 Poll 唯一临时 Session，关闭后错误使测试服务退出；不归因旧 JNI 缓存或未证实的生产连接问题。仅授权 fixture 改为 Poll 失败时通过健康 List 确认该确切 Session 已删除后继续存活，其他错误保留。真实 CLI 关闭临时 PTY 后跨两个 Poll 周期，List、closed scope permissions 和模型 HTTP 均继续可用，见 [fixture-close.json](evidence/fixture-close.json)。Android 测试的正常重连、真实键盘避让和闭环断言仍须完成。
