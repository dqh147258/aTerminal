# 登录地址构建配置

2026-09-26，用户追加要求：本地包内置本地 Server 地址，登录页只展示地址，以小字入口修改，随后代为登录已授权的 Android 模拟器。

## 行为

Android/iOS 登录页默认显示“登录地址”和地址文本，小字“修改服务器地址”展开编辑；“保存服务器地址”校验后收起并持久化。手动保存的地址优先于构建默认地址，不覆盖已登录账号的会话地址。

本地 Debug 包默认地址为 `https://192.168.0.36:7200`。已有公共开发 CA 用于正常 HTTPS 登录，不关闭证书或主机名校验。凭据不进入构建产物。

## 构建

统一脚本支持覆盖两端默认值（会构建原生依赖）：

```sh
python3 scripts/build-artifacts.py android ios \
  --server-url https://192.168.0.36:7200 \
  --server-ca deploy/secrets/lan-ca.crt
```

省略参数时默认本地地址，且在证书文件存在时使用本地 CA。还支持环境变量 `ATERMINAL_SERVER_URL` / `ATERMINAL_SERVER_CA_FILE`。

仅重新构建 Android UI（原生依赖已准备）：

```sh
./apps/android/gradlew -p apps/android \
  :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug --offline \
  -PterminalServerUrl=https://192.168.0.36:7200
```

Android 也支持 `-PterminalServerCaFile=/absolute/path/to/public-ca.pem`；直接构建 Release 时没有默认本地地址，需通过参数配置。iOS 直接 xcodebuild 使用 `ATERMINAL_SERVER_URL` 和 `ATERMINAL_SERVER_CA_BASE64`；统一脚本负责读取 CA 文件并编码。iOS 的 `Info.plist` 模板将构建设置写入真实产物。

## 验证与登录

- Android Debug/测试 APK/lint 通过。六阶段跨进程登录回归全部通过，新增检查包括：默认地址来自 BuildConfig、默认无地址输入框、修改保存后收起、下次登录恢复手动地址。报告：`.local/emulator-5586-server-address/results.json`。
- iOS `build-for-testing` 通过，App 与更新后的 UI 测试包可编译；独立读取最终 App 的 Info.plist，确认地址和公共 CA 与输入一致。本轮未运行 iOS UI 测试。
- 实际本地登录及另一个进程恢复 **2/2 通过**，账号 `aiterminal_local_test`，设备身份一致；普通 launcher 冷启动后仍已登录，App 保持打开，当前有 0 台在线 Desktop。报告与截图位于 `.local/emulator-5586-local-login/`。登录页截图另用 `isolated_ui` 复核后恢复正常登录界面。
- 本地 Server 健康检查通过，复用已有 `.local/local-dev/account.json`，不重建或停止 Server/Desktop。

按需复跑实际本地登录（会将指定设备的正常 App 登录到已有本地账号，不是隔离账号测试）：

```sh
PATH=/Users/carl/Library/Android/sdk/platform-tools:$PATH \
  python3 scripts/login-android-local.py --serial emulator-5586 \
  --output .local/emulator-5586-local-login
```

该脚本通过普通登录页提交账号密码，验证另一进程不输入密码也能恢复同一设备身份，然后以普通 launcher 参数启动 App 并保持登录。遇到已登录的其他账号或服务器会停止，不自动退出替换；临时密码输入文件完成后删除。
