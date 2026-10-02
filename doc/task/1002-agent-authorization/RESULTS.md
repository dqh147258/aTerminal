# 验证进展

状态：In progress。以下是阶段证据，尚未达到任务 Completed 条件。

2026-10-02：系统故障恢复后六个原子任务已使用 `gpt-6.1-sol / high`、保存 thread ID 和原工作树继续。没有重置未提交源码或替换用户账号/活动终端。

## 主线阶段检查

被测 main：`ee4bf708d2f330517ea03380dea19ba98b41278a`，整合 runtime `8eaf8bb`、policy `5b89a2d`、toolset `5ddf090`、独立 review tests `a3cb258`。

执行 `cargo +stable test --locked -p ai-terminal-agent-runtime -p ai-terminal-agent --lib`（隔离临时状态、本地模型/MCP、PTY；因进程身份/目录和loopback能力使用环境升级）：

- Desktop：39 / 39 通过，含 bash/zsh 真 PTY hooks、原有钩子保留、正确退出/冲突 unknown、原生观察目录/限额/取消、账号和权限回归。
- Runtime：95 项中 94 通过；唯一失败 `host::runtime_contracts::rejected_analysis_is_bounded_and_recovery_never_replays_writes` 报 `agent did not settle`。原因是旧写入测试没有显式授权，新策略进入人类等待；runtime执行者已提供显式用户授权的聚焦通过证据，修复尚待合入主线重新验证。
- 新策略17组、Store授权8组、Host审批等待8组以及工具/历史/分析回归包含在上述检查中；不能将数量重复加总。

尚未执行最终主线 clippy/fmt、完整CLI/加密授权行为测试、Android/iOS真实RPC UI、最终独立Review；runtime永久identity/输入版本/动态Skillcwd与移动端接线仍在推进。

## 子任务阶段证据

- Policy `900d1f2` / `5b89a2d`：17组策略测试、严格clippy、fmt/diff通过；只新增/修改授权策略模块，曾临时测试导出已原样恢复。独立Review与集成仍需处理最终反馈。
- Toolset：bash/zsh 真PTY completed/exit1/compound/cd和钩子保留已通过；新原生读取和完整记录分块搜索的主线检查通过。输出区分原生侧读取与终端应用，不宣称通用TUI完成适配器。
- Toolset最终 `0ee30d1`：基于实际runtime39fcb54父接线的隔离Git副本，低并行jobs2/threads2共22/22通过；两lib strict clippy、fmt/owned rustfmt/diff通过。包括观察许可一次消费/拒绝复用/cwd拒绝。此子任务已完成关闭，最终主线与跨端整体验收仍未完成。
- Runtime：Store/Host聚焦16项通过；共享整树人类等待能超短活跃预算继续且不增加模型循环。后续扩大回归中暴露的测试授权/沙箱OScwd问题已定位，最终主线结果尚待合入重跑。
- Runtime阶段39fcb54：Desktop48/48、Runtime95/95在native末端小hook28d5700合入前通过；hook合后cargo check三crates通过，后续22工具测试已覆盖hook。真实Actor在Broker全部preflight后插入另一Agent draft的R13竞态已过（input_revision保持1，审批字符未写入），最终CLI/全部集成检查继续。
- Android/iOS：本地原生UI与控制器验证持续进行，当前不把已构建或部分通过当作全端到端通过。真实用户账号、既有终端与生产模型未用于测试。

物理设备、真实供应商、任意同UID已授权程序的OS隔离不在已验证范围；此任务按用户要求不增加严格目录/OS沙箱。
