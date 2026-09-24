# Web Admin

Admin 与现有 Axum 服务同进程、同源部署，页面地址为 `https://<服务域名>/admin/`。HTML、CSS、JavaScript、图标在编译时内嵌，不需要 Node、独立前端容器或外部 CDN。更新资源后需重新编译/部署 server。

## 启动与认证

沿用服务配置：

- `AI_TERMINAL_ADMIN_TOKEN_FILE`：优先从文件读取管理员令牌，至少 32 字符；建议使用密码学随机生成的高熵值。
- `AI_TERMINAL_ADMIN_TOKEN`：未配置 token 文件时使用的替代环境变量。
- `AI_TERMINAL_DB`：SQLite 文件路径，默认 `ai-terminal.sqlite3`。
- `AI_TERMINAL_BIND`：默认 `127.0.0.1:8787`。

现有 `deploy/compose.yaml` 与 `deploy/Caddyfile` 已转发全部路径，无需新增路由。反向代理必须保留原始 `Host`，页面与 `/v2/admin/` 应同源；外部访问必须通过 HTTPS。不要在代理/访问日志中记录 `Authorization` 或请求正文。页面本身不包含凭据；登录后验证管理员令牌，不创建新的服务器登录会话。没有公开注册接口。

令牌只保留在 JavaScript 内存中，不写入 cookie、localStorage 或 sessionStorage。退出、刷新和离开页面都会清除当前页面的令牌。退出不会吊销其他浏览器中持有的同一令牌；全局轮换需更换服务配置的令牌并重启 server。不要在 URL 中传递令牌。

## 管理操作

- 概览：用户总数、未撤销设备数、在线设备数、有效连接数和服务器时间。在线设备要求 15 秒内心跳且存在有效认证会话；有效连接包括尚未过期的待连接授权，不等同于已建立 relay 的数量。
- 用户：按用户名查询、分页、创建、查看其设备、重置密码。用户名为 1 至 64 字节的 ASCII 字母/数字/`_.-@`，大小写敏感。密码为 12 至 1024 字节，使用已有 Argon2 算法。
- 设备：按名称、用户名、ID 搜索，按用户 ID 过滤，查看平台、最后心跳、在线/离线/已撤销状态并撤销。
- 连接：按用户名、两端设备名称、ID 搜索，按用户 ID 过滤，查看等待/已连接/已过期/已失效/已撤销和授权/租约期限并撤销。连接记录可能由原有 heartbeat 清理任务移除，不是永久审计日志。

创建/重置密码不回传密码或 hash。重置密码使该用户所有 access/refresh 会话和远程连接失效，但设备身份可重新登录。设备撤销会永久撤销该设备身份，重新登录需客户端生成新身份；同时使其会话和相关远程连接失效。连接撤销只断开该连接，不影响用户登录。写操作落库后主动关闭 relay，原有每秒授权复查同时保留。以上操作均不发送关闭桌面本地 Shell 的命令。

时间以浏览器本地时区展示。设备最后在线为 0 时展示“暂无记录”；改密和撤销会清零在线标记。用户表没有创建时间字段，本实现不虚构创建时间。

## API

每个管理接口均要求 `Authorization: Bearer <token>` 和 `X-Admin-Request: 1`。拒绝跨站 `Origin`、`Sec-Fetch-Site: cross-site/same-site`，不提供 CORS。命令行请求可不带 `Origin`/Fetch Metadata；浏览器页面必须同源。响应设置 `Cache-Control: no-store`。

| 方法 | 路径 | 请求 / 返回 |
| --- | --- | --- |
| GET | `/v2/admin/overview` | 概览对象 |
| GET | `/v2/admin/users` | 用户分页，仅 ID、用户名和有效设备数 |
| POST | `/v2/admin/users` | `{"username":"...","password":"..."}`，返回 201 和新用户摘要 |
| POST | `/v2/admin/users/{id}/password` | `{"password":"..."}`，返回 204 |
| GET | `/v2/admin/devices` | 设备分页，无公钥/会话凭据 |
| DELETE | `/v2/admin/devices/{id}` | 撤销，返回 204 |
| GET | `/v2/admin/connections` | 连接分页，无授权正文/密钥/凭据 |
| DELETE | `/v2/admin/connections/{id}` | 撤销，返回 204 |

列表查询参数：`q`（最多 128 字节，字面子串、不区分 ASCII 大小写）、`user_id`（设备/连接使用）、`limit`（1 至 100，默认 25）、`offset`（0 至 1,000,000）。返回 `{items,total,offset,limit}`。错误状态为 400/422 输入错误、401 凭据错误、403 来源错误、404 不存在、409 用户重名、413 正文超过 8192 字节、429 密码计算并发限制、500 内部错误；错误不回传数据库/凭据信息。

页面使用 CSP、禁止 framing、`nosniff` 和 `no-referrer`。动态数据使用文本节点，不通过 HTML 解析。管理员拥有全局权限；目前不提供分级角色、令牌管理界面、永久操作审计、用户删除或设备恢复。

## 验证与本地预览

```sh
cargo +stable test --locked -p ai-terminal-server
cargo +stable clippy --locked -p ai-terminal-server --all-targets -- -D warnings
cargo +stable fmt -p ai-terminal-server -- --check
```

本子任务预览脚本为 `doc/task/0922-web-admin/start-preview.cjs`。先构建 server，再执行脚本；它只绑定随机回环端口，使用新建的权限 0700 临时目录、独立 SQLite 和权限 0600 的随机 token 文件。输出包含 URL 与文件路径，不含令牌。进程元数据 `preview.json` 也只保存在该临时目录，不进入源码文档，不应作为生产配置。浏览器回归脚本 `verify-browser.cjs` 通过环境变量 `PREVIEW_FILE` 读取此元数据，使用本机 Playwright（通过 `NODE_PATH` 指定），可通过 `CHROMIUM_PATH` 指定隔离浏览器可执行文件。它只使用预览凭据，创建 QA 用户/设备，验证表单、撤销、XSS、错误状态和移动布局，截图不包含令牌。

## 图标来源

`crates/server/admin/lucide.js` 是设计资源中的 Lucide **v0.468.0** 本地构建，原始项目为 <https://github.com/lucide-icons/lucide>。对应版本 ISC 许可证从 <https://unpkg.com/lucide@0.468.0/LICENSE> 获取，保留为 `crates/server/admin/LUCIDE-LICENSE`，编译后也可访问 `/admin/LUCIDE-LICENSE`。图标不通过外部网络加载。
