# 工具 helper 实现

状态：Approved / In progress。复用上层 PLAN.md、CONTRACT.md 和用户整体实现授权。

新增 host/store/Broker child modules，父模块由 runtime 执行者接线。复用精确 Run 查询、watch 通知、PTY 写入 gate、读取缓存及 SQLite 记录；不添加 OS/cwd 沙箱。

必要验证（High）：schema 角色约束、固定 any/all 集合、精确任务取消、history cursor 绑定/保留、shell sequence/完整文本关联及错配 unknown、revision 等待与 TUI 重取。用临时数据库、隔离 shell/PTY、本地桩；不访问现有 Desktop 或付费模型。物理设备不在本子任务范围。

进度：正在实现 API；ask_user 持久化与权限状态由 runtime 提供。未知应用任务没有完成适配器。
