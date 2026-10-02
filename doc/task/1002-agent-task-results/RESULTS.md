# task_id 查询、等待与 Review 修复验收

2026-10-02，状态：通过。用户授权修复两个 Review 问题、启动模拟器本地测试并在通过后提交。

## 修复结果

1. 任务结束报告将错误诊断按 UTF-8 边界限制为 2048 字节，附带 `error_truncated`。即使含多字节、引号、换行及控制字符，报告仍在存储上限内，终态事务可以提交，子任务 pins 正常释放。Global 回报沿用同样的截断规则。
2. 查询与等待在 Store 同一临界区内读取结果并将答复和选中的完成报告绑定到调用方 Global Run，阻止并发清理删除引用。父 Run 结束后释放保护。仅允许同账号、同 Desktop 的委托任务，并校验调用方 Run 仍活动。

## 自动化检查

- `cargo +stable test --locked -p ai-terminal-agent-runtime -p ai-terminal-agent`：90 个测试通过（Desktop 30、Runtime 53、模型协议 5、Review 回归 2）。进程工作目录测试需要沙箱外的 macOS `ps/lsof`，本次在该环境下通过。
- `cargo +stable clippy --locked -p ai-terminal-agent-runtime -p ai-terminal-agent -p ai-terminal --all-targets -- -D warnings`：通过。
- `cargo +stable fmt --all -- --check`、`git diff --check`、Python 运行器语法解析：通过。
- Desktop CLI 与 `account_demo` 示例构建通过；Android Debug 和 AndroidTest APK 构建通过。Android 既有弃用提示与沙箱 FSEvents 提示未影响构建。

## Android 模拟器

使用 `android-emulator-control` 启动并核验 `aiterminal_api36_test`，serial 为 `emulator-5586`，Android 16 / API 36 / x86_64。启动脚本把内核 `ramoops` 信息误判为失败；随后已确认匹配的模拟器进程存活、ADB device 在线且 boot_completed=true。

验收使用生产 AgentPanel、mobile-core、加密 RPC、当前构建的隔离 Desktop 和临时 PTY。模型为本地确定性 HTTP/SSE 桩，不代表真实供应商或物理设备验收。保留 App 主账号及连接偏好；测试完成后临时服务、Desktop、PTY、fixture 文件与本次 adb reverse 已由运行器清理，模拟器保持运行。

最终仪器测试 `AgentReadingUiTest.readingSettingsAndAgentEvidenceRoundTrip` **1 项通过，21.592 秒**：

- 从 Global composer 提交，实际调用 `send_agent_message`、`get_agent_task`、`wait_agent_task`；1 ms 等待返回 timed_out，后续等待返回正确终态。
- 子 Agent 返回长答复；Global 得到截断结果及原文 UUID。测试在回读前通过本地管理接口清理该 Session：删除 8 条记录，保留被父 Run 保护的最终答复。完成报告位于 Global scope，所以 Session 清理计数中的 pinned 为 1。
- `read_record` 经过真实分析步骤，将分页游标保留在摘要中；模型侧实际接收到 **2 页 / 14,652 字节**，拼接后与原答复完全相同。
- HTTP 模型返回超长错误；Global 收到 paused、error_truncated=true、2048 字节诊断。运行器另用只读 SQLite 连接核验数据库状态为 paused，子任务 pins 为 0。
- 原有设置、终端证据、图片及多 Global 会话用例一并通过。

证据：[仪器日志](evidence/instrumentation.log)、[App 报告](evidence/results.json)、[结果回读和清理](evidence/task-result-observations.json)、[错误回报](evidence/task-error-observations.json)、[数据库核验](evidence/task-durability.json)、[工具调用界面](evidence/task-results.png)、[错误任务界面](evidence/task-error.png)。

前三轮失败均保留在 evidence/attempt*-instrumentation.log：分别修正既有测试桩把后台状态当作最新用户消息的问题、完成报告与答复处于不同 scope 的清理计数断言，以及新增测试桩未处理 read_record 分析阶段的问题。随后完整流程通过；最后补充滚动与截图检查后再次通过。

## 复跑

已有 Android 原生库与绑定时，构建 Desktop 和 APK：

```sh
cargo +stable build --locked -p ai-terminal --bin aTerminal --example account_demo
env JAVA_HOME='/Applications/Android Studio.app/Contents/jbr/Contents/Home' \
  ANDROID_HOME='/Users/carl/Library/Android/sdk' \
  ./apps/android/gradlew -p apps/android :app:assembleDebug :app:assembleDebugAndroidTest --offline
python3 scripts/test-android-agent.py --serial emulator-5586 \
  --adb /Users/carl/.android/sdk-modern-test/platform-tools/adb \
  --output .local/task-results-emulator/recheck
```

测试仅操作指定的 SDK 模拟器与隔离实例，没有重启日常使用的 Desktop Agent。实际服务需要更新 Desktop 二进制并启动新 Run 后获得新工具。
