# 移动端登录与持久化

2026-09-26，用户追加需求并授权在 `emulator-5586` 验证。

## 行为与实现

- 登录成功后自动保存 AccountSession（设备身份、访问/刷新令牌），后续启动从本地恢复，不要求每次输入密码。Android 使用 Keystore AES-GCM 加密 SharedPreferences，iOS 使用 Keychain；不保存原始密码。
- 临时断网不会清除登录态；恢复身份与成功连接 Desktop 是两个独立状态。
- 修复两端先等待网络注销、后清除本地凭据的问题。现在先同步清除持久凭据，再发送网络注销；即使立即关闭进程，也不会恢复已退出的登录。
- 保存会话时检查账号代际，锁只覆盖本地存储，不覆盖网络调用。旧请求不能在登出后重新保存凭据。Android 销毁 Activity 时使旧保存失效，设备刷新/心跳捕获发起时的代际；iOS 设备列表身份回调也检查代际。
- Android 启动恢复期间阻止并发提交登录，登出重置登录按钮；旧登录请求完成不会重置新登录的表单状态。
- iOS 区分 Keychain 无记录与读取失败，读取失败会显示错误，不再默认为没有保存过账号。

## 自动化验证

使用临时服务、账号和真实 Desktop PTY。测试使用 `acceptance-account` 命名空间；各阶段由外部 runner 执行 `am force-stop` 后启动新的仪器测试进程，并断言 PID 各不相同。

| 阶段 | 验证 |
| --- | --- |
| login | 通过登录表单登录、加密保存、连接真实 Desktop，持久会话不含原始密码 |
| restored | 新进程不填写表单，恢复相同设备身份并连接 Desktop |
| offline | 断开该临时服务的 adb reverse，等设备读取失败后，仍保留身份与保存的会话 |
| logout | 阻塞 worker，使网络注销尚未执行；通过实际退出按钮与系统确认框退出，本地凭据立即消失；模拟旧心跳完成保存，仍不能恢复凭据 |
| signedout | 强制结束上一进程后启动，仍是登录页，无保存的会话 |
| relogin | 再次通过表单登录成功，建立新的设备会话 |

最终六阶段 **6/6 通过**，原有 AutoConnectTest **1/1 通过**。原始结果保存在 `.local/emulator-5586-login/results.json` 与 `.local/emulator-5586-autoconnect-login/results.json`。

主账号 `account.xml` 与连接偏好 `connection.xml` 前后摘要一致。测试结束清理临时账号凭据、服务、PTY 和 adb reverse，保留主账号数据，不执行 `pm clear`。

复跑（需要已构建的本地 Desktop/Server 及 Android APK）：

```sh
./apps/android/gradlew -p apps/android \
  :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug --offline
PATH=/Users/carl/Library/Android/sdk/platform-tools:$PATH \
  python3 scripts/test-android-autoconnect.py --serial emulator-5586 \
  --login-persistence --output .local/emulator-5586-login
```

另以原有 AutoConnectTest 回归 Activity 重建、自动连接最新终端、刷新不改变手动选择：

```sh
PATH=/Users/carl/Library/Android/sdk/platform-tools:$PATH \
  python3 scripts/test-android-autoconnect.py --serial emulator-5586 \
  --output .local/emulator-5586-autoconnect-login
```

Android assembleDebug、assembleDebugAndroidTest、lintDebug 通过。iOS 使用 `xcodebuild -project apps/ios/aTerminal.xcodeproj -scheme aTerminal -sdk iphonesimulator -configuration Debug -derivedDataPath build/xcode ARCHS=x86_64 CODE_SIGN_IDENTITY=- build` 编译通过；本轮未运行 iOS 登录生命周期仪器测试，不把编译结果等同运行验证。

本轮未改 Rust/FFI，因此不重复前一阶段已通过的 Rust 全量测试。未扩大为服务端撤销账号、令牌轮换中途进程终止等专项测试。
