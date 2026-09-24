# Android x86 真设备复验

Nox `127.0.0.1:62001` 在本轮自动化期间发生 `system_server` WindowManager watchdog，之后来宾内核 panic 循环；原 VM 磁盘保留，VM 目前关闭。Android SDK 的 API33/x86_64 新建和既有 AVD 均停在 `adb offline`，测试进程已停止。Android APK/Test APK 与 lint 通过，Nox 崩溃前 17 项界面/绘制/状态测试通过，但 Desktop 脱离/重附着的 Android 真服务流程尚未验收。

设备恢复后，用 `adb devices -l` 确认 `device`，安装 `apps/android/app/build/outputs/apk/debug/app-debug.apk`，通过本地私有 CA 连接 `https://192.168.0.36:7200`，选择已附着的新版 Desktop 专用 Shell。手机输入并观察真实输出；桌面按 Ctrl+] 脱离，确认手机保留画面/历史、不能输入；桌面 `--attach SESSION_ID`，确认手机无须重新选择会话即可继续输入，Tab 和回车仍由 Shell 处理。测试只用新建 Shell，不关闭当前默认 Agent 的两条用户会话。详见 [本地调试说明](../../../deploy/LOCAL-DEBUG.md)。
