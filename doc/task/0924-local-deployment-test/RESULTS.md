# Android x86 真实本地部署验收

2026-09-24 在 ADB 设备 `127.0.0.1:62001`（Android API 25、`x86`）完成。设备没有 ADB 反向映射；App 的服务器地址为 `https://192.168.0.36:7200`，经主机 LAN TLS 代理访问从当前源码构建的 Docker Server。Desktop Agent 登录同一 Server 并提供两条真实 PTY Shell（120/80 列）。测试使用独立 Compose 项目、账号、Agent 状态和数据卷，没有使用内存服务或模型桩。

本地证书由 Debug 验收模式从 App 私有目录的 `acceptance-ca.pem` 读取，交给 Rust TLS 客户端验证证书链和 `192.168.0.36` 主机名；没有关闭证书校验。普通 Android 登录页当前没有私有 CA 导入入口，正式使用自签 CA 的本地服务仍需先补该能力，或改用 Rust TLS 默认根证书信任的证书。验收结束已移除 App 私有 CA 文件。

## 结果

| 检查 | 结果 |
| --- | --- |
| x86 构建 | `build-mobile.py android --android-abi x86`、`prepare-bindings.py` 通过；APK `minSdk=25`，含 `arm64-v8a`、`x86_64`、`x86` |
| Android 构建 | debug APK、测试 APK、`lintDebug` 通过；设备 `install -r` 成功 |
| 服务 | 当前源码构建 Docker Server；LAN `/healthz`、`/admin/` 与管理员概览经 HTTPS/CA 验证均返回 200 |
| 设备状态/界面 | 运行 16 项仪器测试：15 项通过，API 29 专属 RenderNode 项按条件跳过 |
| 真实端到端工作流 | `MobileWorkflowTest.realTerminalWorkflowAndPlaceholder`：1 项通过，0 失败、0 跳过，结果 `passed=true` |

真实工作流覆盖登录、连接 Desktop、PTY 命令与独立输出、120 列横滑、字号实时预览和局部浮窗、侧滑抽屉、AI 不可交互占位、后台断开与最后会话恢复、Ctrl-C 后继续输入、关闭目标会话后连接仍可列出另一会话，以及退出登录。报告的终端数据路径为 `relay`；设备至本地服务的控制连接直接使用局域网 HTTPS。此结果不代表公网、蜂窝网络、真实模型或语音功能验收。

完整设备日志：`build/local-deployment-android-ui.log`、`build/local-deployment-android-lan-workflow.log`；结果及截图：`build/local-deployment-android-lan/`。这些构建产物被 Git 忽略，截图已检查终端与占位浮窗。第一次经 `adb reverse` 的辅助验证保存在 `build/local-deployment-android/`，不作为局域网直连结论。

## 清理

测试 App 已退出登录并停止。已删除本次设备私有凭据/CA、测试导入的公共 CA 文件和 ADB 反向映射；专用 Desktop Shell、Agent 与账号已关闭；`aiterminal-e2e` Compose 容器、网络和专用数据卷已删除，私有测试目录已清理。原 `ai-terminal-dev` 项目的容器、数据库卷和配置未改动，仍处于此前的停止状态；x86 开发 APK 和测试 APK 留在设备上。
