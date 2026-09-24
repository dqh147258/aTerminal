# 默认 Agent 稳定性修复结果

用户的原命令 `target/debug/ai-terminal --state-dir "$PWD/.local/local-dev/agent"` 已在原状态目录上使用新版 Agent。旧进程 PID 22111 创建于 09:19，未因前一次编译而更新；其四条会话均处于 `running`，但每条都携带持久的 `invalid or oversized terminal frame`，所以任何新 CLI 一附着就退出。四个 Zsh 均无前台子进程。切换前已将各会话最后有效快照、可直接阅读的 `.screen.txt` 和最多 200 行滚动历史存入权限 0700/0600 的 `.local/local-dev/legacy-session-archive-20260924/`；这四条会话当时没有独立滚动历史，因此历史 `.txt` 为空。旧 PTY 的进程内状态无法迁移，切换后四条旧 Shell 已结束。

Agent 现在只发布通过协议校验的候选快照，含 Resize 路径；遇到瞬时无效画面时保留上一帧、在日志中记录首个错误，等后续有效帧继续同步，不再把显示错误永久写入会话的致命 `info.error`。CLI 对服务端明确报告的“会话已关闭”正常退出；真正超时或 Agent 不可达仍报告错误。会话关闭、请求队列已满与 RPC 超时由服务端明确区分。

本机重启默认 Agent 时，当前非交互环境未从 OS 凭据库读取旧设备凭据；已用既有本地测试账号和私有文件存储重新登录为在线 `Local Desktop`，并撤销停用的旧 Desktop 设备。`scripts/start-local-agent.sh` 固定使用当前私有凭据目录，重启后先执行它，再运行普通 CLI；账号密码未进入仓库。

| 验证 | 结果 |
| --- | --- |
| Rust 全工作区 | `cargo test --locked --workspace --exclude ai-terminal-bindgen` 通过；含无效中间帧后恢复、真实 `git status`、明确的会话关闭错误和并行输入回归。日志：`build/default-agent-stability-rust-test.log`。 |
| Clippy/格式 | 全目标 Clippy `-D warnings`、`cargo fmt --check` 和 `git diff --check` 通过。日志：`build/default-agent-stability-clippy.log`。 |
| 隔离真实 PTY | 在隔离 Agent 的 Zsh 中运行 `git status` 显示修改文件并返回提示符；外部关闭同一会话后，附着 CLI 退出码为 0。 |
| 默认原路径真实 PTY | 用用户给出的原命令新建 Shell，运行 `git status` 后返回提示符，继续运行 `printf 'STILL_ALIVE\n'` 得到独立输出。外部关闭测试会话时 CLI 退出码为 0。 |
| 重启持久性 | 测试会话关闭后停止默认 Agent，再运行 `scripts/start-local-agent.sh`；原设备身份从私有文件恢复，`Local Desktop` 在 Server 设备列表中显示 `online=true`。 |
| Android x86/API 25 真实 LAN | `MobileWorkflowTest` 1 项通过，`passed=true`、`git_status_survived=true`、Tab 补全和后台恢复通过。手机从 `192.168.0.36:7200` 连接新的 `Local Desktop`，路径为 `relay`，`adb reverse --list` 为空。日志：`build/default-agent-stability-android.log`、`build/default-agent-stability-android-results.json`。 |

Server 保持运行；先前独立的 `Terminal Fix Desktop` Agent 与其用户 Shell 保留。iOS/Android App 代码本次没有修改，前轮 iPhone 15 真实服务全流程仍见 [并行输入结果](../0924-terminal-shared-input/RESULTS.md)。本次测试 Shell 已关闭；随后在默认 Agent 预置了项目根目录的新 Shell `8df358908a56eb55`（120 列），手机选择在线 `Local Desktop` 即可打开，桌面可用 `--attach 8df358908a56eb55` 连接。原命令仍可创建另一条新 Shell。
