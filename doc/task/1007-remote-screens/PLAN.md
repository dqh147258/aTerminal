# 远程屏幕查看

状态：Completed。用户已授权实现，并要求使用 worktree-tasks、sol 模型、最多 3 个子任务；随后明确要求无需等待计划 Review，直接执行。

原始工作目录 `/Volumes/Code/My/aTerminal`，集成分支 `main`，起点 `a3a61cfd4b7b66e19d4dc2003f394d8a9b7d4c19`，起点工作区干净。
管理任务：`20261007-163526-008-remote-screens`。

Android 和 iOS 主页右侧增加“远程屏幕”按钮；打开当前连接电脑的全部显示器列表，选择后持续刷新查看，支持返回列表与关闭。枚举、权限拒绝、无显示器、旧版 Desktop 和断线须呈现可理解的状态。仅查看屏幕，不增加鼠标、键盘远程控制。

用户已选择“当前连接电脑的全部显示器”。Android/iOS 两端均已实现并集成到原始 main。

参考 Sirix 的 `desktop-server/src/app/tasks/screen_state.rs`。采集在 Desktop 执行，复用 aTerminal 现有账号/配对及加密 RPC，不引入 Sirix 服务器。查看页按需约每秒请求一张 JPEG，只有前台可见页面刷新；不积压请求，关闭或后台停止。帧缩放、图像大小、超时和捕获并发须受限，避免影响终端交互。

三个子任务分别拥有 Rust 协议/Desktop/mobile-core、Android、iOS。协调者集成、审查与运行最终验证。子任务使用 `gpt-6.1-sol`；核心协议与采集用 high，平台界面用 xhigh。

| 子任务 | 工作目录/分支 | iTerm SessionID |
| --- | --- | --- |
| `20261007-163737-366-core` | `/Volumes/Code/public-worktree/aTerminal/1007-remote-screens/core` / `worktree/1007-remote-screens/core` | `w2t0p3:6DC99C92-1CB9-4B40-96D6-2D0F6EEA0760` |
| `20261007-163807-782-android` | `/Volumes/Code/public-worktree/aTerminal/1007-remote-screens/android` / `worktree/1007-remote-screens/android` | `w2t0p5:3886EC0D-A307-4CBA-B82A-223B9A96E055` |
| `20261007-163844-120-ios` | `/Volumes/Code/public-worktree/aTerminal/1007-remote-screens/ios` / `worktree/1007-remote-screens/ios` | `w2t0p7:EC79E698-C675-4E3A-94E6-67C42A159B37` |

核心实施约束已确认：屏幕请求使用与历史读取分离的异步任务；List 增加兼容的能力字段，旧 Desktop 不会收到未知 Operation；单帧 JPEG 不超过 120 KiB，尺寸自适应缩放；捕获全局单并发，等待有超时，超时不提前释放仍在运行的采集任务占用。

跨任务接口：`RemoteTerminal.remote_screens_json() -> Result<String>` 与 `remote_screen_frame_json(screen_id: String, max_width: u32) -> Result<String>`。绑定为 `remoteScreensJson()`、`remoteScreenFrameJson(screenId:maxWidth:)`（Kotlin 对应命名）。列表 JSON 为 `{ "screens": [{ "id", "name", "width", "height", "is_primary" }] }`；帧 JSON 为 `{ "screen_id", "mime_type": "image/jpeg", "image_base64", "width", "height", "captured_at_ms" }`，失败使用现有 CoreError。字段名为 snake_case。

测试强度 Medium：必要验收是 Rust 协议/授权/帧边界与异常测试、Android Debug 构建、iOS Simulator 构建，以及完整任务差异审查。根据可用环境补充隔离 UI 测试和本机真实采集。暂不安排物理手机全自动测试：尚未确认完整设备与服务条件；模拟器和本机检查不替代物理设备结论。macOS 录屏授权可能需要本机用户授权，其影响须如实记录。

交付须集成至原始 main，记录全任务审查与修正，提交任务代码及记录，验证最终集成状态。清理只在所有子任务完成、提交全部集成、测试通过及工作区干净后进行。

实施和 Medium 验收已完成：Rust 91 项、Android JVM 15 项及原生/重连 13 项通过，iOS 模型检查与 2 项真实 Simulator UI 流程通过；Android 三种 ABI、iOS Simulator/设备原生库和 App 已构建。本机实际枚举 4 个显示器并采集主屏 JPEG。测试包与正常 Android App 隔离，未更新正常 App 的安装或账号数据。

完整审查、修正和验证见 [REVIEW_FIXES.md](REVIEW_FIXES.md)。原生库/绑定及日志保留在原始 checkout 的 build；最终测试提交记入工作树任务完成记录与交接。未做物理手机、Linux/Windows 实际桌面采集验收；Linux 当前仅支持 X11。
