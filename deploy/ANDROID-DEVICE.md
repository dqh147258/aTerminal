# Android 真机调试

## 常用测试设备

用户于 2026-09-22 确认这是其常用 Android 测试设备：

- 型号：M2012K10C（ares），Android 12，arm64-v8a。
- 本次 ADB 序列号：`dmronjvo9pwsbinf`。每次运行前使用 `adb devices -l` 确认实际连接，命令显式选择设备。
- 屏幕：1080×2400，密度 440dpi。
- **一段时间不操作会自动锁屏，没有密码。按电源键点亮，再向上滑动即可解锁。**
- 不需要修改锁屏超时、关闭系统锁屏或设置密码。自动化中先检查亮屏/锁屏状态，只在需要时唤醒并上划。

```sh
adb devices -l
adb -s dmronjvo9pwsbinf shell input keyevent KEYCODE_WAKEUP
adb -s dmronjvo9pwsbinf shell input swipe 540 2000 540 500 350
```

`KEYCODE_WAKEUP` 用于自动化时仅唤醒，避免盲目按电源键把已经亮着的屏幕再次关闭。手动操作按电源键后上划即可。坐标基于本次竖屏分辨率；横屏或设备变化时先查询 `wm size`。

## x86 本地部署测试设备

2026-09-24 使用 ADB 设备 `127.0.0.1:62001`：Android API 25、`x86`，设备列表显示为 `SM_G930K`。测试命令必须显式指定这个序列号；它与上述 arm64 测试设备是不同目标。APK 需包含 x86 原生库，安装使用 `-r` 保留现有应用数据。

```sh
adb -s 127.0.0.1:62001 install -r apps/android/app/build/outputs/apk/debug/app-debug.apk
adb -s 127.0.0.1:62001 shell am start -W -n dev.aiterminal.app/.MainActivity
```

真实本地部署验收使用 `https://192.168.0.36:7200`，设备直接走局域网，不设置 `adb reverse`。本机部署采用私有 CA；Debug 验收模式（`--ez acceptance_test true`）可从 App 私有目录的 `files/acceptance-ca.pem` 读取 CA，由 Rust TLS 正常验证链和 IP 主机名。测试后删除该文件。普通登录路径不读取此文件；正式使用私有 CA 的服务需要另行提供可信证书导入能力，或部署 Rust TLS 默认根证书信任的证书。完整结果见 `doc/task/0924-local-deployment-test/RESULTS.md`。

```sh
adb -s 127.0.0.1:62001 shell run-as dev.aiterminal.app mkdir -p files
adb -s 127.0.0.1:62001 shell "run-as dev.aiterminal.app sh -c 'cat > files/acceptance-ca.pem'" < deploy/secrets/lan-ca.crt
adb -s 127.0.0.1:62001 shell am start -W -n dev.aiterminal.app/.MainActivity --ez acceptance_test true
# 验收完成后：
adb -s 127.0.0.1:62001 shell run-as dev.aiterminal.app rm -f files/acceptance-ca.pem
```

## 当前开发包

```sh
adb -s dmronjvo9pwsbinf install -r apps/android/app/build/outputs/apk/debug/app-debug.apk
adb -s dmronjvo9pwsbinf shell am start -W -n dev.aiterminal.app/.MainActivity
```

使用 `-r` 更新，保留 App 数据；不要为了重测直接 `pm clear`、卸载应用或修改手机其他应用的数据。若签名冲突，先确认旧版本用途。

**操作前核对前台包名。** 用户曾指出坐标操作误开其他应用并手动返回桌面。禁止在测试超时、退出、自动锁屏或用户切换页面后继续沿用旧坐标。启动目标必须使用 `dev.aiterminal.app/.MainActivity`；检查 `dumpsys window` 的 `mFocusedApp` / `mCurrentFocus`，确认 AI Terminal 活跃且有焦点后再输入。软键盘弹出时还需确认输入目标仍是本应用。

测试失败或失去前台时立即停止输入注入，不自动点击桌面图标寻找应用。当前 instrumentation 在按钮操作和按键注入前检查本应用焦点；不再保留等待外部坐标点击的 keyboard 模式。

账号初始化与 Desktop 命令见 [ACCOUNTS.md](ACCOUNTS.md)。真机测试报告和测试入口会记录在 `doc/task/0922-account-input/ANDROID-DEVICE.md`。

## 测试网络口径

调试可用 `adb reverse tcp:PORT tcp:PORT` 将手机回环控制连接映射到主机隔离服务，测试结束只删除本次添加的映射。此方式的账号/中转链路走 USB；若状态为 WebRTC direct，终端数据可以另走 Wi-Fi 直连。必须分别记录路径，不能把 USB 中转的结果称为真实公网 WSS 或蜂窝性能。

记录帧统计、App 内采样和当前应用的日志；测试密码写入受限测试文件，不放在报告、日志或命令行明文。测试完成退出临时账号，保留用户需要的开发包。

## 可重复的真机验收

测试代码位于 `apps/android/app/src/androidTest/.../DeviceAcceptanceTest.kt`，仅进入测试 APK，不进入用户 APK。测试读取 App 私有目录 `files/device-fixture.json`，密码不写入 APK、报告或命令参数。夹具由 `account_demo STATE CLI --bench` 创建，包含独立账号、Shell 和三种负载会话；不能把生产账号作为这个自动化夹具。

测试覆盖真实登录按钮、会话选择、接管输入、按键事件、InputConnection 中文/Emoji 组合提交、Ctrl-C、各 1,000 字符回显、历史并发读取、Home 后恢复、中转与退出。`mode=smoke` 只运行功能流程。每次完整压力测试使用新的夹具，避免已有输出影响字符计数。

MIUI 上测试进程从后台调用 `startActivitySync` 可能超时。当前测试通过 UiAutomation 的 `am start` 拉起真实 Activity 后，使用生命周期监视器取得实例，不修改系统后台启动权限。

```sh
./apps/android/gradlew -p apps/android :app:assembleDebug :app:assembleDebugAndroidTest
adb -s dmronjvo9pwsbinf install -r apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb -s dmronjvo9pwsbinf shell am instrument -w -r -e class dev.aiterminal.app.DeviceAcceptanceTest dev.aiterminal.app.test/androidx.test.runner.AndroidJUnitRunner
```

编译原生性能版本使用 `scripts/build-mobile.py android --release --webrtc --ndk PATH`，随后重新执行 `prepare-bindings.py` 和 App 构建，以便打入最新 `.so`。保持可调试 App 方便采样，明确记录原生库是 Debug 还是 Release，不能混用两个构建的结果。

采样口径：`input_to_onDraw` 从客户端输入入队前，到已更新的原生视图进入绘制回调；不等于像素扫描到屏幕的时间。`window_frame_total` 是 Android FrameMetrics 的整个窗口帧时间，包含测试回调开销。`native_editor_event` 是注入真实键事件到 EditText 内容变化；InputConnection 验证不等同于用户手动操作搜狗候选栏。

续作新增 `scripts/run-android-acceptance.py`：传入 `--serial`、隔离夹具文件 `--fixture` 和 `--output`，自动安装本项目 App/测试包、检查前台、私有传入凭据、执行测试、收集数据并移除本次 ADB 转发。`--input-interval-ms 20` 用于目标固定节拍对照；失败时停止本应用，不继续注入输入。夹具服务需事先启动，完成后正常关闭。

显示一致性测试可单独执行，无需账号夹具：

```sh
adb -s dmronjvo9pwsbinf shell am instrument -w -r -e class dev.aiterminal.app.DisplayConsistencyTest dev.aiterminal.app.test/androidx.test.runner.AndroidJUnitRunner
```

最新数据和未达标项目见 [一致性优先的续作报告](../doc/task/0922-account-input/FOLLOWUP.md)。
