# 当前阶段

状态 In progress；用户授权及 High 必要检查不变。runtime 是父 Rust、RPC/CLI/stream 唯一实现者。

已接线：SQLite conversation CAS、24h pending、once 消费、规则撤销/重新授权、重启中断；Host 审批与 ask_user、共享活跃计时、真实副作用 permit；Actor host_input_boundary 与真实 OS cwd；用户 RPC/CLI 完整详情；remote can_mutate。

已合 policy 5eb063c/900d1f2、toolset c29f404/fe8d656。三个 crate cargo check 通过；Store 授权 8、Host 人类等待 8 测试通过（本地模型桩，不使用供应商）。

待完成：policy inspect_command_plan/permanent_command_program 提交接线、toolset inspect helper；任意 run_command 暂 source Unknown（审批有效，禁用未证明的永久规则）；扩大真实 Broker/CLI 回归并完成 clippy。当前不是完整交付。

Run 已恢复为 fb63f371-e6fa-4322-a8fe-57d96fa3ec18；不要使用旧 run。pending.state 保持 pending，Run.state=waiting_for_user。
