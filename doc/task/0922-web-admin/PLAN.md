# 同源 Web Admin 与管理 API

- Status: Completed
- Updated: 2026-09-22

## 目标与范围

在 `crates/server` 提供 `/admin/` 中文管理界面及 `/v2/admin/...` API，涵盖管理员登录/退出、概览、用户创建/重置密码、设备搜索/撤销和连接搜索/撤销。仅修改 server、`deploy/ADMIN.md` 和本子任务文档，不增加依赖，不读取生产凭据，不修改共享 crate 或 Cargo.lock。

## 当前状态与证据

`src/lib.rs` 已使用 admin token 的 BLAKE3 摘要鉴权并提供 SQLite/内存连接注册表。`src/account.rs` 已有用户、设备、会话、连接表，Argon2 密码函数和每秒连接授权复查。现有 `tests/accounts.rs` 覆盖账号隔离、登录限制、刷新、改密与授权过期。当前工作树无改动。

## 方案与执行

用户原文授权“先写计划无需用户 Review，写完直接按计划执行”；父计划 `/Volumes/Code/My/AITerminal/doc/task/0922-mobile-admin/PLAN.md` 已记录授权。此子计划沿用该执行范围，不需重复批准。

1. 新增独立 admin 模块，路由层统一 Bearer 鉴权及跨站请求防护，设置 no-store；查询只返回显式白名单 DTO，支持有界分页、搜索和用户过滤。
2. 复用用户名/密码校验及撤销会话函数。密码计算在线程池并沿用并发限制；写操作事务落库，撤销连接即时关闭服务端 relay，已有授权复查作补充。仅断开远端连接，不发送 Shell 关闭命令。
3. 内嵌本地 HTML/CSS/JS/lucide 资源，无 CDN。token 只保留 JS 内存；中文紧凑布局、桌面/窄屏、查询/刷新/分页、加载/空/失败状态、表单校验与二次确认。动态内容使用 textContent，CSP 禁止内联脚本及跨源资源。
4. 新增关键 API 集成测试，运行账号回归、格式与 clippy。使用回环地址和临时数据库验证浏览器流程、截图并记录部署说明和交接证据。

## 验证

测试覆盖所有管理路由未授权拒绝、跨站拒绝、输入/重复用户名、过滤/分页、敏感数据不泄露、改密/设备撤销导致 access/refresh 和连接失效、连接撤销不影响其他连接。执行 `cargo test --locked -p ai-terminal-server` 和 server clippy。浏览器使用隔离数据，覆盖登录错误、创建/重置、列表与撤销、退出、窄屏。

执行结果：本机 `stable` 为 Rust 1.94.1，使用 `cargo +stable`，5 项已有账号测试和 4 项新增管理集成测试全部通过，server clippy、rustfmt、JavaScript 语法和 diff 空白检查通过。隔离 Chromium 147 验证桌面 1440x1000、窄屏 390x844 / 320x720，覆盖 64 字符用户名和 XSS 文本展示，控制台错误为 0。桌面主内容顶对齐，窄屏使用带字段标签的记录布局。截图与验证记录见本目录，限制见 `HANDOFF.md`。

## 风险与回退

管理员 token 拥有全局权限，外部部署必须 HTTPS，禁止日志输出 Authorization。数据库结构保持不变，可撤回新增模块和路由恢复原服务。管理员页面刷新后需重新输入 token。真实设备的本地 Shell 持续运行依赖现有 desktop 行为；本次仅操作授权数据及 relay socket。

## 未决问题、歧义与确认

None.
