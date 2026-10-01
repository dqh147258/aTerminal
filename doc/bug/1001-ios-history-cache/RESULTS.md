# iOS 离线历史缓存与工作目录修复

用户要求修复本轮审查确认的两个 P2 问题并提交。

## 修复

- 原始 history 分页在展开前写入现有 Rust AgentCache，继续遵守其 1 MiB/50 条限制。完整消息存入独立 `message_values` 表，按完整 scope、历史 generation 和 record ID 隔离，重启后可以离线还原。原文按 64 MiB/256 条全局预算淘汰，单条最多 4 MiB；内存采用 8 MiB/100 条缓存。正文搜索同时读取当前分页引用的完整原文，过期 generation 不参与读取或搜索。缓存写入失败有界面反馈，在线内容继续可见。
- ChatArchive 新增可选 `cwd` 字段，完整工作目录与显示名称分开。在线会话信息及目录变化更新当前账号对应索引，保持已有消息、请求状态和草稿信息；关闭、离线历史列表、上下文和目录搜索使用完整路径。旧 JSON 不含 cwd 仍可读取，在线获取会话信息后补齐；已经丢失且再无在线来源的旧路径仍使用原显示名称，不推测或伪造目录。
- 不修改 Rust RPC/FFI、账号凭据、供应商配置、共享服务或原动画会话。

## 验证

- `python3 scripts/check-ios-history.py` 通过。直接编译生产 Swift 历史流水线/原文缓存/搜索/ChatStore，并使用实际 Rust AgentCache 适配器；50 条 22 KB 消息展开后超过 1 MiB，原始页仍成功缓存，重建实例后 50 条全文均可离线读取且没有 RPC。验证末尾正文搜索、账号/会话及 generation 隔离、缓存失败反馈、迟到回调、预算淘汰、完整目录保存、旧 JSON 兼容及保留原历史。
- 既有 AgentCacheSearchChecks 两组和 ChatStoreChecks 通过。
- 专用 iPhone 15 / iOS 17.5 模拟器 4 项 XCTest 通过：完整工作目录查找关闭历史、正常正文搜索、关闭/离线历史只读、Session/Global 草稿隔离。
- x86_64 Debug App/XCTest 与 arm64 Release iOS App 构建通过；Release 保留既有静态库 debug-map 与项目方向配置警告，无编译或链接失败。
- `git diff --check`、Python 语法检查通过。

主 checkout 的忽略目录保留 `build/ios-history-checks/verification.json`、`build/ios-history-checks.log`、`build/ios-history-fix-ui.xcresult` 与对应 Debug/Release 日志。测试使用独立临时数据库和隔离 UI fixture，没有向用户 Terminal 输入。
