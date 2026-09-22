# 真机与环境验证交接

初轮实施时没有可见真机。2026-09-22 用户连接 M2012K10C 后已完成 Android 12 真机功能与性能测试，详见 [ANDROID-DEVICE.md](ANDROID-DEVICE.md)。iOS 仍只有模拟器验证，Windows/Linux 原生运行环境未验证。

Android 登录、直连、USB 回环中转、Ctrl-C、后台恢复、搜狗中文提交与各 1,000 字符压力测试已通过。最终整帧 p95 约 19–23ms，持续输出时编辑事件 p95 约 22ms，仍需继续优化；不能称为全部性能目标达标。锁屏解锁及防误触规则见 [设备调试说明](../../../deploy/ANDROID-DEVICE.md)。

随后追加的一致性优先续作见 [FOLLOWUP.md](FOLLOWUP.md)：已降低重复复制/锁等待，补充像素、原子提交和输入身份回归；最终固定 20ms 目标节拍的数据与上述首轮数据不可混用。部分整帧耗时改善，历史并发回显仍未改善。后续应量化路径 RTT、发布/恢复频次和序列化/校验成本，保持完整性与输入可靠性约束。

建议后续验收：

1. 同一账号分别登录 Desktop 与手机，选择设备/会话，连续中文拼音组合、英文、Emoji、退格与功能键。预期输入框编辑顺滑；文字仅在点击发送后执行；失去控制权后不能继续写入。
2. 120×40 持续输出时打字、Ctrl-C、查看历史、缩放、横竖屏与切会话。预期输入不被完整历史查询阻塞，光标/宽字符正确，切会话不出现旧画面。Instruments 查看 `InputEnqueue` / `TerminalDraw`，Android 用系统帧分析工具；分别记录输入框编辑与远端回显。
3. 使用受控路由器/netem，在 direct、TURN 和 WSS 上覆盖 RTT 20/80/150ms、实际丢包 0/1%、Wi-Fi↔蜂窝切换。每组至少 1,000 次输入，记录 p50/p95/p99、CPU/RSS、是否触发回退及队列拒绝。不要把已有 TCP 停顿模拟结果等同于真实丢包。
4. 手机后台后桌面 Shell 继续运行；回到前台选设备恢复，显式接管后输入。用另一设备撤销当前设备，预期活动连接关闭、旧令牌不能重连；桌面会话保留。
5. 在 Windows 验证 CLI 登录凭据存储、ConPTY/PSReadLine 与设备撤销；Linux 验证 Secret Service 和无 DBus 私有文件模式。CI 配置已补 `libdbus-1-dev`，但本地没有替代对应系统运行验证。
6. 发布前配置可信 HTTPS、正式 App 签名；iOS 真机包目前仅链接验证，不能直接安装。管理员用 `ai-terminal-server user add` 创建账号，不共享管理员 token。

操作说明：[deploy/ACCOUNTS.md](../../../deploy/ACCOUNTS.md)。性能方法和实测边界：[RESULTS.md](RESULTS.md)。用户无需为了这些推荐验收步骤更新 TODO。
