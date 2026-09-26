# Android 16 Agent 与读取锚点实测

- Date: 2026-09-26
- Status: Passed
- 用户指定设备：`emulator-5586`
- AVD：`aiterminal_api36_test`
- Android API：36，ABI：x86_64

使用生产版 `AgentSettingsPanel` / `AgentPanel`、实际 mobile-core、真实账号登录 SDK、加密配置/Agent RPC 和隔离 Desktop PTY。仪器测试账号只保留在内存中，不写入主账号偏好；Agent 历史缓存使用独立临时数据库。模型为本地确定性 SSE 桩，不冒充 ModelScope 实测。

## 通过项目

1. 设置界面显示默认首 10 行、尾 20 行。
2. 输入 0 在界面被拒绝，Desktop 配置修订号不变。
3. 保存首 7 / 尾 31，界面重载值正确；另用 Desktop CLI 回读同一账号配置得到 `7/31`（fixture revision 2）。
4. 从 Agent 面板发送只读任务，经真实加密 RPC 触发隔离 PTY 读屏和分析，界面显示最终 `UI_FIXTURE_DONE`。
5. 历史视图显示持久回复；原文对话框实际打开并显示原文中的 `TUI status: 01`。
6. 原样首部 7 行、尾部 31 行保持；搜索尾部 31 行全部为 `UI_LOG_*` 稳定日志，排除状态栏与交互提示符。
7. App 主账号和主连接 SharedPreferences 前后相同；没有 `pm clear`、卸载数据、修改全局 rc 或关闭用户终端。
8. 临时服务器、Desktop、PTY 和本次 adb reverse 映射已清理；模拟器保持运行。

`AgentReadingUiTest.readingSettingsAndAgentEvidenceRoundTrip` 最终实跑 **1 项通过，9.329 秒**。截图检查发现预填数值会隐藏 EditText hint，已为首/尾补充常驻标签并重新实跑通过。

## 证据与复跑

本机报告与截图位于 `.local/emulator-5586-agent/`：

- `instrumentation.log`、`results.json`
- `desktop-reading.json`
- `defaults.png`、`settings-saved.png`
- `conversation.png`、`history.png`、`evidence.png`

已有移动原生库和绑定后，在仓库根执行：

```sh
CARGO_INCREMENTAL=0 cargo +stable build --locked --offline -p ai-terminal \
  --bin aTerminal --example account_demo
./apps/android/gradlew -p apps/android \
  :app:assembleDebug :app:assembleDebugAndroidTest --offline
python3 scripts/test-android-agent.py --serial emulator-5586 \
  --output .local/emulator-5586-agent
```

运行器只控制明确指定的设备，为每轮创建临时服务、账号和 PTY。测试直接通过内存 Account 连接来隔离已有登录状态，因此本报告不覆盖登录表单和设备选择页面；也不代表 iOS 真机、Android 物理机或真实模型在手机上的性能验收。真实 ModelScope 的独立 Desktop 端到端结果见 [LIVE-MODELSCOPE.md](LIVE-MODELSCOPE.md)。
