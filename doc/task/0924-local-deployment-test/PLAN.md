# x86 本地部署全流程验收与文档修订

- Status: Completed
- Updated: 2026-09-24

## 目标与范围

保持 Android/iOS 的 AI 对话与语音入口为不可交互占位，修正当前交付与部署文档；确认 Android API 25/x86 构建和运行。按用户本轮澄清，在 `127.0.0.1:62001` 上以实际本地 Docker Server、真实 Desktop Agent 和 PTY 完成登录、终端、恢复、关闭和退出验收；设备必须直接访问主机 `192.168.0.36` 的 HTTPS 服务。测试账号与 Docker 数据卷独立于已有本地部署，结束后清理本次资源；不启用模型或语音。

## 当前状态与证据

`adb devices -l` 显示目标设备在线，SDK 25、ABI `x86`。仓库已有三 ABI 构建配置和 2026-09-23 的设备通过记录。`deploy/ASSISTANT.md`、`doc/task/0922-mobile-admin/RESULTS.md` 与 `HANDOFF.md` 曾把移动 AI/语音描述为可用，与 Android `MainActivity.openChat()` 和 iOS `ChatPanel` 的占位界面不符。旧 Admin 预览端口 `64535` 已停止；README 的 iOS 模拟器构建命令在当前 x86_64 主机默认尝试 arm64，指定 `ARCHS=x86_64` 才通过。原 `ai-terminal-dev` 容器保持停止；本次使用单独 Compose 项目测试当前镜像，未改动既有数据卷。

## 方案与执行

用户已明确要求直接调整文档并做真实本地部署全流程测试，且提供了目标 Android 设备；视为本计划的执行授权。

1. 将产品与部署文档改为 AI/语音尚未开放，区分保留的历史代码、历史验证与当前入口；将 Admin 预览改为可重复启动说明，修正 x86 ABI 和 iOS 构建命令。
2. 用当前源码构建 Android x86 原生库、APK/测试 APK 及 Server Docker 镜像，核对 minSdk、ABI 与镜像健康。
3. 启动隔离的真实本地 Compose 部署和 LAN TLS 代理，创建仅用于验收的账号与 Desktop Agent/PTY；目标 x86 设备使用 `https://192.168.0.36:7200` 与本地 CA 正常校验证书，不使用 `adb reverse`、内存服务或模型桩。
4. 在设备上执行状态/界面检查和真实登录、终端输入、120 列横滑、设置、AI 占位、后台恢复、Ctrl-C、关闭会话后连接可用、退出；记录测试结果并清理本次账号、Agent 和 Compose 数据卷。

执行结果：为验收本地私有 CA，在 Android Debug `acceptance_test` 路径增加 App 私有 `acceptance-ca.pem` 的显式读取，仍由 Rust TLS 验证链和 IP，普通登录路径不变。真实 Docker/Agent/PTY 与 x86 设备的局域网端到端测试通过，过程、边界和清理见 `RESULTS.md`。

## 验证

`build-mobile.py` x86、`prepare-bindings.py`、Gradle assemble/test APK/lint、APK 元信息与当前源码 Server Docker 构建通过；前一轮 Rust 工作区测试已通过，本轮没有修改 Rust 源码。设备 16 项状态/界面测试中 15 项通过，API 29 专属项按条件跳过；局域网真实 PTY 完整工作流 1 项通过、0 跳过、`passed=true`。App 使用 `https://192.168.0.36:7200`，设备无 `adb reverse`，证书由显式本地 CA 正常校验。详见 `RESULTS.md`；不把局域网验证称为公网或蜂窝网络验收。

## 风险与回退

设备已有 App 数据，安装只使用 `-r`；仪器测试只用 `acceptance-*` 私有存储，先确认前台 App，失败后停止输入。专用 Compose 项目/卷和 Agent 状态目录独立于原有部署；所有测试命令仅作用于本次新建 PTY，清理时不得删除既有服务的数据卷或账号。本机私有 CA 必须显式受信且验证 IP 主机名，不使用跳过证书校验的方式通过测试。

## 未决问题、歧义与确认

None.
