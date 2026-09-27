# 全局AI助手独立会话改造

- Status: Completed
- Updated: 2026-09-27

## 目标与范围
按 `/Volumes/Code/OpenDesignProjects/4c6b6741-85d2-4ded-b4c0-37377166a5c9/2026-09-27-全局AI助手改造需求说明.md` 和同目录五张截图修改 Android App，分离终端 Session 对话与全局AI助手，提供独立列表、完整聊天页面、真实运行状态和未读状态。保持账号/Desktop 权限边界、模型、工具、图片和语音。生产不插入演示消息。

## 当前状态与证据
AgentPanel.kt 使用 Session/Global 切换，空字符串代表唯一全局会话。MainActivity.kt 已有全屏面板和右侧图标栏。Store.agent 的绑定是 owner/desktop/session，但 Scope.agent 已有独立身份，RPC 支持 agent_id 定位。现有 SQLite 历史、任务恢复和工具授权可直接复用。工作区存在上一轮 UI/图片改造，保留这些修改。

## 方案与执行
用户已明确授权“根据文档调整 App”，按该范围实施。
1. 后端添加幂等创建全局会话及分页摘要查询，所有新会话 session=None；旧默认全局 Scope 原样映射到历史列表，无需复制消息。
2. Android 增加图标入口、独立列表与全屏聊天返回路径。移除作用域切换；草稿、缓存、异步请求按 agent_id 隔离。迁移旧空字符串草稿键且只执行一次。
3. 列表查询真实任务状态，以持久回复序号和实际阅读位置维护本机未读；重连恢复服务端状态。保留聊天/列表阅读位置。
4. 添加关键隔离/兼容/导航测试，构建 Rust 和 Android，使用已有模拟器验证目标页面。

## 验证
Rust 测试覆盖幂等创建、作用域隔离、旧历史保留、列表摘要与权限；Android instrumentation 覆盖导航、新空白会话、草稿隔离、真实阅读标记及状态并存。构建 debug APK、lint，并检查截图。模拟器和 fixture 不代表真实供应商模型端到端验证。

## 风险与回退
新增 RPC 需要 Android 与 Desktop 同时升级；旧后端出错时显示真实错误，不模拟会话。旧 Scope 和历史格式不变，新增全局会话复用原运行时。按文件变更块回退，保留数据库。

## 执行结果
Android 独立全局会话、真实状态/未读、旧历史和草稿兼容已实现。51 项 Rust 测试、7 项 UI 回归、真实加密 RPC 端到端验证及 Android 构建/lint 通过。低高度布局问题已修复并通过严格回归。详见 RESULTS.md；更新配套 Desktop 与 APK，实体设备建议见 HANDOFF.md。

## 未决问题、歧义与确认
None.
