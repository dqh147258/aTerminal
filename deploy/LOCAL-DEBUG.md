# 本机快速启动、调试与测试

本文适用于这台 Mac 的局域网部署：主机 `192.168.0.36`，Android x86 测试设备 `127.0.0.1:62001`。Server、Admin、Desktop Agent 和手机使用真实进程与真实 PTY；移动端 AI 对话和语音输入当前仍是不可交互占位。需要 Docker Desktop、Rust 1.94.1；重建移动端还需要 Xcode、Android SDK/NDK r28+ 和 JDK 17。

## 1. 启动 Server

在项目根目录运行：

```sh
cd /Volumes/Code/My/AITerminal
python3 scripts/prepare-lan.py
python3 scripts/local-dev-account.py ensure
curl --fail --noproxy '*' --cacert deploy/secrets/lan-ca.crt https://192.168.0.36:7200/healthz
```

`prepare-lan.py` 启动 `ai-terminal-dev` 的 Server 和 LAN TLS 容器，沿用现有数据库、证书与管理员令牌，不删除数据。正常情况下健康检查输出 `ok`。若修改过 `crates/server` 或 Admin 页面，先运行 `docker compose -p ai-terminal-dev -f deploy/compose.local.yaml build server` 更新 `ai-terminal-server:development` 镜像，再执行上述启动命令。初次 Docker 构建可能较久，日常重启无需重建。

| 用途 | 入口或账号 |
| --- | --- |
| App / Desktop 的服务地址 | `https://192.168.0.36:7200` |
| Mac 本机 Admin 页面 | `http://127.0.0.1:7201/admin/` |
| Admin 登录 | **没有用户名和密码**，只需管理员令牌；在本机运行 `cat deploy/secrets/admin-token` 后粘贴到页面的“管理员令牌”字段 |
| 测试用户 | 用户名 `aiterminal_local_test`；运行 `python3 scripts/local-dev-account.py show` 查看密码 |

测试用户的随机密码保存在被 Git 忽略的 `.local/local-dev/account.json`（目录权限 0700、文件权限 0600），不会写入本文或提交到仓库。`ensure` 可重复执行：账号和本地密码文件都存在时直接复用；如果服务中已有同名用户而本地密码文件丢失，它会停止并提示人工核对，不会擅自重置密码。**Admin 令牌不能当作测试用户密码，也不要输入到手机。**

## 2. 启动 Desktop Agent 和 Shell

先确认 Server 已健康，然后运行。`start-local-agent.sh` 用当前部署的私有凭据目录启动或检查 Agent；重启 Mac 后，应先运行它，再直接使用 `target/debug/ai-terminal`：

```sh
cargo +stable build --locked -p ai-terminal --bin ai-terminal
scripts/start-local-agent.sh
```

如果状态显示 `Not logged in`，再登录一次：

```sh
target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent" auth login \
  --server https://192.168.0.36:7200 \
  --username aiterminal_local_test --name 'Local Desktop' \
  --ca-file deploy/secrets/lan-ca.crt
```

密码在终端隐藏提示中输入，用上一节的 `show` 命令获取。当前本地部署的账号凭据存于 `.local/local-dev/config/ai-terminal/` 的私有文件中；`start-local-agent.sh` 显式选择该存储方式，避免无交互环境改用空的系统凭据库。Desktop 登录后 Agent 在后台运行；以下命令创建一条真实 Shell。按 **Ctrl+]** 脱离桌面画面时 Shell 仍在运行，但新版 Agent 会让手机转为只读；再次执行 `--attach SESSION_ID` 才恢复两端输入。某些终端把这个原始按键报告为 Ctrl+5，CLI 同样识别：

```sh
target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent"
target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent" --list
target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent" auth status
```

CLI 创建会话时使用当前终端窗口的行列数；测试 120 列横向滚动时，先把终端窗口调到至少 120 列。Agent、Shell 与用户登录分别是不同状态：关闭 CLI 窗口不等于关闭 Shell 或退出账号。`--list` 能确认会话仍为 `running`。

重新编译 `ai-terminal` 不会替换正在运行的 Agent。Shell 的 PTY 状态只在 Agent 进程内，不能无损迁移；有未完成会话时不要用 `--agent-stop` 升级。2026-09-24 已将报错的旧 Agent 切换为新版，旧会话最后画面和历史保存在私有目录 `.local/local-dev/legacy-session-archive-20260924/`；当前 `.local/local-dev/agent` 是可直接使用的默认入口。若以后需要先保留旧会话测试新版本，可用另一个私有 `--state-dir` 启动并在手机设备列表选择相应 Desktop。

只调试单机 Desktop 终端时，不必启动 Server 或登录账号：

```sh
cargo +stable run -p ai-terminal
cargo +stable run -p ai-terminal -- --cwd /path/to/project
cargo +stable run -p ai-terminal -- --list
cargo +stable run -p ai-terminal -- --attach SESSION_ID
cargo +stable run -p ai-terminal -- --attach SESSION_ID --watch
cargo +stable run -p ai-terminal -- --history SESSION_ID
cargo +stable run -p ai-terminal -- --close SESSION_ID
cargo +stable run -p ai-terminal -- --snapshot /tmp/screen.pb -- /bin/sh -c 'printf "hello\n"'
```

`--watch` 只读观察，不让手机恢复输入；`--attach` 回到同一 PTY、原目录和画面；`--close` 结束指定 Shell；`--snapshot` 无交互地捕获程序结束时的 Protobuf 屏幕。Shell 真正结束后，移动端可查看最后画面/已有滚动历史，但不能继续输入或复原进程状态。Windows 上默认优先 `pwsh.exe`，也可在 `--` 后显式传入 Shell。不能无损接管其他程序早已启动的 Shell 内存状态。

## 3. 在 Android x86 设备调试

先用 `adb devices -l` 确认 `127.0.0.1:62001` 在线。已有开发 APK 时直接安装；改过 Android 或 Rust 移动端代码时先按下文“重新构建与测试”重建 x86 原生库、bindings 和 APK。

```sh
adb -s 127.0.0.1:62001 install -r apps/android/app/build/outputs/apk/debug/app-debug.apk
adb -s 127.0.0.1:62001 shell run-as dev.aiterminal.app mkdir -p files
adb -s 127.0.0.1:62001 shell "run-as dev.aiterminal.app sh -c 'cat > files/acceptance-ca.pem'" < deploy/secrets/lan-ca.crt
adb -s 127.0.0.1:62001 shell am start -W -n dev.aiterminal.app/.MainActivity --ez acceptance_test true
adb -s 127.0.0.1:62001 reverse --list
```

反向映射列表应为空。App 登录页填服务地址 `https://192.168.0.36:7200`、测试用户名和 `show` 输出的密码；连接在线的 Desktop，选择已由桌面 CLI 附着的 Shell。有写权限的手机与 Desktop CLI 可同时输入并同步同一 PTY 画面，无需切换控制权。直接输入 `printf 'LOCAL_DEBUG_OK\n'` 并按回车，再执行 `git status`，确认仍能继续输入；Tab 应作用于当前 Shell 的补全，Ctrl-C 应中断前台命令。按 Ctrl+] 从桌面脱离后，手机保留画面与历史但停止输入；用 `--attach SESSION_ID` 恢复，确认手机也再次可输入。账号设备面板只展示在线设备，不自动撤销离线设备。`acceptance_test` 使用独立的 App 偏好与账号存储，不覆盖普通 App 登录数据。

当前默认 `.local/local-dev/agent` 仍有用户 Shell，属于上一版正在运行的 Agent；重新编译不会热更新该进程。新版 App/CLI 对它保持旧输入行为的兼容，完整的“桌面脱离后手机只读”规则需要在这些 Shell 不再使用后重启默认 Agent。可先用独立私有状态目录和新 Agent 测试；不要为升级而直接结束正在工作的 Shell。

这台机器的本地证书由私有 CA 签发。Debug 验收模式会读取上述 App 私有 CA 文件，让 Rust TLS 验证证书链及 IP 主机名；它**不会跳过 TLS 校验**。普通登录模式不读取该文件，当前 UI 也没有私有 CA 导入入口，因此不能把 Debug 测试通过解释为普通用户能直接登录这套自签证书服务。验收结束删除测试 CA：

```sh
adb -s 127.0.0.1:62001 shell run-as dev.aiterminal.app rm -f files/acceptance-ca.pem
```

## 4. 重新构建与测试

仅改 Server/Admin 时重建镜像后重新运行 `prepare-lan.py`。改动 Rust 移动内核或 Android 后，先装好目标 `aarch64-linux-android`、`x86_64-linux-android`、`i686-linux-android`，再运行：

```sh
python3 scripts/build-mobile.py android --toolchain stable --webrtc --ndk /path/to/android-sdk/ndk/28.2.13676358
python3 scripts/build-mobile.py android --toolchain stable --webrtc --android-abi x86_64 --ndk /path/to/android-sdk/ndk/28.2.13676358
python3 scripts/build-mobile.py android --toolchain stable --webrtc --android-abi x86 --ndk /path/to/android-sdk/ndk/28.2.13676358
python3 scripts/prepare-bindings.py --toolchain stable
./apps/android/gradlew -p apps/android :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug --offline
```

Android 最低支持 API 25，APK 应含 arm64-v8a、x86_64、x86。只验证当前 x86 设备时仍须确保其他 ABI 的已生成库存在，`prepare-bindings.py` 才能正确打包。iOS 模拟器构建在本机使用 x86_64；Apple Silicon 改用 `aarch64-apple-ios-sim` 和 `ARCHS=arm64`：

```sh
python3 scripts/build-mobile.py ios --toolchain stable --webrtc
python3 scripts/build-mobile.py ios --toolchain stable --webrtc --simulator
python3 scripts/prepare-bindings.py --toolchain stable
python3 scripts/package-ios.py --simulator-target x86_64-apple-ios --replace
xcodebuild -project apps/ios/AITerminal.xcodeproj -scheme AITerminal \
  -sdk iphonesimulator -configuration Debug -derivedDataPath build/xcode \
  ARCHS=x86_64 CODE_SIGN_IDENTITY=- build
```

正式 Xcode App 构建需要签名与 Keychain 元数据，`build-ios-probe.py` 不能替代它；iOS 模拟器交互步骤见 [iOS 局域网试用说明](IOS-LAN-TRIAL.md)。Rust 与本机终端回归：

```sh
cargo +stable test --locked --workspace --exclude ai-terminal-bindgen
cargo +stable clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings
python3 scripts/test-host-terminal.py target/debug/ai-terminal
```

## 5. 快速定位问题

```sh
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml ps -a
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml logs --tail 100 server lan_tls
tail -n 100 .local/local-dev/agent/agent.log
adb -s 127.0.0.1:62001 shell dumpsys window | rg 'mCurrentFocus|mFocusedApp'
adb -s 127.0.0.1:62001 logcat -d -s AndroidRuntime
```

输入或点击设备前，确认前台是 `dev.aiterminal.app/.MainActivity`；失去焦点后停止输入自动化。若看不到 Desktop，先检查 CLI `auth status`、`--list`，再确认 Server 健康。若登录报证书错误，检查 App 是否以 `--ez acceptance_test true` 启动、CA 文件是否存在、主机 IP 是否仍为 `192.168.0.36`。若修改 Server 后行为未变，重建镜像并重新运行 `prepare-lan.py`。不要用 `adb reverse` 把局域网问题掩盖成回环成功。

完整的 API 25/x86 真实本地部署自动化结果、截图和验证边界见 [验收报告](../doc/task/0924-local-deployment-test/RESULTS.md)。只检查设备状态/界面时可安装测试 APK，运行 `WorkspaceStateTest`、`DisplayConsistencyTest`、`WorkspaceUiTest`；需要账号的 `MobileWorkflowTest` 会关闭它使用的目标 Shell，不能指向正在工作的个人会话。

## 6. 关闭本次调试

手机先退出测试账号。以下命令只停止本文专用的 App、Desktop Agent/Shell 和 `ai-terminal-dev` 服务，不卸载 App，也不清空用户数据：

```sh
adb -s 127.0.0.1:62001 shell am force-stop dev.aiterminal.app
adb -s 127.0.0.1:62001 shell run-as dev.aiterminal.app rm -f files/acceptance-ca.pem
target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent" auth logout
target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent" --agent-stop
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml stop
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml ps -a
lsof -nP -iTCP:7200-7201 -sTCP:LISTEN
```

`--agent-stop` 会结束该 Agent 的所有 Shell；如果只想暂停手机远程访问而保留本地 Shell，执行 `auth logout` 后停在这里即可。Admin 与 Server 是同一服务，Compose `stop` 后 Admin 页面也会关闭。确认 `ps -a` 中两个容器均为 `Exited`，且 `lsof` 没有 7200/7201 监听；再次启动从第 1 步开始。Compose `stop` 保留测试账号、数据库卷、证书和管理员令牌；不要对持久的 `ai-terminal-dev` 项目执行 `down -v`。测试账号密码或 Admin 令牌如需轮换，应在 Admin 页面操作，并同步更新本机私有账号文件。若另外运行了 `scripts/run-ios-lan.py`，其模拟器和独立 Agent 可用 `python3 scripts/stop-local.py` 一并关闭；该脚本也会停止同一组 Docker 容器。

若仍使用先前的 `Terminal Fix Desktop` 独立测试 Agent，确认其中的 Shell 都不再需要后，另行运行 `target/debug/ai-terminal --state-dir "$PWD/.local/shared-input-test/agent" auth logout` 和同一路径的 `--agent-stop`；不要把它与默认 `.local/local-dev/agent` 的关闭命令混用。
