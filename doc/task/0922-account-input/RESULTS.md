# 账号登录与输入优化结果

2026-09-22。实现、自动化验证和模拟器流程已完成；真机/公网边界见 [HANDOFF.md](HANDOFF.md)。使用入口见 [账号使用说明](../../../deploy/ACCOUNTS.md)。

同日追加 Android 12 真机验证已完成，结果见 [ANDROID-DEVICE.md](ANDROID-DEVICE.md)。登录、3,000 字符压力、直连/USB 中转与后台恢复通过；修复绘制开销并使用 Release 原生库后，持续输出整帧 p95 从 55.58ms 降至 22.81ms，尚未达到 16.7ms。以下初轮“无 Android 真机”记录仅代表追加验证前的环境。

最新一轮以可用性和一致性优先的优化、固定节拍测量及方案取舍见 [FOLLOWUP.md](FOLLOWUP.md)。本页历史结果不替代该轮真机结论。

## 交付行为

- Server 内置账号密码、管理员本机创建/重置、Argon2id、登录限流、刷新令牌轮换、设备归属/列表/撤销和短期连接授权。
- Desktop CLI `auth login/status/logout`、`devices list/revoke`；后台 Agent 刷新凭据和发现连接。macOS Keychain 与显式 Unix 私有文件模式均已做真实 CLI 验证；Windows/Linux 原生凭据库运行仍需对应环境验收。
- iOS/Android 登录、账号菜单、设备列表、改密、移除设备、退出；设备私钥不上传，Noise IK 双端认证绑定连接授权，WebRTC fingerprint 经加密认证信令交换。
- 登录账号后关闭旧配对通路，退出不自动恢复旧邀请；账号切换不转移旧 Shell，远程访问按会话所属账号校验。本地 CLI 不要求登录。
- `stream/2` 主动状态订阅、32 条有界输入/显示窗口、64 条请求重放缓存（另有 8MiB 总预算）、输入去重和控制权隔离。显示在途预算约 4MiB 门槛加一条有界消息；客户端乱序窗口/消息总量同样受限。
- FFI 只交付变化 cells；iOS/Android 使用显示时钟消费，按脏行更新。iOS 缓存字体/属性并合并 ASCII 同样式绘制，复杂字符保留按 cell 绘制；本地输入状态和画面分开，输入入队不等 ACK。
- 保留现有“文字输入框＋发送”语义、功能键、历史、字号与复制；没有提前引入预测回显或另一套终端模拟器。

## 验证

| 项目 | 结果 |
| --- | --- |
| `cargo +stable test --locked --workspace --exclude ai-terminal-bindgen` | 34 项测试通过 |
| `cargo +stable clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings` | 通过 |
| `cargo +stable fmt --all --check` | 通过 |
| `cargo +stable test --locked -p ai-terminal-remote --features webrtc` | 通过，含直连超时跨路径重试不重复执行 |
| `scripts/test-host-terminal.py target/debug/ai-terminal` | PTY 输入/输出、备用屏与宿主模式恢复通过 |
| `scripts/test-account-cli.py ...`，文件存储和 `--os-vault` 两种模式 | 隐藏密码登录、状态、设备列表、退出通过；未输出密码 |
| Rust iOS aarch64 真机库与 x86_64 模拟器库 | 构建通过 |
| iOS Xcode Debug 模拟器 App；`build-ios-probe.py` 真机链接 | 构建通过，真机产物未签名/未安装 |
| Android arm64-v8a / x86_64 Rust 库与 `assembleDebug` | 构建通过；没有连接 Android 设备 |
| iOS 模拟器账号登录→发现 Desktop→Noise→选会话→输入 | direct / relay 两条路径均观察到独立命令输出 |

关键新回归包括账号隔离、并发刷新只有一个成功、旧令牌失效、改密/管理员重置、错误设备/过期连接授权、登录限流、握手身份/授权绑定、1,000 字符不丢不重、活动直连撤销后 Shell 保留、流式乱序/重复请求与控制权抢占、切账号不能接管旧 Shell、晚到旧会话更新被忽略、FFI 脏区累计。

原始构建/测试日志、截图、数值 JSON 在 `build/account-input/`。该目录不提交；源码备份为 `source-before.tar.gz`，不包含凭据目录。此工作目录本来没有 Git 元数据，未创建提交或远程 PR。

## 绘制性能

同一 macOS x86_64 主机、iPhone SE (3rd generation) iOS 17.5 模拟器、Debug 构建、同一 fixture、scale=1 离屏全屏绘制。预热 10 次后测 100 次，单位 ms。包含 UIGraphicsImageRenderer 工作；不等于完整 App 帧耗时或真机指标。

| 屏幕 | 改造前 p50 / p95 / p99 | 仅缓存 p95 | 合并绘制后 p50 / p95 / p99 |
| --- | --- | --- | --- |
| 80×24 | 117.23 / 130.80 / 133.97 | 33.89 | 3.74 / 5.17 / 5.96 |
| 120×40 | 297.80 / 319.73 / 326.03 | 81.72 | 8.31 / 10.92 / 11.55 |

另外完成 10 秒 Instruments Time Profiler 采样（`terminal-render.trace`），并将每档测量增加至 2,000 次；两档 p95 为 5.68ms / 11.96ms，p99 为 6.68ms / 13.15ms。采样对象为同一 Debug 离屏绘制探针；导出 11,428 条采样行，确认包含 TerminalView.draw/TerminalBenchmark 栈。

全屏负载 p95 已低于 16.7ms 目标；普通输入只更新脏行，无变化的本地输入编辑不触发终端重绘。效果来自消除重复字体度量、属性创建与大量逐字符绘制，不是缩短网络轮询。

## 输入到状态副本的延迟

`input_latency` 使用真实 Agent/PTY/Noise/传输/移动 Rust core，在回环上加独立有界 TCP 延迟代理；每组 1,000 个字符，最多每 10ms 发出一个字符。每个字符从成功入队前计时到副本观察到该字符；不包含原生屏幕呈现。旧版发送会阻塞，因此同样负载完成时间显著更长。旧版读取频率高于原 App 的 50ms 轮询，属于偏向旧版的对照，不能当作原 App 的实际手感测量。

| 80×24 / 空闲 | p50 | p95 | p99 | 1,000 字符总时间 |
| --- | ---: | ---: | ---: | ---: |
| 旧版、本机 direct | 21.04ms | 27.96ms | 28.82ms | 26.49s |
| 新版、本机 direct | 11.41ms | 14.92ms | 16.76ms | 11.04s |
| 旧版、relay 注入 RTT 80ms | 179.99ms | 183.47ms | 187.62ms | 181.44s |
| 新版、relay 注入 RTT 20ms | 34.40ms | 37.89ms | 39.82ms | 10.94s |
| 新版、relay 注入 RTT 80ms | 93.51ms | 96.25ms | 97.76ms | 10.86s |
| 新版、relay 注入 RTT 150ms | 162.06ms | 165.40ms | 166.66ms | 11.19s |

上述窗口优化后的低丢包模拟场景均满足“注入 RTT + 40ms”目标。四段 TCP 延迟取整数，因此 150ms 配置理论注入为 148ms；实际还有调度、代理和应用处理开销，不能将配置值当作精确实测网络 RTT。

120×40、RTT 80ms、三种负载各 1,000 次输入：空闲/30Hz 持续输出/每 100ms 历史查询的 p95 分别为 **112.70 / 113.43 / 112.81ms**，无丢失或重复。并行运行这些负载并伴有构建活动，因此数值包含竞争，不做微小差异排序。

独立资源采样的持续输出场景 p95 111.96ms；`/usr/bin/time -l` 报告整个探针运行 13.02s、user 14.24s、sys 0.81s、最大 RSS 61,464,576 bytes。此探针包含回环 Server、移动 core、延迟代理，并另起 Agent，数字不能作为单独 Server 的 RSS；Debug 构建也不代表发布包资源占用。

## 停顿与限制

另外运行每 100 个 TCP 读块增加 150ms 有序停顿的场景。它模拟可靠链路的重传等待，**不是操作系统层 1% 丢包**。在 RTT 150ms 的最终 32 输入窗口/仅直连重试实现中，1,000 字符完整到达，p50/p95/p99 为 **220.20 / 417.07 / 456.68ms**，总计 11.23s。停顿仍会增大尾延迟，没有宣称弱网完全无卡顿。

真实公网/NAT/TURN、1% UDP 丢包、移动省电/温升与低端真机的完整矩阵未在本环境完成。没有通过调整系统路由或关闭 TLS 校验制造测试结果。真实设备指导放在 HANDOFF，不把未测范围标为通过。
