# 修复结果

2026-10-04，三个 Review 问题已修复，用户要求的提交包含实现、回归测试和本记录。

授权提交统一使用ToolContext.commit_effect，把最后实时校验与同步副作用放在同一execution gate内。创建Session直接使用此边界；委托注册/追加与模型停止在jobs锁内提交，保持jobs→gate顺序。已有动作先提交则权限RPC必须等待，权限撤销先ACK则后续副作用拒绝；新可控并发用例验证了两个顺序。

原生stdout/stderr保留有界前缀与截断标记，并持续排空至EOF。相同逻辑用于用户Skill脚本，原脚本输出超限策略保留。实际同版本CLI/确定性模型复现：Python输出stdout/stderr各512KiB之后执行文件写入，退出码0、两项truncated=true、marker确实生成；修复前退出码1且marker没有生成。真实子进程回归同时覆盖两个管道、保存长度与后续副作用，取消/超时相关原回归继续通过。

失效请求记录terminal_at，列表/新请求维护自身scope与来源authority下最近64条且最多24小时的失效提示；配套详情一并清理。pending/resolved和永久规则不误清，human_responses保留幂等回执。新增authority/expiry索引避免反复扫描所有记录。1001条失效、跨Session来源视图、活跃及已回答待办、TTL/跨账号、清理详情后旧nonce重试均有真实Store回归覆盖。iOS注释更新说明已退休失效草稿的生命周期，协议和UI代码没有新增分支。

必要检查通过：Desktop60/60、Runtime103/103；真实CLI3/3与加密RPC18/18（0ignored、106.84s）；两lib及CLI/bin/example/相关集成tests严格clippy -D warnings；fmt/diff；iOS生产授权/草稿控制器主机检查。最后索引改动另复核Store授权测试。全部临时账号/本地模型、私有目录，不调用付费供应商或物理设备；不重复无改动的模拟器构建。

源码自审覆盖新锁顺序、提交时机、程序管道生命周期及清理/幂等边界；没有移除账号、只读、取消、Session或版本校验。失效卡退休后不再在pending查询展示；用户RPC的原幂等响应仍可重试。原有授权功能完整验收历史见doc/task/1002-agent-authorization。
