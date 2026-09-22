# AI Terminal 架构调研与实施方案 TODO

- Status: In progress
- Updated: 2026-09-22

## Checklist

- [x] P0：检查工具链、建立 Cargo workspace 与可复现构建配置
- [x] P0：实现统一屏幕模型、快照/增量与状态校验测试
- [x] P0：对接 Alacritty 引擎并验证 UTF-8、查询、备用屏、resize
- [x] P0：实现本地 PTY/ConPTY 原型、宿主增量绘制与退出恢复
- [x] P0：验证 UniFFI / libdatachannel 的桌面与移动构建及最小原生视图
- [ ] P0：验证 PowerShell/PSReadLine 与跨宿主兼容门槛（用户回复“继续”，已延后到发布前；未通过）
- [x] P1：后台会话、attach/detach、独占控制、输入去重与历史（本机验证通过，Windows 仍待 CI）
- [x] P2：配对、E2EE、Server/WSS 与双移动端远程基线（核心 E2E、双端构建、iOS 模拟器运行通过）
- [x] P3：WebRTC 优先、RTT p95/ACK 质量评估与超时切换去重（真实本机链路测试通过）
- [ ] P3：跨公网 NAT、TURN、蜂窝/企业网络及真实丢包抖动矩阵
- [x] P4：Compose 本地部署、健康检查和 20 对连接短时资源负载
- [x] P4：原生会话选择/创建、语义输入、历史、缩放、配对安全存储；iOS 模拟器显示验证
- [ ] P4：Android 运行/输入法、双端后台恢复与无障碍完整验收，细粒度文本选择
- [ ] P4：30 分钟低配长测、本地交互附加延迟/移动帧耗时指标
- [x] P4：Android arm64/x86_64 APK、iOS 真机/模拟器 Rust 库和 XCFramework、Xcode 工程
- [ ] P4：五平台发布级构建、Windows 运行、签名、公网 TLS 部署与完整许可证/SBOM

## 2026-09-22 局域网试用

- [x] 配置 7200 HTTPS/WSS、7201 回环健康入口，生成固定 IP 的测试证书并验证容器健康
- [x] 为配对配置增加可选私有 CA，验证 HTTPS/WSS 信任与拒绝未信任证书
- [x] 创建持久试用 Agent、桌面会话和配对，重建 iOS 应用
- [x] iOS 模拟器经 192.168.0.36:7200 完成读屏、输入、保存配对与重启恢复
- [x] 留下可复用启动/测试配置及操作说明，保留试用服务
- [x] 用户随后要求全部关闭：停止本项目容器、Agent/Shell、模拟器及窗口；保留配对/数据并补齐完整本地调试文档

本次追加任务已完成，整体首版发布计划仍为 In progress。证据：

- 后续关闭指令已执行；`scripts/stop-local.py` 重复运行成功，将旧会话元数据归档为 `demo.last.json`，下次启动沿用配对创建新 Shell。文档 `deploy/IOS-LAN-TRIAL.md` 已补齐启动、重建、日志/断点、验证、关闭及故障排查；服务不再保持运行。
- 关闭核对：容器 Exited、7200/7201 无监听、Agent 无进程、iPhone SE Shutdown、Simulator 窗口已退出。旧 Server 容器退出码 137；局域网 Compose 已改为 SIGINT 对接现有 ctrl_c 处理，配置解析通过，下次启动生效，未重新启动做运行验证。

- `python3 scripts/prepare-lan.py`：Server/TLS 容器 Healthy；192.168.0.36 和 127.0.0.1 的 7200 使用测试 CA 校验返回 `ok`，7201 仅回环健康入口；原宿主 8787 映射已移除，SQLite 数据卷保留。
- `curl` 无 CA / 错误主机名均以证书错误拒绝；新增私有 CA 测试，空值、非法 PEM、超限数据均不能退回不安全连接。
- `python3 scripts/run-ios-lan.py --toolchain stable`：相同桌面会话 `4dc05e91002848ba`，WebRTC direct，iOS 与权威 PTY 均观察到独立输出 `IOS_LAN_7200_20260922_004620`。
- `--restore --relay-only`：未导入邀请，从 Keychain 恢复，WSS relay，独立输出 `IOS_LAN_7200_20260922_004938`。连接均使用 HTTPS/WSS 7200 的私有 CA 校验。
- `--restore --interactive`：恢复直连只读画面，不再自动发送测试命令，保留模拟器与试用进程。截图 `build/screenshots/ios-lan-direct-restored-interactive.png`。
- `cargo +stable test --workspace --exclude ai-terminal-bindgen`：23 项通过；全 workspace Clippy `-D warnings`、rustfmt、Xcode 构建通过。
- 修复实际验证问题：nginx 只读根目录的临时路径全部转到 tmpfs；iOS 控制开关改为绑定真实控制状态，避免自动验证接管后仍显示关闭。
- 运行配置 `deploy/ios-lan.json`，私有资料 `.local/ios-lan/`、`deploy/secrets/`，说明 `deploy/IOS-LAN-TRIAL.md`。未向日志/文档输出管理员 token、CA 私钥或完整邀请。

## 历史验证证据

2026-09-21 P0 证据：

- `cargo +stable test --locked -p ai-terminal -p ai-terminal-engine -p ai-terminal-protocol -p ai-terminal-mobile`：12 个测试通过，包括真实 PTY、退出码、UTF-8 分块、查询、备用屏、同步更新 EOF、快照/增量与校验。
- 同组 crate 的 clippy `--all-targets -- -D warnings` 和 rustfmt 通过；本机 stable 实际为 1.94.1。
- `cargo +stable test -p ai-terminal-transport --target-dir target/native-webrtc --test loopback`：真实本机 ICE/DTLS/SCTP 8 KiB 互传通过；没有宣称 NAT 穿透/远程协议完成。transport clippy 通过。
- `scripts/test-host-terminal.py` 对 sh、zsh、Vim 的实际 PTY 输入输出和宿主模式恢复通过；修复 EOF 最后一批画面丢失，测试退出等待竞态也已修复。
- Linux Docker rustc 1.98.0、禁用网络、只读 Cargo registry 下同组 12 项测试通过；非精确 1.94.1 Linux 结果。
- `scripts/build-mobile.py android --webrtc`：Android arm64 `.so`，固定 NDK r28、ABI、API 26，真实链接 WebRTC；包含配套 libc++_shared.so。
- `scripts/build-mobile.py ios --webrtc`：iOS arm64 `.a`；Xcode 15.4 / SDK 17.5，显式部署版本 15.0。`build-ios-probe.py` 原生 SwiftUI/UIKit 未签名应用链接通过。
- Gradle `:app:assembleDebug` 通过；APK 的 Rust/C++/JNA 三个 arm64 ELF 均验证到 16 KiB LOAD alignment。未安装到设备。
- Swift → UniFFI → Rust 实际调用测试通过：中文/组合字符、重复快照、reset；不是 iOS 真机结果。
- 已准备 Windows/Linux/macOS CI，未建立远程仓库或执行远程 CI。ConPTY 原型来自 portable-pty，Windows 运行验证单列未完成。
- 原生构建已修复实际问题：上游 CMake 未指定 Android ABI、缺少 C++ runtime 打包、UniFFI Android annotations 依赖、iOS 隐式 SDK/部署版本不一致。

2026-09-21 执行顺序调整：用户在收到 Windows 环境缺失说明后回复“继续”，开始 P1；Windows 验证仍是发布前必过项。


P1 本机证据：后台 Agent 独立持有 PTY，私有目录/随机凭据保护 loopback RPC；使用持久连接、单会话 actor、有界输入队列和有限增量窗口。新增跨进程测试验证 detach 后进程保留、重新接管、输入重试去重、冲突序号及旧控制者拒绝、关闭/Agent shutdown。共 15 项终端/Agent/协议测试与 clippy 通过，managed CLI 的 sh/zsh/Vim 交互回归通过。历史读取有界且不移动实时光标，当前为点时刻页面，稳定历史块 ID 留作远程分页实现。修复 macOS accepted socket 继承 nonblocking 导致连接立即断开的实际问题。

P2/P3 证据（2026-09-21）：

- `cargo +stable test --workspace --exclude ai-terminal-bindgen` 与全 workspace clippy 通过；新增移动共享核心 E2E 验证连接、创建、语义输入、输出、只读授权；测试检查独立输出行，避免将命令回显误判为执行结果。
- Noise NKpsk0 固定桌面公钥与配对 PSK；WSS 承载 Noise 密文和经认证的 WebRTC 信令，直连承载 DTLS/SCTP。两条路径均端到端保密；直连依赖 Noise 认证后的 SDP fingerprint，而不是复用跨路径的 Noise record nonce。
- 真实 DataChannel 测试建立直连后故意延迟回复，客户端 500 ms 后经中转重试；缓存响应确保执行计数仍为一次，迟到回复不会回退状态。超大消息/背压改走中转；应用不丢弃原始终端字节。
- p95 使用最多 20 个应用探测样本；持续质量劣化/ACK 停顿可转中转，10 秒冷却降低振荡。仍需真实弱网调参，不宣称全部网络可用。
- Server 端撤销立即关闭中转控制连接；直连仍依赖该控制连接/租约，因此撤销或信令断开会停止远程连接，本地 Shell 继续运行。
- iPhone SE iOS 17.5 模拟器：真实 Rust Core → Docker Server → Desktop Agent 完成配对并显示真实 PTY，状态为直连；截图 `build/screenshots/ios-se-live.png`。Keychain 通过正式 Xcode 的 simulator entitlements 工作；手工链接 probe 仅用于构建/渲染，不用于 Keychain 验收。
- 同一模拟器终止/重新启动后不传入邀请文件，从 Keychain 恢复配对并接管原会话，成功执行验证输入，独立输出行 `SIMULATOR_INPUT_OK`；截图 `build/screenshots/ios-se-input.png`。这不等于蜂窝网络/真机后台的完整矩阵通过。
- Android `.so` 与 APK 重新编译通过（arm64/x86_64），API 33 模拟器两次启动保持 offline，已停止本任务进程，未计为运行通过。

P4 部分证据：

- `docker compose -p ai-terminal-dev -f deploy/compose.local.yaml -f deploy/compose.test-network.yaml up -d --no-build --wait`：Healthy；地址 `http://127.0.0.1:8787/healthz`。
- 本机 Docker 默认地址池耗尽，仅给本项目配置 `10.253.77.0/24`。未清理/修改其他项目网络。
- 镜像仓库代理 EOF，标准多阶段拉取未通过；采用本机缓存 Rust 1.98.0 与 Debian trixie digest、离线 Cargo cache 构建 Release 二进制，再用 Dockerfile.runtime 打包并启动。不是精确 Rust 1.94.1 / bookworm 的完整部署验证。
- `relay_load ... 20 30`：20 对连接、每对 64 KiB/s、30 秒，累计 payload 39,324,000 bytes；抽样进程 RSS 10,372 KiB、VmHWM 11,068 KiB，容器 working set 7.941 MiB。只包含本机中转，不含公网 TLS、TURN、Docker VM 或操作系统内存；不是 30 分钟压测。
- `scripts/package-ios.py --simulator-target x86_64-apple-ios` 生成 arm64 真机 + x86_64 模拟器 XCFramework；当前为 Debug 静态库，425 MiB，不能当作最终 App 包体。
- Xcode 工程 `apps/ios/AITerminal.xcodeproj` 构建成功，SDK 17.5 / 最低 iOS 15，模拟器签名由 Xcode 生成；真机签名团队尚未指定。
- 最终全 workspace 共 22 项自动化测试通过，`cargo +stable clippy --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings` 与 `cargo +stable fmt --all --check` 通过；文档本地链接检查通过。
