# 在线设备、终端附着与光标结果

Android/iOS 的账号设备面板现在只显示 Server 报告为在线的设备。离线记录仍在账号中，不因为隐藏列表而自动撤销。终端光标由覆盖文字的半透明块改为高对比样式；块光标会重绘其下的文字，输入时把光标滚回可视区域，手动滚动后暂停自动跟随。Android 已避免使用会触发全视图链滚动的 `requestRectangleOnScreen`，改为分别滚动终端的水平与垂直容器。

新版 Agent 由本机 CLI 的专用附着操作维护 Desktop 活跃状态。按 Ctrl+] 脱离后，PTY、工作目录和已有画面仍在，但手机只读；Desktop `--attach SESSION_ID` 返回同一 PTY 后手机自动恢复输入。原始 Ctrl+] 字节在本机 crossterm 中被报告为 Ctrl+5，CLI 现兼容这个等价键。异常终止时，最多 15 秒后附着租约过期并变只读。Shell 真正退出时，手机仍可打开最后画面与现有滚动历史，但不能继续输入或复原进程。

现有默认 `.local/local-dev/agent` 还在运行旧 Agent，且仍有两条用户 Shell `84751f6b26645d1d`、`8df358908a56eb55`。新版 CLI 在这个旧 Agent 上自动回退到旧附着操作；新版 App 把缺少附着状态字段的旧 Agent 视作旧行为，因此两条会话仍可使用，但“桌面脱离后手机只读”只在新版 Agent 上生效。已将这两条会话最后有效画面和滚动历史保存到私有 `.local/local-dev/presence-preupgrade-20260924/`，没有结束它们。将新规则部署到默认 `Local Desktop` 必须重启 Agent，这会结束两条仍可用的 Shell，归档不能复活其进程；待用户决定后再切换。隔离的新版 `Presence Test Desktop` Agent 用于真实验收，测试 Shell 均已关闭，账号已退出、Agent 已停止，四条测试创建的离线 Probe 设备记录也已撤销。

| 验证 | 结果 |
| --- | --- |
| Rust | 全工作区测试、全目标 Clippy 和格式检查通过。隔离 Agent 加密通道回归覆盖 Desktop 脱离后手机自动只读、输入被拒、历史可读、重新附着恢复输入、Shell 退出后只读，以及旧 Agent 兼容。 |
| Desktop CLI 真实 PTY | 在新版 Agent 上实际发送 Ctrl+] 后 CLI 退出码 0，会话保持运行且 Desktop 附着状态为 false；重新 `--attach` 回到原目录/画面并执行 `RESUME_OK`。新版 CLI 附着旧默认 Agent 的专用测试 Shell 也成功输入，测试会话已关闭。 |
| Android x86 | 新版 x86/x86_64/arm64 原生库、Debug/Test APK 与 lint 构建通过；Nox 上 17 项界面/绘制/状态测试通过，含离线设备隐藏。真实局域网长流程在 Desktop 脱离检查点前被 Nox `system_server` WindowManager watchdog 重启打断，随后 Nox 来宾内核 panic 循环；新建和已有的 SDK x86_64 AVD 均停留 `adb offline`。这轮 Android 真服务生命周期测试未通过，不能据此宣称设备验收完成。故障 Nox VM 已关闭，磁盘数据保留。 |
| iOS | iPhone 15/iOS 17.5 模拟器 5 项 UI 测试通过，含离线设备隐藏；连接 `192.168.0.36:7200` 的新版 Agent 全流程通过，自动确认 Desktop 脱离只读、最后画面保留、CLI 重新附着后恢复输入；连接仍运行的旧 `Local Desktop` 的兼容全流程也通过。结果包：`build/terminal-presence-ios-ui.xcresult`、`build/terminal-presence-ios-detach-final.xcresult`、`build/terminal-presence-ios-legacy.xcresult`。 |

终端引擎继续保留现有 ANSI 256 色、24 位前景/背景色及 OSC 调色板路径；本次没有继承 Mac 宿主 Terminal 的主题。Agent 在独立进程中运行，宿主主题需要新配置同步协议及对应的跨端校验；用户允许该可选项在实现成本高时暂缓。若有具体 TUI/颜色序列可另行定位丢色点。
