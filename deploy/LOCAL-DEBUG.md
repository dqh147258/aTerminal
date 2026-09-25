# 本机快速启动、调试与测试

本页针对这台 Mac 的真实局域网部署：Server `https://192.168.0.36:7200`，Android 设备 `127.0.0.1:62001`。手机直接访问主机的局域网 IP，不使用 `adb reverse`。Server、Desktop Agent、手机 App 和终端 Shell 都是真实进程。移动端 AI 对话和语音输入目前仍是不可交互的占位功能。

## 一键启动与关闭

在仓库根目录执行：

```sh
python3 scripts/local-dev-up.py
ai-terminal
ai-terminal --list
python3 scripts/local-dev-down.py
```

启动脚本会检查并启动 Docker Server/LAN TLS、复用或创建测试账号、编译 Desktop CLI、安装 `~/.cargo/bin/ai-terminal` 命令、复用 Desktop 的登录身份、安装 Debug APK，并让 Android 自动登录和连接 `Local Desktop 新版`。如果 APK 不存在会自动构建；Android 代码改动后加 `--build-android` 强制重建。启动完成后，新开终端也可以直接运行 `ai-terminal`，无需附带二进制路径或 `--state-dir`。若 shell 找不到命令，检查 `~/.cargo/bin` 是否在 `PATH` 中。只启动 Server 和 Desktop 时使用 `--skip-android`；两脚本都可用 `--android-serial` 指定另一台设备。

关闭脚本会停止指定 Android App、`.local/local-dev/agent-next` Desktop Agent 及其所有 Shell、以及本项目的 Docker 容器。**先处理仍在运行的 Shell 工作**：Agent 停止后其 PTY 无法恢复。脚本保留 Server 数据卷、证书、测试账号、Desktop 私有凭据及手机的 Keystore/登录身份，下一次启动会复用同一设备，不会执行 `auth logout` 或撤销设备。单独运行 `scripts/stop-local.py` 是旧 iOS 试用环境的关闭入口，不能替代此处的脚本。

## 账号和地址

| 用途 | 入口或账号 |
| --- | --- |
| 手机和 Desktop 的 Server | `https://192.168.0.36:7200` |
| Mac 上的 Admin | `http://127.0.0.1:7201/admin/` |
| Admin 登录 | 无用户名/密码；在本机执行 `cat deploy/secrets/admin-token`，把令牌填入 Admin 页面 |
| 测试用户 | `aiterminal_local_test`；在本机执行 `python3 scripts/local-dev-account.py show` 查看随机密码 |

测试密码位于 Git 忽略的 `.local/local-dev/account.json`，该目录和文件分别限制为 0700/0600。`ensure` 会复用已有账号和密码；如果 Server 里已有同名用户但本地凭据丢失，脚本会停止，不会悄悄重置密码。Admin 令牌不是测试用户密码。Debug App 临时读取本机 CA 和测试密码完成自动登录，随即删除临时密码文件；保存的手机身份位于 App 私有存储。这个自签名 CA 的调试入口不代表普通用户无需信任配置即可登录。

## 构建指定平台的产物

使用 `scripts/build-artifacts.py`，平台名称为 `desktop`、`android`、`ios`、`server` 或 `all`。可用空格或逗号组合；`--dry-run` 只打印步骤：

```sh
python3 scripts/build-artifacts.py desktop android
python3 scripts/build-artifacts.py ios,server
python3 scripts/build-artifacts.py all
python3 scripts/build-artifacts.py --dry-run desktop,android
```

脚本默认使用 `stable` Rust toolchain；可用 `--toolchain 1.94.1` 固定版本。Android NDK r28+ 默认从 `ANDROID_NDK_HOME`、`ANDROID_NDK_ROOT` 或 SDK 安装目录查找，必要时传入 `--ndk /path/to/ndk`。需要 Docker Desktop；iOS 构建需要 macOS/Xcode，Android 构建需要 Android SDK/NDK 和 JDK 17。首次构建可能较久。

| 平台 | 编译内容 | 输出 |
| --- | --- | --- |
| `desktop` | Rust Desktop CLI | `target/debug/ai-terminal` |
| `android` | arm64-v8a、x86_64、x86 Rust 库、FFI、Debug APK、Android 测试 APK 和 Lint | `apps/android/app/build/outputs/apk/debug/app-debug.apk` |
| `ios` | 设备与本机模拟器 Rust 库、FFI、XCFramework、Debug 模拟器 App | `build/xcode/Build/Products/Debug-iphonesimulator/AITerminal.app` |
| `server` | 本地 Server Docker 镜像 | `ai-terminal-server:development` |

构建只生成产物；Server 镜像改动后再运行 `python3 scripts/local-dev-up.py` 让部署加载新镜像。`--build-android` 是一键启动时重建手机产物的快捷选项。正式 iOS 发布包还需要签名，此脚本产出的是模拟器调试 App。iOS 真机/模拟器的局域网试用见 [iOS 局域网说明](IOS-LAN-TRIAL.md)。

## 调试和验证

`ai-terminal` 创建 Shell；`ai-terminal --list` 列出会话；`ai-terminal --attach SESSION_ID` 在桌面重新附着并恢复手机输入；`ai-terminal --history SESSION_ID` 查看历史。Desktop 和手机附着同一运行会话时均可输入，回车、Tab、Ctrl-C 应作用于同一 PTY。桌面按 Ctrl+] 脱离后，手机保留画面与历史，但该会话暂停手机输入；重新附着后恢复。Shell 真正结束时只保留历史，不能复原原进程。

用电脑键盘控制 Android/iOS 模拟器时，只要当前终端会话可输入且没有打开弹窗，普通字符、Enter 和 Tab 会直接送到 Shell；即使焦点曾落在终端工具按钮上，Enter 也不会打开“历史”。弹窗中的文本框仍正常接收键盘输入。移动端默认显示细竖线光标，Desktop CLI 将默认光标形状交由宿主 Terminal 的设置决定。

终端键盘和输入法已提交的字符使用 `RemoteTerminal.typeText`，避免 Zsh 把逐字输入当作粘贴而反白显示。输入法提交的 TAB 和模拟器文本事件也会转为终端 Tab 补全；显式粘贴接口 `sendText` 继续保留 bracketed paste 语义。这项修复只需更新手机 App，无需重启 Desktop Agent 或现有 Shell。

Android 可使用键盘栏“粘贴”、Ctrl-V 或系统粘贴动作；iOS 支持系统粘贴及终端长按菜单“粘贴”。这些入口均按原样发送剪贴板文本，不自动按回车。IME 未标明来源时，仅单独的 TAB/换行按键处理；含 TAB/换行的批量文本整体走粘贴。由于 `commitText` 不区分键入和剪贴板，若要粘贴单独一个 TAB/换行，请使用显式粘贴入口。

一键启动完成后，在 Desktop 运行 `git status`、Tab 补全、Ctrl-C 等操作，同时观察手机终端画面和会话列表。要确认设备没有重复注册，可多次运行启动脚本，然后检查 `ai-terminal devices list` 中当前 Desktop 和当前 Android 的 ID 均保持不变，且每个平台仅有一台在线。`adb -s 127.0.0.1:62001 reverse --list` 应为空。手机前台应为 `dev.aiterminal.app/.MainActivity`。Android 最低支持 API 25，构建同时覆盖 arm64-v8a、x86_64 和 x86。

常用检查命令：

```sh
curl --fail --noproxy '*' --cacert deploy/secrets/lan-ca.crt https://192.168.0.36:7200/healthz
ai-terminal auth status
ai-terminal devices list
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml ps -a
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml logs --tail 100 server lan_tls
tail -n 100 .local/local-dev/agent-next/agent.log
adb -s 127.0.0.1:62001 logcat -d -s AndroidRuntime
```

Rust 与本机终端回归可运行 `cargo +stable test --locked --workspace --exclude ai-terminal-bindgen`、`cargo +stable clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings`、`python3 scripts/test-host-terminal.py target/debug/ai-terminal`。完整的 API 25/x86 本地部署验收记录见 [验收报告](../doc/task/0924-local-deployment-test/RESULTS.md)。
