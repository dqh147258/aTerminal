# 宿主显示确认与现有 Agent 升级

- Type: manual-verification
- Updated: 2026-09-26

代码和隔离自动化验证已完成。当前工具环境没有已验证可靠的桌面 GUI 控制/截图通道，本轮未操作真实 iTerm/Terminal.app 用户窗口；以下是建议的视觉确认，不是自动化测试步骤。

## 安全试用

现有 `/Users/carl/.cargo/bin/aTerminal` 启动脚本指向本仓库 `target/debug/aTerminal`，已构建新版。实际配置的 Agent 尚未重启：查询发现 4 个会话，其中 1 个仍存活且已连接。重启会终止存活 Shell 并丢弃内存历史，因此本轮保留它。

可先在 iTerm 或 Terminal.app 新窗口使用隔离实例：

```sh
aTerminal --state-dir /tmp/aterminal-desktop-trial
```

在内层 Shell 输出 `for i in {1..200}; do printf '\033[31m行 %s 中文\033[0m\n' "$i"; done`，检查滚轮、Shift+PageUp/PageDown 可看到早期输出及中文颜色；Esc 回到实时；回看时另一个输入端产生输出不应推走阅读位置。`--watch` 只允许浏览。iTerm 声明 truecolor 时保留 RGB，Terminal.app 的标准 xterm-256color 环境应有正常的 256 色显示。

进入前在外层输出一个标记，进入后应从清空的备用屏显示会话；Ctrl+] 脱离后外层标记仍在，原宿主历史不被清除。重新 `--attach SESSION_ID` 应能继续回看同一 Agent 的历史。宿主原生滚动条仍不是内部历史入口；使用滚轮或分页键。

结束隔离试用后：

```sh
aTerminal --state-dir /tmp/aterminal-desktop-trial --agent-stop
```

## 正常实例升级

先结束/保存现有 Shell 工作和需要保留的输出，然后执行：

```sh
aTerminal --agent-stop
aTerminal
```

新 CLI 对旧 Agent 的滚动请求会显示一次升级提示，旧会话仍可输入；不会自动重启，也不会把未知操作当成“历史为空”。当前 Agent 的滚动能力只有重启后才能更新。

Windows 宿主视觉与 ConPTY 本轮未验证。Shell 的 ↑ 命令历史、跨 Agent 重启的输出持久化不属于本次修复。


## 追加功能的确认与安装

- Desktop：鼠标移到**终端内容区**最右一列（不是窗口外侧原生滚动条），应出现内部滚动条；拖到顶部可看到最早保留输出，拖到底部松开后恢复实时。悬停隐藏后最右侧文字应完整恢复。Vim 等接管鼠标时常规点击仍归应用，Shift 明确转为本地浏览。
- Android/iOS：打开同一会话的“终端历史”，首屏最多加载 200 行；点击“加载更早记录”继续读取，已加载数应累加直到总数。输出 450 行唯一编号后逐页检查首尾与 Desktop 一致；新增输出不会改变旧副本，点击“读取最新历史”后才更新。关闭面板或切换会话时不混入迟到页面。
- 需要同时更新 Desktop Agent 和移动 App。保留现有会话的升级安排不变；当前手机上旧 App 不会因构建成功而自动更新。
- Android APK：`apps/android/app/build/outputs/apk/debug/app-debug.apk`。
- iOS 模拟器 App：`/tmp/aterminal-history-xcode/Build/Products/Debug-iphonesimulator/aTerminal.app`。原构建目录写入失败后已在此隔离目录构建成功；设备共享库也已构建，但未做签名安装或真实设备手势验收。


## Android 主画面手势已验收

新版已安装在用户授权的 `emulator-5586`，隔离测试保留正常账号并通过真实手势验证。主画面可向下拖动内容读取更早输出，向上拖回最新；也支持鼠标滚轮。回看时显示历史位置，输入返回实时。此前“未安装设备”的记录仅适用于上一阶段；本次 Android 已安装，iOS 主画面手势未改动。

设备证据：`.local/mobile-scroll-check/verified/results.json`、`history.png`、`returned.png`。如使用其他 Android 设备，安装最新 `apps/android/app/build/outputs/apk/debug/app-debug.apk` 即可；未操作 `127.0.0.1:62001`。正常 Agent 的只读诊断已确认支持 Scrollback，无需为本次 Android 手势修复重启它。


## 2026-09-27 重启后状态

Android 16 emulator-5586 已重新启动，最终 APK 已重装并通过主画面触摸/滚轮、输入与绘制回归。模拟器保持运行，App 已退出隔离测试并正常重新启动。原账号保留；系统重启后的正常业务 Server/Agent 不由本次隔离测试启动，若界面显示离线，需按本地启动文档恢复服务。测试报告位于 `.local/mobile-scroll-check/reboot/`。
