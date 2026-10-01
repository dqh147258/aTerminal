# 执行清单
- [x] 检查协议、cwd 权威来源及 Android/iOS 样式，记录本轮明确授权。
- [x] 实现 Desktop 持久化、真实 cwd 记录、目录验证与兼容协议扩展。
- [x] 实现 mobile-core 接口及 Android/iOS 最近目录选择和失败处理。
- [x] 完成针对性测试与可行编译检查，记录证据及协调者 UI 验收交接。

验证证据见 RESULTS.md：Rust 27 + 5 + 12 项通过，真实 cwd / 默认 cwd / idle MRU 补充回归通过；本工作树 UniFFI 生成、Android compileDebugKotlin、iOS 全源码 typecheck 通过。
