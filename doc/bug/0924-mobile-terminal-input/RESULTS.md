# 移动端终端直输与会话同步结果

Android/iOS 现由终端画面取得键盘焦点，已确认的文字直接进入当前 Desktop PTY；系统键盘 Return 执行回车，Tab、退格、Esc、方向键和 Ctrl-C 可通过特殊键工具栏或硬键盘发送。独立可见的终端草稿框与“发送”按钮已移除，AI 浮窗仍为不可交互占位。两端保留显式“接管输入”开关；未接管时点击终端不会写入 Shell。

工作空间打开时立即查询 Desktop 会话，打开期间每 3 秒更新一次。Android 列表只重绘条目，不清空搜索框或抢占当前会话；iOS 轮询不阻塞终端界面。iOS 还修复了“上次会话已由 Desktop 关闭，但仍有其它运行会话”时停在空终端的问题，此时恢复一条在线会话。

| 验证 | 结果 |
| --- | --- |
| Android API 25/x86 | debug APK、测试 APK、lint 通过；16 项状态/界面测试中 15 项通过，API 29 专属项按条件跳过 |
| Android 真实本地服务 | `MobileWorkflowTest` 1 项通过、`passed=true`；终端直输、系统键盘 Enter、真实 Zsh 的 Tab 路径补全、120 列横滑、后台恢复、Ctrl-C、关闭会话后继续连接均通过 |
| Android 外部会话变化 | App 保持连接时从 Desktop 新建专用会话，打开抽屉无需手动刷新即显示；Desktop 关闭后条目自动移除，截图已核对 |
| iOS iPhone 15 模拟器 | 4 项布局/状态 UI 测试通过；真实本地服务完整 UI 测试 1 项通过，包含系统键盘 Return、Tab 补全、80/120 列切换、画面/恢复、外部会话新增及关闭 |
| iOS Release | x86_64 模拟器构建通过 |

验收连接的是 `https://192.168.0.36:7200` 的本地 Docker Server、真实 Desktop Agent 和独立 PTY，不使用模型桩；Android 设备无 `adb reverse`，最终终端数据路径记录为 `relay`。Android 的 `terminal_input_test`、iOS 的 `--service-test` 使用独立的账号/偏好存储及显式本地 CA，普通用户数据未清除。Android 长文本、中文/Emoji 提交前组合与特殊键映射由专门仪器测试覆盖；iOS 使用原生文本输入代理，仅在提交候选后发送。

本地证据在被 Git 忽略的 `build/` 下：`mobile-terminal-android-return.log`、`mobile-terminal-android-return-results.json`、`mobile-terminal-android-ui-regression-retry.log`、`mobile-terminal-android-ime-final.log`、`mobile-terminal-session-auto-refresh.png`、`mobile-terminal-session-auto-remove.png`、`mobile-terminal-ios-return-retry.xcresult`、`mobile-terminal-ios-ui.xcresult`、`mobile-terminal-ios-release-final.log`。

所有本次专用 Shell、测试凭据文件和临时目录已清理；Desktop 原有的 `05c5caa5`、`673fa9d7` 两条会话与本地 Server 继续运行，Android 调试 App 已恢复到原 `acceptance_test` 工作区。自签本地 CA 仍只在 Debug 验收路径受信，普通登录页的私有 CA 导入不属于本次修复。
