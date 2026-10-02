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

## 2026-10-03 后端最终 Review 接管

当前 run：c0ea835a-41c8-447a-a34c-fd46b0befc01。后端最终 Runtime/Review 主线已完成，由 Android 接管 R20/R21。原先 `/bin/echo` PTY 永久场景已过时：最终 PTY only once/full；永久真实场景改为 `run_program {program:"/usr/bin/tee",args:["-a","auth-review-always.log"],stdin:"always\n"}`，精确三行对应首次、规则自动放行、撤销后再授；第二次撤销后同参数 deny，不产生第四行。

源码修复与必要新增验收（尚未运行）：

- R20：响应 ID/撤销结果确认后清本次 operation nonce；丢 ACK 继续同 nonce。local UI 先丢 ACK 重试，再同 ID 再授/撤销新 nonce；真实 UI/RPC 两次撤销均 rules 为空、新 nonce，后续同 Native 参数再次审批且无多余 marker。
- R21：移除 `MainActivity.writeReason` 对 closed/exited/detached 的管理/任务门控。连接/账号/Desktop/scope 与真实 `can_mutate` 继续限制；PT​​Y writes 最终由 Host 检查。local Session UI 终端 unavailable 下 read_only 任务、回答、设置、停止仍可达；真实 fixture关闭后 query/ask_user/设置，full 下 PTY write 仍由服务器拒绝且无 marker。
- 本地永久审批桩改 Native 普通 program/args/stdin 形态。长详情、CAS、失效scope、旧Desktop、只读设备和输入恢复仍保留。
- runner 复用主线 Review 已确认 envelope，增加显式 `--cli` / `--example`，保存运行二进制 SHA256、核验独立 APK package。无需重新 Cargo；复用 Review 最终二进制。600秒验收窗口只启动隔离fixture，不使用用户服务。
- MobilePrototype 可选截图辅助函数重试并记录缺图，行为断言继续必需。此前14授权/其他原回归25项过，2项因可选截图null而中断，最新最终行为结果仍待。

当前大型构建窗口归 iOS，Android 静态准备/提交，不运行 Gradle/模拟器。协调者释放后按 jobs=2 / parallel=false 构建，并在独立包/原模拟器执行必要本地及真实 RPC 验收，不能把旧桩结果宣称 Native 永久已实测。
