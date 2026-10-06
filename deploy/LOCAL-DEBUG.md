# 本机快速启动、调试与测试

本页针对这台 Mac 的真实局域网部署：Server `https://192.168.0.36:7200`，Android 设备 `127.0.0.1:62001`。手机直接访问主机的局域网 IP，不使用 `adb reverse`。Server、Desktop Agent、手机 App 和终端 Shell 都是真实进程。移动端已开放 Agent 对话与设置，语音入口关闭；模型与读取锚点配置见 [Agent 说明](ASSISTANT.md)。

## 一键启动与关闭

在仓库根目录执行：

```sh
python3 scripts/local-dev-up.py
aTerminal-dev
aTerminal-dev --list
python3 scripts/local-dev-down.py
```

启动脚本会检查并启动 Docker Server/LAN TLS、复用或创建测试账号、编译 Desktop CLI、安装 `~/.cargo/bin/aTerminal-dev` 命令、复用 Desktop 的登录身份、安装 Debug APK，并让 Android 自动登录和连接 `Local Desktop 新版`。如果 APK 不存在、缺少构建元数据或包名与当前应用不一致，会自动构建；Android 代码改动后加 `--build-android` 强制重建。启动完成后，新开终端也可以直接运行 `aTerminal-dev`，无需附带二进制路径或 `--state-dir`。若 shell 找不到命令，检查 `~/.cargo/bin` 是否在 `PATH` 中。只启动 Server 和 Desktop 时使用 `--skip-android`；两脚本都可用 `--android-serial` 指定另一台设备。

`~/.cargo/bin/aTerminal` 留给 release 版本；本地调试入口 `aTerminal-dev` 调用仓库中的 `target/debug/aTerminal`，使用 `.local/local-dev/config-next` 配置和 `.local/local-dev/agent-next` 状态目录。启动脚本只安装调试入口，不覆盖 release 命令。

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
| `desktop` | Rust Desktop CLI | `target/debug/aTerminal` |
| `android` | arm64-v8a、x86_64、x86 Rust 库、FFI、Debug APK、Android 测试 APK 和 Lint | `apps/android/app/build/outputs/apk/debug/app-debug.apk` |
| `ios` | 设备与本机模拟器 Rust 库、FFI、XCFramework、Debug 模拟器 App | `build/xcode/Build/Products/Debug-iphonesimulator/aTerminal.app` |
| `server` | 本地 Server Docker 镜像 | `ai-terminal-server:development` |

构建只生成产物；Server 镜像改动后再运行 `python3 scripts/local-dev-up.py` 让部署加载新镜像。`--build-android` 是一键启动时重建手机产物的快捷选项。正式 iOS 发布包还需要签名，此脚本产出的是模拟器调试 App。iOS 真机/模拟器的局域网试用见 [iOS 局域网说明](IOS-LAN-TRIAL.md)。

## 调试和验证

`aTerminal-dev` 创建 Shell；`aTerminal-dev --list` 列出会话；`aTerminal-dev --attach SESSION_ID` 在桌面重新附着并恢复手机输入；`aTerminal-dev --history SESSION_ID` 查看历史。Desktop 和手机附着同一运行会话时均可输入，回车、Tab、Ctrl-C 应作用于同一 PTY。桌面按 Ctrl+] 脱离后，手机保留画面与历史，但该会话暂停手机输入；重新附着后恢复。Shell 真正结束时只保留历史，不能复原原进程。

Desktop 内用滚轮回看输出，或用 Shift+PageUp/PageDown 按页浏览；Esc、滚回底部或键入内容返回实时画面。浏览时新输出继续保存在会话中，阅读位置不跳动；颜色、中文和组合字符保留。`--watch` 支持相同浏览操作但不发送键入内容。应用启用鼠标协议时滚轮交给应用；Shift+滚轮改为本地回看。Vim 等备用屏按应用启用的 alternate-scroll 模式接收滚动方向键。

鼠标移到 aTerminal **内容区最右一列**即可显示内部滚动条；点击轨道可跳转，按住滑块上下拖动可定位，回看时滚动条保持可见。返回底部并松开后恢复实时输出。普通应用已接管鼠标或使用备用屏时，右侧事件仍归应用；可按住 Shift 使用本地历史浏览。滚动条只覆盖本地显示，不缩窄 PTY 或移动端画面，隐藏后恢复被覆盖的字符。

aTerminal 使用宿主备用屏：iTerm/Terminal.app 的原生滚动条不代表 aTerminal 会话历史，请用上述内部浏览方式。进入时清空备用屏并归位光标，退出恢复宿主主屏，不删除宿主历史。CLI 根据自身宿主的 `COLORTERM` / `TERM` 选择 truecolor、256 色或基础 ANSI 色；内层 Shell 的 truecolor 能力不受宿主降级影响。

Android/iOS 的“终端历史”从同一 Agent 历史副本分页读取，每页最多 200 行，点“加载更早记录”可读到保留范围的最早记录；200 是页大小，不是保留长度。面板显示已加载/总行数及保留上限提示，“读取最新历史”建立新副本。两端与 Desktop 统一按权威终端物理行计数，范围包含历史和捕获时的当前屏幕；手机换行显示不会改变总行数。仅当捕获时刻、会话和尺寸相同时，才应比较相同的总数。纯文本分页保留内容，Desktop 另保留样式。只读配对也可读完整历史；关闭面板、切换会话或断开后释放/回收副本。旧 Agent 会提示升级，不能静默把最近 200 行当成全部记录。

Android **终端主画面**也可直接查看历史：手指向下拖动内容可回看更早输出，向上拖动返回较新的输出，滚到最底部恢复实时。实时网格本身高于屏幕时，先平移到网格顶部，再继续拖动即可进入历史；横向仍可平移宽终端。鼠标滚轮同样支持。回看状态显示当前位置并隐藏实时光标，新输出继续接收但不打断阅读；开始输入、改变终端尺寸或切换会话会返回实时。历史手势不发送 Shell 方向键，避免误翻命令历史。该主画面手势已在 Android emulator-5586 验证；iOS 本轮未改动主画面手势，仍保留独立历史面板。

Android 主画面手势回归可用 `python3 scripts/test-android-scroll.py --serial emulator-5586 --output .local/mobile-scroll-check/verified`：使用独立账号/PTY，安装 Debug 与测试 APK，验证真实上下滑动、横向平移、后台追加、输入一次和鼠标滚轮，保留正常账号数据。依赖已构建的 `account_demo` 示例与 Desktop binary；测试会临时打开隔离界面，结束后可重新启动 App 回到正常账号。

Agent 保留最多 10,000 行输出历史。回看副本按客户端隔离，最多 150 万个单元/64 MiB；同一会话至多 4 个副本、合计 128 MiB。遇到历史/内存裁剪时最早一页会显示提示；返回实时、脱离、尺寸或主备用屏改变会释放副本，失联客户端在 30 秒后回收。原会话历史不因此删除。历史目前存放在 Agent 内存中，停止 Agent 或显式关闭会话后不能恢复。

滚动功能需要 CLI 和 Agent 都更新。连接旧 Agent 时仍可实时使用，会提示一次升级需求，不会自动重启后台。先完成需要保留的 Shell 工作，再执行 `aTerminal-dev --agent-stop`，下一次运行 `aTerminal-dev` 会启动新版 Agent；停止前应自行保存需要的输出。若要保留当前 Agent 并单独试用，可运行 `aTerminal-dev --state-dir /tmp/aterminal-desktop-trial`，试用后用同一 `--state-dir` 加 `--agent-stop` 关闭隔离 Agent。隔离实例不会继承当前手机配对与会话。

用电脑键盘控制 Android/iOS 模拟器时，只要当前终端会话可输入且没有打开弹窗，普通字符、Enter 和 Tab 会直接送到 Shell；即使焦点曾落在终端工具按钮上，Enter 也不会打开“历史”。弹窗中的文本框仍正常接收键盘输入。移动端默认显示细竖线光标，Desktop CLI 将默认光标形状交由宿主 Terminal 的设置决定。

终端键盘和输入法已提交的字符使用 `RemoteTerminal.typeText`，避免 Zsh 把逐字输入当作粘贴而反白显示。输入法提交的 TAB 和模拟器文本事件也会转为终端 Tab 补全；显式粘贴接口 `sendText` 继续保留 bracketed paste 语义。这项修复只需更新手机 App，无需重启 Desktop Agent 或现有 Shell。

Android 可使用键盘栏“粘贴”、Ctrl-V 或系统粘贴动作；iOS 支持系统粘贴及终端长按菜单“粘贴”。这些入口均按原样发送剪贴板文本，不自动按回车。IME 未标明来源时，仅单独的 TAB/换行按键处理；含 TAB/换行的批量文本整体走粘贴。由于 `commitText` 不区分键入和剪贴板，若要粘贴单独一个 TAB/换行，请使用显式粘贴入口。

一键启动完成后，在 Desktop 运行 `git status`、Tab 补全、Ctrl-C 等操作，同时观察手机终端画面和会话列表。要确认设备没有重复注册，可多次运行启动脚本，然后检查 `aTerminal-dev devices list` 中当前 Desktop 和当前 Android 的 ID 均保持不变，且每个平台仅有一台在线。`adb -s 127.0.0.1:62001 reverse --list` 应为空。手机前台应为 `com.yxf.aterminal/.MainActivity`。Android 最低支持 API 25，构建同时覆盖 arm64-v8a、x86_64 和 x86。

常用检查命令：

```sh
curl --fail --noproxy '*' --cacert deploy/secrets/lan-ca.crt https://192.168.0.36:7200/healthz
aTerminal-dev auth status
aTerminal-dev devices list
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml ps -a
docker compose -p ai-terminal-dev -f deploy/compose.lan.yaml -f deploy/compose.test-network.yaml logs --tail 100 server lan_tls
tail -n 100 .local/local-dev/agent-next/logs/agent.log
adb -s 127.0.0.1:62001 logcat -d -s AndroidRuntime
```

Rust 与本机终端回归可运行 `cargo +stable test --locked --workspace --exclude ai-terminal-bindgen`、`cargo +stable clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings`、`python3 scripts/test-host-terminal.py target/debug/aTerminal`。完整的 API 25/x86 本地部署验收记录见 [验收报告](../doc/task/0924-local-deployment-test/RESULTS.md)。

## 指定模拟器上的隔离 Agent 测试

用户指定的 Android SDK 模拟器 `emulator-5586`（Android 16 / x86_64）已验证配置首尾行数与 Agent 读取流程。可用 `python3 scripts/test-android-agent.py --serial emulator-5586 --output .local/emulator-5586-agent` 复跑；该脚本仅控制指定设备，使用临时账号/服务/PTY，不清除 App 数据，并在结束时移除本次端口转发。与上面的真实 LAN 部署不同，隔离测试使用临时 loopback 服务和限定端口的 adb reverse。完整证据与构建前置条件见 [Android Agent 验收](../doc/task/0925-terminal-agents/ANDROID-AGENT-UI.md)。

## 登录页默认地址与指定模拟器登录

本地 Debug 包内置 `https://192.168.0.36:7200`；登录页只展示地址，小字“修改服务器地址”可编辑保存。已保存的地址优先于构建默认值。`scripts/build-artifacts.py android ios --server-url URL --server-ca /path/to/public-ca.pem` 可配置两端地址和公共 CA；省略参数使用本地地址及已有 `deploy/secrets/lan-ca.crt`。

在原生依赖已构建时，用 `./apps/android/gradlew -p apps/android :app:assembleDebug :app:assembleDebugAndroidTest --offline -PterminalServerUrl=https://192.168.0.36:7200` 更新 Android 包。然后运行 `python3 scripts/login-android-local.py --serial emulator-5586`，使用已有本地账号经普通登录页登录并验证进程重启恢复，最后保持 App 登录。此脚本要求显式指定设备；不启动或停止 Server/Desktop，不覆盖其他已登录账号。

## 目录改名后 Docker 挂载旧路径

若启动提示 `bind source path does not exist`，且路径仍为旧目录名（例如 `AITerminal` 而当前为 `aTerminal`），原因是现有 Server 容器保留了文件 secret 的旧绝对路径。macOS 文件系统对大小写的处理不能替代 Docker Linux 挂载路径；普通 `compose up` 可能仍复用该容器。

`prepare-lan.py` 会比较现有容器的 `/run/secrets/admin_token` 挂载与当前项目路径，发现不一致时自动重建 Server/TLS 容器，保留 `ai-terminal-dev_server_data` 数据卷、证书及账号。路径一致时照常复用容器，不会每次启动都强制重建。
