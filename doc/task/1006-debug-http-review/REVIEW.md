# Debug HTTP 与连接超时审查

审查开始：2026-10-06；完成：2026-10-07。分支：`main`。基线：`7bbffb4`。范围为本次工作区的 Android Debug HTTP、30 秒建连、登录错误处理、FFI 与构建脚本调整。

## 发现与修正

1. 原 HTTP 开关依赖 Rust 的 `debug_assertions`，会使使用优化原生库的 Android Debug App 无法开启 HTTP。改为显式原生特性与 App 调试标志共同控制；增加 `--release --debug-http` 构建方式。默认优化构建不带此特性，Release App 的 `TERMINAL_DEBUG=false` 与 manifest 明文限制保持禁用。
2. WebSocket 建连已放宽到 30 秒，但账号连接与旧版移动连接仍只等待对端 15 秒。统一使用 30 秒等待，并补充阶段错误信息。旧版桌面配对监听继续长期等待，避免引入周期断连。

同时检查了登录重定向/HTML 响应处理、完整错误链、UniFFI/JNA 绑定、Debug manifest 合并、地址校验及文档；未发现其余已确认的问题。HTTPS 证书校验、URL 凭据/query/fragment 限制与端到端加密保持。

## 验证

- `cargo +stable test --locked -p ai-terminal-remote -p ai-terminal-mobile --features debug-http,webrtc`：Remote 10 项、Mobile 12 项通过。
- `cargo +stable test --locked -p ai-terminal-remote --features webrtc`：10 项通过，覆盖默认公网 HTTP 拒绝。
- 优化配置分别运行 Remote 的 `--features debug-http` 与默认特性测试：各 9 项通过；前者验证显式 App 调试启用及禁用，后者验证即使请求启用也不能绕过缺失的特性。
- 对端等待回归使用实际 TCP/WebSocket 与虚拟时钟，验证 16 秒后到达的 `ready` 可被接受，以及未就绪对端在 30 秒限时后得到明确错误。
- Remote/Mobile 全目标 Clippy（`debug-http,webrtc`）在 `-D warnings` 下通过。
- Android 三 ABI 原生库、Debug APK、测试 APK、Lint 构建通过；Release Kotlin 与 manifest 编译通过。
- 核对生成的 Debug/Release 配置与合并 manifest：仅 Debug 的调试标志和明文许可为 true。
- 构建脚本的默认 Debug、WebRTC Debug、默认优化、优化 Debug HTTP，以及非 Android 参数拒绝共 5 种组合通过验证。
- Rust 格式、Python 构建脚本语法与 Git diff 检查通过。

本轮为源码与构建审查，没有重新安装手机 App 或变更服务部署。此前的手机 HTTP 登录、终端输入、历史和重连验证属于审查前版本，不代替本轮回归。
