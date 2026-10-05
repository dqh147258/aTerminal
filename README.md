# aTerminal

aTerminal 由 Rust Desktop Agent、原生 Android/iOS 客户端和轻量协调服务器组成。Desktop 保持唯一的终端状态，手机维护显示副本；连接优先使用 WebRTC，无法直连时通过加密 WSS 中转。

**当前是可运行的开发版本，尚未完成发布验收。** Desktop Shell、账号与设备管理、端到端加密远程终端、移动工作台和 Web Admin 已实现。Android 最低支持 API 25，提供 arm64-v8a、x86_64、x86 构建。跨公网 NAT、长期性能、发布签名与商店发布仍需验收。

Android/iOS 工作台提供登录、设备和会话选择、终端画面直接输入（含回车、Tab 等特殊键）、终端历史、显示设置及最后会话恢复。AI 浮窗提供全局/当前终端 Agent、发送/追加/停止、分页历史与证据回读；模型和扩展配置保存在 Desktop。语音入口关闭。使用方式和授权边界见 [Agent 说明](deploy/ASSISTANT.md)。

**[本机快速启动、调试与测试](deploy/LOCAL-DEBUG.md)** 包含 Server/Admin 启停、测试账号和管理员令牌位置、Desktop Agent、Android x86 设备、移动构建、日志及完整关闭步骤。

部署与实现资料：

- [服务部署](deploy/README.md) · [账号与设备](deploy/ACCOUNTS.md) · [Web Admin](deploy/ADMIN.md)
- [移动工作台实现计划](doc/task/0922-mobile-admin/PLAN.md) · [终端架构](doc/task/0921-terminal-architecture/PLAN.md)
- [双层 Agent 计划与合同](doc/task/0925-terminal-agents/PLAN.md)

CI 文件覆盖 macOS/Linux/Windows 自动化及移动原生构建。测试包工作流为桌面四种目标架构和 Android 三种 ABI 生成带校验和的 Actions artifacts；产物仅在对应校验与构建步骤成功后上传，保留 14 天。使用方式、测试包边界及实际验证范围见 [测试包说明](deploy/TEST-PACKAGES.md)。GitHub Actions 显示的运行结果是验证状态依据；测试包不代表正式发布验收。

桌面命令为 `aTerminal`（构建：`cargo +stable build --locked -p ai-terminal --bin aTerminal`）。Android 与 iOS 的应用标识均为 `com.yxf.aterminal`；包名变更会作为新应用安装，需要重新登录，旧应用数据不会自动迁移。
