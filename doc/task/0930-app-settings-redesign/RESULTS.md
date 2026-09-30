# Android 新 UI 与设置重构验收

- Date: 2026-10-01
- Status: Completed
- Scope: Android；iOS 后续使用同一参考同步。

Android 已采用最新实际 HTML 的深蓝/亮蓝主题、分组信息行、显示卡片与固定底栏。设置中的供应商、模型、目录、默认绑定、读取、MCP 和 Skills 使用全屏配置流程，支持父页/系统返回、取消、字段错误、忙碌状态、失败保留草稿与修订冲突后显式重试。密钥留空保留原凭据，离开编辑即清除内存字段；旧配置未暴露字段保持。

按用户例外：品牌为 **aTerminal**；登录服务器摘要/展开编辑交互沿用现有实现并统一风格；不提供旧手机归档或旧手机数据迁移。正常账号的 Session/Global 历史、草稿、图片、工具证据、独立 Global 多会话和终端连接仍保留。

## 验证

| 检查 | 结果 |
| --- | --- |
| Build / lint | Kotlin app/test compile、Debug/test APK build、lintDebug 通过。lint 有现存国际化/弃用等 warning，不宣称零 warning。 |
| 设置确定性回归 | 最终 11 个 `AgentSettingsUiTest` 通过：本地校验、失败草稿、busy、Azure、清 key、成功保存不依赖额外 show、冲突重试、字段保留、目录能力更新与迟到屏障、绑定、读取、MCP、Skill、思考约束。 |
| Workspace | 登录字段/服务器结构、账号来源返回、显示偏好/横竖屏 3 个用例通过。 |
| 真实 encrypted RPC | `AgentReadingUiTest` 通过：读取参数保存、PTY 证据、TUI 原文/过滤、图片消息至确定性 HTTP 模型与 Global 链路。 |
| 真实 MCP / Skill 表单 | `LiveSettingsExtensionsUiTest` 通过：MCP 导入/编辑/启停/确认删除；完整 Desktop Skill 安装/读写/启停/确认删除。编辑前后非 Markdown 资源 SHA256 保持，安装/编辑使用不可变版本包，其他完整配置哈希保持；仅清理本轮 UUID 条目。 |
| 小屏 / 大字体 / IME | 临时 800×1600、font_scale=1.3 且键盘打开时，取消和保存均可见、可操作，边界 y=572–704；完成后恢复 1080×2340、font_scale=1.0。 |
| 实际 UI | 正常账号与同一 Desktop/PTY 连接，80×25 远端列数保持；逐页截图与统一原型对照，登录截图仅使用隔离页面。 |

共 16 个最终设备 instrumentation 用例通过，另有小屏/IME 视觉检查。日志/JSON 保存在 [evidence](evidence/)，页面对应关系见 [Android 实现图集](ANDROID_REFERENCE.md)。

## 永久参考

- [Android / iOS 共用原型图集](UI_REFERENCE.md)：20 张 PNG、页面结构、源文件 SHA256。
- [Android 实现图集](ANDROID_REFERENCE.md)：正常数据/隔离登录、尺寸/密度/字体、对应关系和压力场景。
- [独立代码审查](REVIEW.md)：目录能力覆盖与测试提前断言两项问题已在 `635851e` 修复；主题 API25 兼容已静态复核。

实现源提交 `4b68623`、审查修复 `635851e`、真实扩展测试 `dbf97e3` 通过 Git 同步到 `main`，历史提交保留 `[未Review]` 标记。协调者另补框架默认控件强调色与 aTerminal 品牌文案。编译和测试不代表商店发布或人工审查。

## 边界与保留环境

本轮没有改 iOS。系统 SAF 手机文件夹选择/上传本轮未再次走实际系统选择器；完整上传代码与服务端限制保留，Desktop 完整目录安装和编辑后的资源保留已实测。真实图片链路测试使用确定性模型，不能扩展为 OpenRouter 视觉质量实测。

模拟器与测试 Terminal 保持运行，正常账号/设备身份和默认 OpenRouter profile 保留。恢复的显示偏好为原值 11 sp / 73%，系统字体比例为 1.0，未清除用户磁盘数据。

Task: `20260930-230203-393-app-settings-redesign`

| 子任务 | 工作树 / 分支 | iTerm SessionID |
| --- | --- | --- |
| `20260930-234528-581-android-settings-ui` | `/Volumes/Code/public-worktree/aTerminal/0930-android-settings-ui` / `worktree/0930-android-settings-ui` | `w3t0p9:BD74BF12-2933-4253-A5F2-ABC600EF99C9` |
| `20260930-230455-025-ui-settings-audit` | `/Volumes/Code/public-worktree/aTerminal/0930-ui-settings-audit` / `worktree/0930-ui-settings-audit` | `w3t0p7:D3E28D54-9B07-49ED-AB9A-254DCAD06DD8` |
