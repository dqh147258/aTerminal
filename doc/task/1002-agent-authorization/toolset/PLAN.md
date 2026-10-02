# 工具 helper 实现

状态：Completed（本执行者 owned 范围）。复用上层 PLAN.md、CONTRACT.md 和用户整体实现授权。

新增 host/store/Broker child modules，父模块由 runtime 执行者接线。复用精确 Run 查询、watch 通知、PTY 写入 gate、读取缓存及 SQLite 记录；不添加 OS/cwd 沙箱。

必要验证（High）：schema 角色约束、固定 any/all 集合、精确任务取消、history cursor 绑定/保留、shell sequence/完整文本关联及错配 unknown、revision 等待与 TUI 重取。用临时数据库、隔离 shell/PTY、本地桩；不访问现有 Desktop 或付费模型。物理设备不在本子任务范围。

进度：owned helpers、Shell/原生读取、角色schema、部署文档与必要回归已交付。接入runtime39fcb54实际父模块后22项聚焦检查全部通过，两lib strict clippy（-D warnings）、fmt/all、owned rustfmt/check与diff/check通过。恢复接口/证据/限制见 [HANDOFF.md](HANDOFF.md)；结构化结果见 [CHECKPOINT.json](CHECKPOINT.json)。

新增inspect_command采用policy固定native程序/argv，原PTY源码继续审批；不改变输入或cwd。检查强度保持High，cargo jobs最多2、test threads最多2，与runtime错峰，不调用付费模型。未知应用任务没有完成适配器。

本子任务只提交owned source/docs，完成本范围即报告completed，由root关闭终端后调度独立Review和最终跨端/main集成验收。
