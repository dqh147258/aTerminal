# Runtime implementation

状态：Completed (owned runtime and native result isolation follow-up)。复用上级 PLAN.md、CONTRACT.md 的用户授权与 High 验证范围。

1. 固定用户 RPC 与 SQLite permissions / pending / exact rules；旧 allow_input=true 映射 ask、false 保持只读。
2. 接入 Host 审批、问答、动态权限与共享活跃预算暂停；保持 action ledger 和 analysis barrier。
3. 接入 Desktop 身份/cwd/fence、远程只读门控、CLI；合并协调者指定 policy/toolset 接口提交。
4. 隔离 Store/Host/本地模型桩与真实 Broker/CLI 测试已过；提交最终本任务源码并报告主线/独立Review复跑项。

接口响应：permissions 直接对象；pending/rules 是 {items,cursor,has_more}；state 增加 permissions、pending；resolve={pending,duplicate}。等待对外状态 waiting_for_user，仍属于活跃 Run，不能启动替代 Run。

Root已批准独立run_program原生入口和v3规则：PTY永不永久，可靠leaf实际exec/hash/args/stdin/cwd才提供always；不增加OS/cwd沙箱。详见HANDOFF.md、CHECKPOINT.json。

必要本地High验证通过，证据见VALIDATION.md；root仍负责最终主线、独立Review和两端验收。

后续Review发现native运行记录误用PTY失效/关联：修复source分流、get/wait读取管理过程状态与重启未知，新增并发回归。

来源分流后续已完成：Store工具6/6、实际独立Global observer并发native/PT​​Y/get/wait/档案1/1及strictclippy过；无需重复不相关验收。

R2完整注册Skill包actualhash验证及泛用脚本once/full边界已实现并通过针对回归；无需OS依赖沙箱。
