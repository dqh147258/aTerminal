# Runtime implementation

状态：Approved / In progress。复用上级 PLAN.md、CONTRACT.md 的用户授权与 High 验证范围。

1. 固定用户 RPC 与 SQLite permissions / pending / exact rules；旧 allow_input=true 映射 ask、false 保持只读。
2. 接入 Host 审批、问答、动态权限与共享活跃预算暂停；保持 action ledger 和 analysis barrier。
3. 接入 Desktop 身份/cwd/fence、远程只读门控、CLI；合并协调者指定 policy/toolset 接口提交。
4. 隔离 Store/Host/本地模型桩与 Broker/CLI 测试；提交本任务源码并报告集成复跑项。

接口响应：permissions 直接对象；pending/rules 是 {items,cursor,has_more}；state 增加 permissions、pending；resolve={pending,duplicate}。等待对外状态 waiting_for_user，仍属于活跃 Run，不能启动替代 Run。
