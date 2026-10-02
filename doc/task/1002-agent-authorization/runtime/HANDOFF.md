# 当前有效交付摘要

状态：Completed owned implementation / necessary local verification；最终源码见提交报告。current run=d51ef561-a773-416c-b35c-eb5e1cd13301。前置源码 8eaf8bb、39fcb54、06b4c9d，依赖 policy5b89a2d/toolset28d5700、toolset文档0ee30d1；原计划执行授权有效，High、低并行 jobs2/testthreads2。

审批/问答、CAS对话权限、规则、TTL、重启中断、exact once、deny/full、委托权限和共享预算已接入真实 Host/RPC/CLI/remote。Run.state=waiting_for_user，pending.state=pending；一次消费和实际副作用check/commit均保留身份、取消、账户、模型只读和人工fence。R13真实Actor末端阻止其他Agent新draft附加；只有initial zero-input或exact submission sequence/command证明空输入，raw/typeahead保持unknown。

`run_program` 是独立原生入口：absolute program、literal args<=64/total16000 bytes、stdin UTF8<=16000（null/缺省EOF）、env_clear、OS cwd、own process group、30s/bounded output、real exit、source=native_program、command_id/result_record_id持久档案。只有可靠native leaf+real file hash+精确args/stdin/cwd可always，未知/解释器/wrapper仍once/full。PTY工具继续原语义且没有永久规则。v3 namespace使旧PTY规则不能匹配。没有OS/cwd沙箱。

Runtime lib99/99、Desktop lib57/57、CLI2参数解析和3真实Daemon+本地SSE+PTY/native用例通过；最新人类等待cancel复核Host11/11聚焦过。真实tee marker Always->auto->revoke->deny->regrant->revoke追加量精确、规则ID一致；native未知程序可执行、真实exit7/EOF/输出限额/cancel杀组且原PTY未停。Global full子任务继承不改Session开关；deny后真实setfull只放行新调用。strictclippy两lib及tests、CLIbin/native真实test过；fmt/diff过，最终检查通过，详见VALIDATION.md与CHECKPOINT。

Native local未登录账户内部身份=local/installation，Store/RPC owner隔离不变；HTTP MCP cwd未知，不用本地目录冒充远端；stdio固定spawncwd保持；Skill默认session cwd真正随同Run cd变化。扩展移除/disable/version/凭据变更停止旧调用；MCP启动线性点短持gate，async I/O不持锁。

剩余属于root：独立encrypted Review、实际Bash/Zsh覆盖marker、主线整合复跑、Android/iOS真实UI/协议验收。没有用户付费模型、物理设备、正常账号/终端被用于测试；PowerShell不可用未运行。原生stdout/进程完成不等于通用TUI/application完成。


最新native来源分流followup（在fa29899后）：PTY invalidate只影响PTY行；native get/wait直接读管理过程Store state，不经Shell correlate。begin_command持久run_id，update保留旧run关联，Store.open已有native非final=>unknown/final/exitnull/reason=desktop_restart；不重放。get_result有界回读command指定result_record_id的immutable stdout/stderr/exit；大档案带cursor，可read_record继续，不取Session最新Run。Store工具6/6与真实独立Global observer读运行native、另一Agent写PTY、wait完成/exit0/marker/档案1/1以及相关strictclippy过。
