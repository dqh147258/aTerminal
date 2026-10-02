# Android 授权与问答

状态：Approved / In progress。沿用协调者 PLAN.md / CONTRACT.md 与用户全面执行授权；High 覆盖。当前 run：8dc5141e-1c4c-4837-82d3-5cb0d4522827。

Android 源码已实现：

- `AgentPanel` 新任务显式发送服务端确认的 `permission_mode`，默认 ask；不读旧 allow_input/full 草稿升级授权。`waiting_for_user` 可停止/追加。
- `AgentAuthorizationPanel` 读取 Desktop 模式/full/revision/can_mutate，CAS 修改后只展示服务器确认值；缺失 grant/不支持新版时保守禁用。开关、待办与规则均绑定原账号/Desktop/对话。
- 聊天持久卡支持 once/always/deny、选项及自由回答，显示目标 Session/Run/cwd/完整有界参数/永久规则范围。不可永久批准时 disabled 并解释。失败保留卡/回答，resolve/revoke 请求 ID 持久幂等。
- 长审批使用 `approval_details(pending_id,cursor)`，分段核对 pending_id/fingerprint/text/has_more/truncated，最大 1 MiB。全文取齐并显示后才能 once/always，提交 `details_ack:true` 和匹配 fingerprint；失败、错指纹或不完整分页时仍可 deny。详情不从草稿恢复。
- 历史刷新保持回答字段连接到 View，保护输入焦点/文字选择；权限弹窗关闭回调按实例核对，避免旧弹窗清空新引用。
- 全授权与规则只存 Desktop；本地仅缓存待办、回答草稿和幂等请求 ID。Global 子 Run 审批经来源 Global scope 响应。

验证安排/当前证据：

- Android 构建/AndroidTest 构建/lint 已通过，使用原 SDK、现有 JNI/bindings。生成物属于本地依赖，不进入源码提交。
- 14 个本地原生 UI 用例覆盖三种决定、永久不可用、撤销、问答丢响应/重开重试、CAS/full 等待确认、scope 失效、分页损坏、只读 grant/缺 grant/旧 Desktop、失效 pending、历史焦点、小屏大字体/IME、长详情失败及错指纹。
- 原有 AgentFocus/GlobalAssistant/MobilePrototype/ToolDetails/WorkspaceReviewRegression 共 13 用例同步新版 fixture，继续检验草稿/图片/历史/设置。此前发现旧设置按钮/返回文案与实际原生行已不符，已更新断言；限定高度需等待真实布局完成，不能以旧 viewport 尺寸判定。
- 当前完整 27 用例重跑中；之前长详情显示缓存碰撞已修正，不将失败轮次记作通过。
- 真正加密 RPC 测试：`AgentAuthorizationRpcUiTest`，读取 `files/agent-ui-fixture.json`，输出 `files/authorization-ui-results.json` 的 passed/expected_markers。覆盖实际 UI once/always/deny/问答/full/长详情/cwd/撤销/取消/只读模式。永久案例用 `/bin/echo always >> auth-review-always.log`，不用解释器。runner 由 review 执行者拥有；待 root 合入 runtime 后执行并独立核验临时 PTY marker，不以本地桩替代端到端结果。
- 模拟器是原 emulator-5586；重启后经 android-emulator-control 恢复，status 核对 boot_completed。测试包 `com.yxf.aterminal.authorizationfixture`，构建 `-PauthorizationUiFixture=true`，正常账号/应用/终端数据隔离。不调用付费模型、不测试物理设备。

日志保存在 `apps/android/authorization-build-final.log` / `authorization-regression-final.log`（忽略生成物）。最终可复核摘要、提交号和真实 RPC 限制由本说明更新及 worktree checkpoint 报告。
