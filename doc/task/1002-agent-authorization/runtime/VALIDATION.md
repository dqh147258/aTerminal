# Runtime 验证证据

2026-10-03，本 worktree source基于8eaf8bb / 39fcb54 / 06b4c9d及工具依赖28d5700。High本地必要覆盖，所有Cargo jobs=2、test threads=2；OS进程身份/cwd、loopback与临时PTY用环境升级，完全隔离Desktop状态、账号与模型。

通过命令与结果：

- `cargo +stable test --locked -j 2 -p ai-terminal-agent-runtime -p ai-terminal-agent --lib -- --test-threads=2`：Runtime 99/99；Desktop最终独立复跑57/57（最后HTTP MCP目录修复和native二进制门控）。
- 最后人类等待身份复核加入取消select后，`cargo +stable test --locked -j 2 -p ai-terminal-agent-runtime --lib host::user_interaction -- --test-threads=2`：11/11。
- `cargo +stable test --locked -j 2 -p ai-terminal --bin aTerminal agents::tests -- --test-threads=2`：2/2参数/RPC解析。
- `cargo +stable test --locked -j 2 -p ai-terminal --test authorization_cli -- --test-threads=2`：3/3真实CLI、独立Daemon、本地SSE模型桩与PTY/native。最后Native result_record_id接线后其always闭环聚焦再次通过。
- `cargo +stable clippy --locked -j 2 -p ai-terminal-agent-runtime -p ai-terminal-agent --lib --tests --no-default-features -- -D warnings`：通过，包含外部retention测试编译。
- `cargo +stable clippy --locked -j 2 -p ai-terminal --bin aTerminal --test authorization_cli -- -D warnings`：通过。
- `cargo +stable fmt --all -- --check`、`git diff --check`：通过。

行为覆盖：approval exact once/重复响应/deny/full开启释放pending/后设full只执行新调用；questions真实回答；CAS/24h过期/取消/重启不重放/Global继承不污染Session；共享全树human暂停和其他active任务耗时；真实Broker preflight之后插入他Agent draft的Actor最后拒绝；初始/完整提交/未知typeahead的输入边界；legacyallow映射、readonly设备和scope/未知RPC字段；rules两轮regrant/revoke ID稳定；v2不能匹配v3；native tee exact append首次always/auto/revoke/deny/regrant/revoke；unknown原生程序仍可调用，exit7/EOF/输出限额/cancel kill group且原PTY未停；动态Skill同Run cd真实cwd；native旧cwd拒绝；扩展disable/version撤销；HTTP远端cwd未知；closed Session只读capability。

完整临时输出位于 `/private/tmp/aterminal-runtime-final-libs.log`、`aterminal-runtime-final-desktop.log`、`aterminal-runtime-last-host.log`、`aterminal-runtime-native-final-cli.log`、`aterminal-runtime-last-clippy.log`、`aterminal-runtime-last-cli-clippy.log`。它们是本地测试桩证据，不证明真实供应商或物理设备。

剩余root验收：最终主线复跑、独立encrypted Review/实际Bash-Zsh override marker、Android/iOS UI；PowerShell本机不可用，不声称runtime实测。无付费模型、正常账号/终端/手机模拟器被用于本执行者验证；无OS/cwd沙箱或通用TUI完成保证。
