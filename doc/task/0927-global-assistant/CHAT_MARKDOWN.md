# 正常回复全文与 Markdown

用户反馈：LLM 正常返回需要直接完整展示，聊天页需要支持 Markdown。此要求替代此前对普通长消息最多显示六行的展示方式。

## 修改
- `AgentPanel.kt`：普通用户/助手消息、工具调用前的助手说明、流式回复均原位展示全文，移除六行截断和“查看完整消息”折叠入口。工具调用记录仍保留摘要与证据详情。
- `ChatMarkdown.kt`、`app/build.gradle.kts`：接入 Markwon 4.6.2 原生 Markdown；支持标题、粗体/斜体、列表、任务列表、删除线、引用、行内代码/代码块、链接和表格。文字可选择复制，配色沿用现有深色主题，适用于 Session 和全局AI助手。
- 历史接口超过 12 KiB 时返回预览；现在按原 `record_id` 自动分页读取消息原文 JSON，再展示完整文本。按原有账号/Desktop 私有目录保存原子写入的记录缓存，支持离线重开；保持原 RPC、历史分页和消息身份。读取失败显示明确的全文重试入口，未加载完的助手回复不标为已读。
- `AgentTimeline.kt` 移除旧长消息折叠判定。更新相关 instrumentation 回归；记录新依赖和 APK 内许可证。

## 验证
- APK、测试 APK、lint 构建通过；`git diff --check` 通过。
- 6 项 Android 模拟器 UI 测试通过：Markdown 样式 spans、24 项长回复末尾不省略、流式转持久记录不重复、工具证据详情、180 行原文多段读取/失败重试/离线重开、输入焦点，以及独立会话和未读状态。
- Rust 既有长历史契约测试通过，核对预览与分页原文的无损读取协议。
- 截图：`artifacts/chat-markdown/markdown-chat.png`、`markdown-tail.png`。日志在同目录。
- 本轮未修改后端协议，无需为这项显示修复再次替换 Desktop。依赖下载首次遇到临时 TLS 错误，正常重试后 Maven Central 构建通过，未添加临时仓库或降低 TLS 校验。

APK：`apps/android/app/build/outputs/apk/debug/app-debug.apk`。
