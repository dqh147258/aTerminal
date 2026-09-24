# AI Terminal

AI Terminal 由 Rust Desktop Agent、原生 Android/iOS 客户端和轻量协调服务器组成。Desktop 保持唯一的终端状态，手机维护显示副本；连接优先使用 WebRTC，无法直连时通过加密 WSS 中转。

**当前是可运行的开发版本，尚未完成发布验收。** Desktop Shell、账号与设备管理、端到端加密远程终端、移动工作台和 Web Admin 已实现。Android 最低支持 API 25，提供 arm64-v8a、x86_64、x86 构建。跨公网 NAT、长期性能、发布签名与商店发布仍需验收。

Android/iOS 工作台提供登录、设备和会话选择、终端输入与历史、显示设置及最后会话恢复。移动端 AI 浮窗当前仅为不可交互的占位界面，不发送模型请求、不监控终端、不录音；已有历史仍可查看。Desktop 模型协议代码保留，后续启用边界见 [AI 协议说明](deploy/ASSISTANT.md)。

**[本机快速启动、调试与测试](deploy/LOCAL-DEBUG.md)** 包含 Server/Admin 启停、测试账号和管理员令牌位置、Desktop Agent、Android x86 设备、移动构建、日志及完整关闭步骤。

部署与实现资料：

- [服务部署](deploy/README.md) · [账号与设备](deploy/ACCOUNTS.md) · [Web Admin](deploy/ADMIN.md)
- [移动工作台实现计划](doc/task/0922-mobile-admin/PLAN.md) · [终端架构](doc/task/0921-terminal-architecture/PLAN.md)
- [未来 AI 定义](doc/task/0921-terminal-architecture/AI-SPEC.md)

CI 文件覆盖 macOS/Linux/Windows 自动化及移动原生构建；添加工作流不等于已经在远程 CI 跑过。仓库当前尚未配置远程或发布。
