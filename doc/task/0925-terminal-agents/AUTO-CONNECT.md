# 自动连接与终端首帧进入首页

2026-09-26，用户授权调查当前 Desktop/App 自动连接问题并实现无空首页过渡的登录流程。

## 现场与根因

当前本地账号有一个在线 Desktop 和一个运行中的 Terminal。现场只读观察中，旧 App 从“连接中”到“打开会话”最终自动显示终端；没有执行人工连接。因此不能把现场现象简单归为没有任何自动连接代码或账号不一致。

实际冷启动进一步发现保存账号的 HTTPS 请求约 15 秒超时；与 App 无关的 Android HTTPS 探针也失败。保存 CA 与构建 CA 完全一致，原始 TCP 可达，但 LAN 地址和宿主机网关的 TLS 都超时，TLS 1.2 亦同；同一个服务通过临时 ADB 隧道约 25 ms 返回 200。保留 AVD 数据重启模拟器后，原 LAN HTTPS 恢复（TLS 默认约 660 ms，TLS 1.2 约 1008 ms）。据此确认现场另有模拟器自身网络转发异常，不通过关闭证书校验、改账号或修改宿主代理绕过。临时诊断测试和隧道已移除。

已确认的实现缺口：

- Android `signedIn`、iOS 恢复 username 后立刻显示首页；查询设备、建立加密通道、查询会话、选择会话和取得首帧随后才发生，造成空首页、设备状态及终端之间的跳变。iOS identity 改变还会提前打开设备面板。
- 未连接时只在启动/回前台/手动刷新查询设备。周期心跳只更新本机在线状态，Desktop 后上线不会被发现。
- 心跳缺少在途标志；慢请求期间可持续向单线程 worker 排队。本轮增加去重；没有将未测量的现场延迟全部归因于这一点。

## 实现

Android/iOS 都增加独立的连接准备状态。登录/恢复身份后查询设备并自动连接，直到收到可渲染的 Terminal 首帧，再将首页设为可见。设备选择、会话选择和首帧准备期间不展示空的 Terminal 首页；iOS 不再因身份恢复自动弹设备面板。

保留现有选择规则：唯一在线 Desktop 自动连接其最新运行会话；多个 Desktop 时按已有记录恢复，否则等待明确选择。不会随机选择多个设备，也不会自动创建或关闭用户 Shell。

没有在线设备、没有运行会话及连接错误是明确的结束状态，页面不会一直加载。前台未连接时按现有 10 秒心跳周期重新发现设备；已连接但没有所选会话时刷新会话，随后新建的终端也能自动显示。心跳只保留一个在途任务，登录/连接准备期间不排队心跳。后台暂停、恢复、账号代际和连接代际校验继续有效。

## 自动化

Android `AutoConnectTest` 使用 `OnPreDrawListener` 审计首页绘制：在已知有运行终端的连接路径中，首页可见时必须同时具备所选会话和 TerminalView。覆盖登录首次展示和 Activity 重建恢复，不能只凭最终能连接断言没有闪烁。

独立临时服务/账号/真实 PTY 的三种场景通过：

1. Desktop 与 Terminal 已在线：自动连接、首次首页有终端；手动切换后的刷新保持选择，重建恢复最新会话。
2. App 先登录、Desktop 后上线：先明确显示无在线设备，随后不点击或重启 App 自动打开终端。
3. Desktop 在线但没有会话：结束加载并提示无会话；随后创建真实 PTY，App 自动打开并通过首帧审计。

原六阶段跨进程登录回归也通过：登录、恢复、离线启动、旧请求晚写入与登出、退出后重启、再次登录。测试不清除主账号，不触碰用户的真实 Shell。

复跑命令（原生构建产物和 APK 已准备）：

```sh
PATH=/Users/carl/Library/Android/sdk/platform-tools:$PATH \
  python3 scripts/test-android-autoconnect.py --serial emulator-5586 \
  --output .local/emulator-5586-auto-entry
# 分别追加 --late-desktop / --late-session / --login-persistence 运行另外场景。
```

报告分别位于 `.local/emulator-5586-auto-entry/`、`.local/emulator-5586-auto-late/`、`.local/emulator-5586-auto-late-session/`、`.local/emulator-5586-auto-entry-login/`。

Android assembleDebug/assembleDebugAndroidTest/lint 通过，iOS build-for-testing 通过。本轮 iOS 未运行设备 UI 测试，不把编译结果等同首帧运行验证。未改 Rust/FFI 或传输协议，不重复 Rust 全量测试。

## 真实工作空间结果

保留用户 AVD 数据修复网络后，恢复原串号 `emulator-5586`。`login-android-local.py --expect-terminal` 两次独立进程启动均自动选择用户已存在的会话 `691706fbd1f536a9`，从 Activity 启动到首帧就绪分别为 **1894 ms / 1751 ms**。账号与设备身份一致，最终普通 launcher 启动保持终端已连接，截图已复核。没有重启 Desktop/Server、关闭 Shell 或发送终端输入。

报告和截图：`.local/emulator-5586-auto-entry-live/results.json`、`signed-in.png`。探针结果保存在 `probe-after-restart.json`；临时诊断代码已删除。三种自动连接回归、六阶段登录回归、两次实际冷启动都通过。
