# Admin 交付

## 改动

- `crates/server/src/admin.rs`：全部管理 API 的统一 Bearer/X-Admin-Request 鉴权、跨站拒绝、no-store/CSP、安全 DTO、搜索/过滤/有界分页、用户创建/重置、设备/连接撤销、事务与主动 relay 断开。
- `crates/server/src/lib.rs`：合并 Admin 路由。
- `crates/server/src/account.rs`：仅将用户名校验、密码 hash、设备会话撤销三个函数设为 `pub(super)` 供复用。
- `crates/server/admin/`：内嵌中文管理页、CSS、JS、本地 Lucide 0.468.0 及同版本 ISC 许可证。来源为设计资源；许可证可信版本源见 `deploy/ADMIN.md`。
- `crates/server/tests/admin.rs`：4 项管理集成测试。
- `deploy/ADMIN.md`：部署、权限/退出语义、API 合同、操作影响、图标来源和验证方法。
- 本目录：计划、验证脚本/记录、截图。未修改依赖、Cargo.lock、根 README、共享 crates；未 commit/merge 或关闭工作树。

## 验证证据

Rust 工具链：`rustc +stable --version` = `rustc 1.94.1 (e408947bf 2026-03-25)`。固定别名触发重复下载，因此使用相同版本的已安装 stable。

| 检查 | 结果 |
| --- | --- |
| `cargo +stable test --locked -p ai-terminal-server` | 5 个已有 accounts + 4 个新增 admin 测试全部通过 |
| `cargo +stable clippy --locked -p ai-terminal-server --all-targets -- -D warnings` | 通过 |
| `cargo +stable fmt -p ai-terminal-server -- --check` | 通过 |
| `cargo +stable build --locked -p ai-terminal-server` | 通过，预览已运行最终 UI 构建 |
| `node --check crates/server/admin/app.js` | 通过 |
| `git diff --check` | 通过 |
| `verify-browser.cjs` | Chromium 147.0.7727.15，1440x1000、390x844、320x720 通过，控制台错误 0 |

集成测试使用临时 SQLite 和回环监听，覆盖所有管理路由未认证/跨站拒绝、同源通过、CORS 不开放、CSP/no-store、用户名/密码/未知字段校验、重复名、分页/搜索/用户过滤、敏感字段不泄漏、过期状态、真实 WebSocket 关闭、改密使全部 access/refresh 失效且其他用户不受影响、设备撤销、单连接撤销不影响另一个已建立连接、404。

浏览器覆盖登录失败/成功、密码长度和确认校验、用户创建与重名、改密、按用户查看设备、恶意设备名作为字面文本展示、设备撤销且 access 返回 401、连接撤销、搜索空态、503 错误和刷新恢复、64 字符用户名在窄屏换行、浏览器本地/会话存储为空、退出及刷新后重新登录。结果记录为 `browser-verification.json`。

内置 browser bootstrap 返回 `missing field sandboxPolicy`，按照协调者授权使用本机独立 Chromium fallback。Chromium 需在文件系统沙箱外启动，仅连接隔离回环服务，不使用个人浏览器 profile。

## 截图

- `users-desktop.png`：顶部 30px 对齐的用户列表、概览和操作。
- `devices-desktop.png`：设备状态、用户筛选和 XSS 字面文本。
- `connections-desktop.png`：连接状态与撤销结果。
- `revoke-confirmation.png`：明确操作影响的确认框。
- `devices-mobile.png`：390px 字段布局与最大长度用户名。
- `create-mobile.png`：窄屏创建表单。
- `login-desktop.png`、`login-mobile.png`：登录页。

截图没有令牌或密码值，仅包含固定假密码对应的 QA 账号/测试设备和不具认证作用的 ID。预览随机 token 对源码、文档、截图文件的精确值扫描为 0 匹配；所有密码输入截图时为空。凭据权限 0600，隔离目录权限 0700。预览 URL/凭据文件路径通过完成报告传递，进程元数据仅在临时目录，源码中没有 `preview.json`。

## 限制与后续真机验证

管理界面未加入分级管理员角色、永久审计、用户删除、设备恢复或令牌轮换页面；连接表沿用 heartbeat 清理，并非审计历史。退出清除当前页面令牌；共享管理员令牌的全局失效仍需配置轮换并重启服务。

服务端已经验证撤销只断开 relay 并使认证失效，不发送终端会话关闭命令。本子任务未操作真实手机/桌面账号；协调者可在整体验收时用一次桌面本地长运行 Shell 验证：手机连接后由 Admin 撤销连接或重置密码，手机连接失效，桌面本地 Shell 和任务仍继续运行。
