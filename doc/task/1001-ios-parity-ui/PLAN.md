# iOS 功能与 UI 同步实现

- Status: Completed
- Updated: 2026-10-01

## 目标与范围
同步共用 20 页参考的工作空间、设置、LLM、读取、MCP/Skills 和独立 Global 会话。仅修改 iOS Swift、必要项目引用及本任务文档，不操作模拟器、共享服务，不提交合并。

## 当前状态与证据
WorkspaceScreen 仅有显示设置；ChatPanel 旧 Form 混合所有配置；AssistantModel 仍包含旧归档导入。Desktop configuration 与 global_list/global_create/global:ID 协议已由 Android 实现，可直接复用。参考 UI_REFERENCE.md、IOS_HANDOFF.md、ANDROID_REFERENCE.md、AgentSettingsPanel.kt、GlobalConversationPanel.kt。

## 方案与执行
沿用中央 /Volumes/Code/My/aTerminal/doc/task/1001-ios-parity/PLAN.md 记录的用户明确执行授权。
1. 统一蓝色视觉和全屏设置路由，保留原登录/终端数据。
2. 新建独立设置组件，原对象合并编辑；页面/身份屏障、busy、冲突刷新后显式重试；目录能力完整替换与绑定继承。
3. 完整扩展管理/资源上传；独立 Global 多会话及持久化草稿隔离；移除旧归档入口和导入，不删数据/API。
4. 构建和关键确定性验证，交付矩阵给独立验收子任务。

## 验证
完整 iOS Simulator SDK 构建（不启动模拟器）、Swift 静态检查；验证 JSON 保留、目录能力、绑定、输入校验。运行时 UI/真实 RPC 由协调者验收。

## 风险与回退
异步结果仅在原页面/原身份接收；密钥仅请求生命周期保留；Skill 编辑只更新 SKILL.md；可整体回退 Swift 变更，不迁移历史数据。

已完成源码和Review R1–R8对应修复；独立生产逻辑检查9/9通过，Debug完整x86_64 App构建及Release全量typecheck通过，git diff --check通过。协调者已授权以 `[未Review]` 标记提交，提交后冻结源码且不 merge；UI/真实RPC由协调者接手，详见HANDOFF.md和TEST_API.md。

## 未决问题、歧义与确认
None.
