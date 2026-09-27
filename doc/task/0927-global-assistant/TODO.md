# 全局AI助手独立会话改造 TODO

- Status: Completed
- Updated: 2026-09-27

## Checklist

- [x] 后端独立全局会话、分页摘要、兼容与权限验证
- [x] Android 独立入口、列表、聊天、草稿与未读
- [x] 构建、回归、模拟器与截图核验

- [x] 按新增侧边栏截图调整工作空间布局；构建、lint、侧栏与全局导航回归通过，见 SIDEBAR.md。

- [x] 正常 LLM 回复原位展示全文并支持 Markdown；长历史原文分页/缓存、6 项 UI 和长历史协议回归通过，见 CHAT_MARKDOWN.md。

- [x] 工具详情全屏改版，参数/结果原位展开及复制；UI、构建/lint 与真实证据端到端通过，见 TOOL_DETAILS.md。

- [x] 按用户要求移除 Android 旧手机归档入口及导入/展示实现；构建、lint 与侧栏回归通过。

## Verification evidence

- None yet.

## Verification evidence

- Rust runtime 28 项、Desktop 23 项通过。
- Android 构建与 lint 通过（0 errors）；7 项 UI 回归通过，覆盖新导航、未读/运行状态、草稿、迟到发送、旧草稿兼容、图片、分页、横屏、180dp 可用高度及输入焦点。
- 加密链路端到端测试通过，证明多个真实全局会话及旧记录兼容；使用本地确定性模型。
- 验证日志和截图：`artifacts/global-assistant/`；详细记录：`RESULTS.md`。
