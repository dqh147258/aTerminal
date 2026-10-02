# 独立后端 Review 与加密验收结果

2026-10-03，当前子任务范围 **Completed：后端源码安全 Review、18 个独立加密 RPC 必要场景、相关严格检查与证据**。root 已明确将移动端修复/原生 UI/RPC 验收作为 `open transferred_required` 接管；整个跨端任务没有 Completed。

被测实现：runtime `45ab2d4`（包含 `06b4c9d`、`fa29899` 和 `ad5bd37` 等最终修复），在 review 分支合并头 `a7fabe5` 上加入本子任务测试/fixture。生产实现源码没有由本子任务临时改写。最终 fixture 修正只处理真实用户 Skill ID `user/` 前缀、既有观察屏障和实际 Shell 覆盖证明。

## 实际结果

| 检查 | 结果与证据 |
| --- | --- |
| 同版 CLI/example 构建 | 通过，jobs=2，4m22s；[build.log](evidence/build.log) |
| 优先 native 永久重授/撤销与运行中来源分流 | 实际通过；[native-priority.log](evidence/native-priority.log) 中另有一个 fixture 失败，后续已修正 |
| Bash/Zsh 原始 slash-function 覆盖 | 本地隔离 Shell 实际成立，程序文件 bytes 不变；[shell-dispatch.json](evidence/shell-dispatch.json) |
| Bash/Zsh 与 native 永久执行 | 通过，先实际产生 override/function-hit，再 native 仍追加 original 两行；[native-priority-final.log](evidence/native-priority-final.log) |
| 18 项完整加密 RPC 必要验收 | **18 passed / 0 failed / 0 ignored / 0 filtered**，107.33s；[encrypted-final.log](evidence/encrypted-final.log) |
| 完整 Skill 包 helper-only 变更 | 实际 once/full 都拒绝旧快照、泛用脚本不能 always、native 永久能力继续工作；包含在最终 18 项；[skill-rerun.log](evidence/skill-rerun.log) |
| 严格 clippy | 相关独立 test 与 account_demo，`-D warnings` 通过，9.11s；[clippy.log](evidence/clippy.log) |
| fmt/diff/Python runner AST | 通过；没有以静态检查代替上述行为验收 |

全部 Provider/MCP 是本地确定性 SSE/stdio fixture；账号/Desktop/PTY/native 子进程都来自临时目录。没有调用付费供应商，没有访问生产账号、用户活动终端或服务，没有启动移动模拟器验收。

CLI 和 example 的绝对路径供协调者/移动 runner 复用：

- `/Volumes/Code/public-worktree/aTerminal/1002-authorization-review/target/debug/aTerminal`
- `/Volumes/Code/public-worktree/aTerminal/1002-authorization-review/target/debug/examples/account_demo`

Cargo 窗口已释放。上述构建已包含最终 runtime 和修正后的确定性模型；再次同步生产实现后，协调者应在最终 main 复跑相关检查。

## 当前矩阵与证据来源

“独立 RPC”表示本子任务通过真实 Account/RemoteTerminal 或真实 Noise 只读 grant、实际 Host/Broker 与临时进程观察。其他来源明确标注，不把实现者的单测数量重复算成本子任务测试。

| 合同风险 | 当前结论 |
| --- | --- |
| A01 旧 allow_input 与只读 | 独立 RPC 观察旧 true 进入 ask；实际只读 grant 无法 send/resolve/set/revoke/answer；旧 false/model readonly 的其他分支由 runtime/CLI 聚焦证据补充 |
| A02 安全读取和不确定 Shell | 独立 RPC 原生读取实际 stdout/exit0，Shell 函数和半行输入没有副作用；compound inspect 明确错误，无 marker；其他正向语言/攻击 corpus 以策略测试与源码为据 |
| A03 分次输入/Enter | 独立 RPC 输入文本和 Enter 分别审批；拒绝 Enter 未执行 draft；其他控制/多行分支保留 runtime/策略证据 |
| A04/A10 一次、拒绝与多设备重放 | 两台临时真实手机账号竞争、重复 resolve 幂等，实际 marker 仅一行；拒绝没有 marker；拒绝缓存与显式 full 新动作覆盖已实测 |
| A05/A06/A19/A20 精确规则 | native tee 的 stdin/args/cwd 变化需要新审批；always/revoke/always/revoke 实际可撤销；helper-only 改动不执行旧包；未知解释器不能 always、full 仍可执行；JSON map/版本/秘密指纹敏感性由策略与 runtime 聚焦证据补充 |
| A07/A11 full 与委托/预算 | 根 full 下真实委托无需风险卡；全树 human 阻塞超过短活跃预算仍恢复，存在活跃兄弟时预算继续；模型请求计数和工具/轮次额度没有刷新 |
| A08/A09 身份、人工与取消 | 真实只读 grant 和伪造 account/device/client 字段不能扩大权限；人工输入使旧 action 无副作用；PTY pending/native/MCP 取消实际无后续 marker；更多撤权/账号/attachment 分支见 runtime/CLI 证据 |
| A12 重启未知动作 | 最终源码持久 run_id、启动时旧未 final native=unknown 与旧 pending interrupted；runtime 提供恢复聚焦测试；本子任务未独立重新启动完整 daemon RPC 链，不冒充该来源 |
| A13 命令/程序完成 | native 真 stdout、exit0/exit1、managed command_id get/wait 和两个 Agent 并发 PTY 独立；PTY 结果仍依据 hook/冲突 unknown，相关真实 hooks/Actor 测试由 runtime/toolset 提供 |
| A14/A15 新工具及历史 | 独立共享树批量 all/get/wait 结果固定 IDs；scope/分页/retention/delta/精确取消回归由 toolset/runtime 聚焦测试与源码补充；没有任意 TUI 完成适配器声明 |
| A16 移动 UI/CLI | 后端同 envelope/真实 grant 已验；CLI 已有 runtime 集成证据；Android/iOS 原生 UI/RPC 为 `open transferred_required` |
| A17 管理入口与无 OS 沙箱 | 已识别权限管理入口 Forbidden，full 不提升 read_only/身份；源码与策略/CLI 回归支持。不声称任意已授权同 UID 程序受到 OS 隔离 |
| A18/A21 full/question 与完整详情 | full 释放有效审批却不自动回答 question；long details 未 ack/错 fingerprint 都不能执行；完整分页后一次实际写入；真正 question 答案进入工具结果 |
| A22 Agent 写入竞态/来源 | 独立 Native 与另一 Agent PTY 并发互不污染结果；旧 PTY 输入 fence 的 Actor 最后消费竞态已有 runtime 真模型/代理/PTY 回归，独立源码已核对 |

18 个必要场景逐名结果见最终日志。真实 marker、模型请求记录和精确工具结果保存在 [encrypted-final/](evidence/encrypted-final/)；汇总见 [encrypted-final-summary.json](evidence/encrypted-final-summary.json)。仅导出合成数据、账号/进程无权限凭据的 UUID 与临时路径，未导出账号密码、endpoint token 或数据库。

## 保留的失败与修正

首轮优先测试 2/3 通过，Shell case 尚未按旧 PTY accepted 状态进行新 Run 的观察；后续只修 fixture，增加实际 function-hit/override 证明，并让确定性模型仅对 `observe_terminal_after_uncertain_action` 先观察、用新 action ID 重提。它不会重试 authorization_denied，也不会代替用户批准。原始失败日志 [native-priority.log](evidence/native-priority.log) 和 [native-priority-rerun.log](evidence/native-priority-rerun.log) 保留。

完整首轮 17/18 通过，唯一 Skill fixture 使用错误 ID 前缀，实际 `skill_not_found` 后读到了下一 native 审批。修为 `user/review-skill` 后聚焦通过，再完整 18/18 通过。原始 [encrypted-all.log](evidence/encrypted-all.log) 保留，不能把该失败描述成脚本曾获得永久权限。

## 必要移动事项转交

以下不是可选建议，也不是后端通过即可忽略的项目：

1. **Android R20：revoke nonce 长期复用。** `AgentAuthorizationPanel.idempotent` 持久缓存 hash(action,rule_id)，成功撤销未清 key；同规则撤销、再授、再撤销可能重用旧 request UUID，服务端正确返回旧 duplicate，新的规则仍活跃。root/Android 必须修复并以真实 UI/RPC 再授再撤销验收。
2. **Android R21：PTY 可写性误作 Agent 管理权限。** writeReason 的 detached/closed 检查阻断 settings、answer、readonly send；应区分真实设备 can_mutate 与操作目标的 PTY fence/attachment。root/Android 必须修复并验收。
3. **iOS 原生 UI/RPC。** `3f7a2b` 轻量 diff 可见 revoke 成功清 idempotency key，管理权限使用 connection/grant 而非 terminal lease，完整详情 fingerprint/generation 校验已接线；实际 iOS UI/RPC 尚未由本子任务执行。root/iOS 必须完成其系统验收和最终 diff Review。

root 在最多两个活跃执行者约束下明确接管这些 `open transferred_required` 项，并允许本后端 Review 阶段 completed；整个产品任务仍需这些项及最终 main 验收通过后才可 Completed。物理设备和线上模型没有验证。
