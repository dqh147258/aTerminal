# iOS 子任务

用户已明确授权：按设计实现移动 App；先写计划，不再 Review，写完执行；分配三个 worktree 子任务。读取父计划 `/Volumes/Code/My/AITerminal/doc/task/0922-mobile-admin/PLAN.md` 和共享 AI 合同 `CONTRACT.md`。遵循 worktree-tasks 执行器报告，不重复申请批准，不再分派子任务。

你负责 `apps/ios/**`、必要的 `scripts/generate-ios-project.rb` 和自己 `doc/task/...`。按 `/Volumes/Code/OpenDesignProjects/4c6b6741-85d2-4ded-b4c0-37377166a5c9` 的 HTML/CSS/截图，使用现有 SwiftUI/UIKit 实现完整移动工作台：登录与密码可见/校验/加载/错误，终端主体、工作空间抽屉、搜索/切换会话、设备选择和真实连接、创建/关闭会话、可隐藏输入及功能键、接管输入、终端历史、账号管理、显示设置（字体 12-24，浮层不透明度 60-96%，默认 16/88，持久保存及恢复默认）、AI 聊天/请求状态/历史/搜索/继续对话、原生语音识别（仅填草稿，有取消/权限/不可用状态）。保留加密通道和 Rust native renderer，不做 WebView 演示。

颜色/排版/层级贴设计。不要带 demo 账号/虚构在线/固定终端输出。保留现有真实功能，做好小屏、键盘、安全区、旋转、长文本和无障碍。AI 接口由协调者新增，不自行修改 Rust；按合同开发，生成绑定与库稍后同步。可先用当前绑定验证其他 UI，再切真实 assistant API。聊天按服务+账号+设备+会话隔离私有存储，退出不串号。

可复用主仓库忽略产物 `build/AITerminalCore.xcframework`、`build/bindings`、`build/ai_terminal_mobile.swift`、`build/fixtures`；不要复制 .local 或 secrets。Xcode simulator build，用独立 derivedDataPath；可运行模拟器并做登录、抽屉、设置、聊天/长文本的截图检查（Debug fixture 必须明确隔离，不进入产品数据）。不要擅自操作真机。写验证结果和限制，不修改根 README/Cargo.lock/Android。不 commit、合并、删除或关闭工作树/终端。完成 CLI 上报 completed，协调者审查集成。
