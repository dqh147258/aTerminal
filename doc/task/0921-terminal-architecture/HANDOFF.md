# 当前交付与发布前验证

- Status: Manual verification
- Updated: 2026-09-22

## 当前可运行结果

本地 CLI/后台 Agent、版本化屏幕、独占输入控制、配对、Noise/WSS、中转撤销、WebRTC 直连及超时切换均已实现。Rust 核心跨进程 E2E 与直连故障测试通过。Android APK 与 iOS XCFramework/Xcode App 已构建。

2026-09-22 按用户要求配置 `https://192.168.0.36:7200` 局域网 HTTPS/WSS，回环健康检查 `http://127.0.0.1:7201/healthz`。测试 CA 通过配对显式信任，不关闭证书校验；iOS 模拟器直连和中转均已测通。随后用户要求全部关闭，容器、Agent/Shell、模拟器均已停止；证书、配对、数据库和构建产物保留。重新启动会创建新的 Shell，会话 ID 不再沿用。见 [本地调试说明](../../../deploy/IOS-LAN-TRIAL.md)。

iPhone SE iOS 17.5 模拟器已完成真实配对、Keychain 保存/恢复、WebRTC 直连读屏和真实 Shell 输入；截图 `build/screenshots/ios-se-live.png`、`build/screenshots/ios-se-input.png`。测试后清理隔离演示会话和配对，Server 保留运行。Android API 33 模拟器两次启动均保持 adb offline，已停止本任务进程；APK 编译通过不能替代设备运行验证。

## 发布前仍需完成

1. Windows 11 + Windows Terminal + PowerShell 7：运行 CI 测试，并检查 PSReadLine、多行编辑、补全、Ctrl-C、resize、ConPTY 关闭。当前无 Windows 环境；已按用户“继续”指令延后此项，未标记通过。
2. Android 真机：连接配对、Keystore 恢复、中文 IME、外接键盘、滚动/缩放、后台/前台恢复。预期后台不终止桌面 Shell，前台重新连接后画面追上且旧输入不重放。
3. iOS 真机：在 Xcode 指定签名团队后运行，检查上述输入/生命周期及 Keychain 行为。当前只做模拟器，未安装到真机。
4. 两个不同网络：STUN/TURN、UDP 阻断、Wi-Fi/蜂窝切换、抖动/丢包、服务器撤销。预期转中转时不重建 Shell、不重复输入、不回退 revision；无可用链路时明确离线。
5. 性能：低配 1 vCPU 服务器至少 30 分钟资源曲线、本地附加延迟、120×40 移动帧耗时。当前仅有 20 对连接 30 秒中转负载，不包含公网 TLS/TURN。
6. 发布：精确锁定工具链的五平台构建、Apple/Android 签名、证书续期、完整许可证 notices/SBOM、短时 TURN 凭据、无障碍和细粒度文本选择。

这些用户所有的运行检查不混入自动化 TODO。自动化命令与已通过证据见 README.md、TODO.md。整体计划仍是 In progress，没有宣称第一版全部完成或可直接公开发布。

## 重要边界

同一会话一个尺寸；手机通过平移/字号查看，不自行换行重排交互屏幕。跨字体像素一致不在合同内。终端能力仍以已测文本协议为界，私有图像协议和完整 terminfo 兼容矩阵尚需发布审查。

配对邀请含能力密钥，应只导入目标设备。只读权限在 Agent 强制执行。Windows 本机凭据依赖当前用户私有临时目录的 ACL，需 Windows 验证；不应把 state-dir 放共享目录。

直连仍保持 WSS 控制连接以处理授权撤销；控制连接消失会终止远程连接，本地会话继续。断线重连不自动重放结果未知的操作。AI 暂无实现，仅保留 AI-SPEC.md 定义。
