# Admin 子任务

用户授权原文：根据设计实现移动 App，服务缺少 Web Admin，功能由我们决定；先写计划无需用户 Review，写完直接按计划执行；分配 Admin、Android、iOS 三个 worktree 子任务。完整父计划在 `/Volumes/Code/My/AITerminal/doc/task/0922-mobile-admin/PLAN.md`，请读取。遵循 worktree-tasks 执行器登记/报告要求，不再申请重复批准，也不继续分派子任务。

你负责 `crates/server/**`（含静态 Admin 资源）、`deploy/ADMIN.md` 和你自己的 `doc/task/...`。实现 Axum 同源 `/admin/` 管理页面及 `/v2/admin/...` API；复用现有 admin token 鉴权、SQLite 和账号安全语义。建议功能：登录/退出、概览计数、用户列表/创建/重置密码、设备列表/搜索/按用户过滤/撤销、连接列表/撤销、时间和状态展示。操作需真实落库，撤销/改密应切断既有连接且保留本地 Shell。返回值不泄漏密码 hash、会话凭据、public key 或连接授权正文。token 仅内存，不写 localStorage；无公开注册。所有管理 API 必须鉴权，注意跨站请求、输入验证和 XSS。可复用已有密码函数并做最小可见性调整。

UI 做实用、紧凑、完整的中文管理工具，延续移动设计的深灰/浅蓝与绿/红状态色，带搜索、刷新、加载/空/错误、表单校验、操作确认和响应式布局。使用本地 lucide 图标（设计资源可读），不要新建营销页，不依赖外部 CDN。纯 HTML/CSS/JS 配合 Axum 静态资源即可，无需单独 Node 部署。

关键 API 回归测试和已有账号测试必须跑；可用 browser skill 做本地实际浏览器验证，服务仅回环+隔离临时 DB/凭据。不读或输出 deploy/secrets。写部署说明。不要修改 Cargo.lock/根 README/共享 Rust crates；确需依赖修改先信息通知协调者。不 commit、不合并、不删除、不关闭工作树/终端。

先写子计划后直接执行。完成提供修改文件、测试证据、访问 URL、截图、残留限制，通过 CLI 成功上报 completed。协调者会审查并把改动集成回原目录。
