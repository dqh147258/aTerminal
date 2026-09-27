# Desktop 终端滚动、历史、清屏与颜色修复 TODO

- Status: Completed
- Updated: 2026-09-27

## Checklist

- [x] 实现宿主颜色适配和屏幕 guard，验证颜色与恢复合同
- [x] 实现有界带样式历史快照、本地 RPC 与资源回收，验证状态隔离和授权
- [x] 接入 CLI 滚轮、分页、备用屏和只读浏览，验证输入路由
- [x] 完成隔离 PTY 回归、受影响包测试与 Clippy，更新使用文档和交付记录

- [x] 增加 CLI 内部滚动条、拖动与轨道定位，验证应用鼠标和显示隔离
- [x] 统一不可变历史分页合同并安全开放远程只读，验证跨页与权限
- [x] 接入 Android/iOS 分页历史及 FFI，验证加载顺序、范围和生命周期
- [x] 重建移动与 Desktop 产物，完成追加回归并更新交付文档

## Verification evidence

- Engine/Protocol/Agent/CLI 完整测试：69 passed，1 ignored（既有真实模型凭据测试）。
- 新增真实双层 PTY 验证滚轮、分页、彩色中文、后台持续输出、输入一次、备用屏切换、detach/reattach、只读与 resize；最终 CLI/PTY 定向检查 14 passed。
- `scripts/test-host-terminal.py` 五组（sh/256、zsh/RGB、Vim/256、ANSI、启动失败）全部通过，含 termios 与宿主恢复。
- `cargo +stable clippy --locked -p ai-terminal-engine -p ai-terminal-protocol -p ai-terminal-agent -p ai-terminal --all-targets -- -D warnings` 通过。
- `cargo +stable fmt --all -- --check`、`git diff --check` 通过；`cargo +stable build --locked -p ai-terminal --bin aTerminal` 成功。
- 10,000 行边界通过；debug 本机 80 列副本约 31.6 MiB，单次捕获约 222 ms。
- `deploy/LOCAL-DEBUG.md` 已更新。GUI 与现有 Agent 升级边界见 HANDOFF；实现/测试明细见 RESULTS。

### 追加验证证据

- 五包完整测试 83 passed、1 ignored；最终额外恢复测试包含在 CLI 13 passed 与 Desktop PTY 2 passed 中。
- 真实只读远程配对分多页读出 450 行编号输出，首尾/顺序/数量一致；仍不能输入。200/450/10100 行冻结分页和追加输出回归通过。
- 内部滚动条真实 PTY 拖到顶部/底部、拖出轨道、悬停点击；单测覆盖短轨道、宽字符与隐藏后恢复，均通过。
- Rust 五包 Clippy、fmt、diff check 通过，Desktop binary 已更新。
- Android 三 ABI WebRTC 库、FFI、Debug APK、测试 APK、Lint 构建成功。
- iOS aarch64 设备/x86_64 模拟器共享库和 XCFramework 成功；使用 `/tmp/aterminal-history-xcode` 成功构建模拟器 App，保留 Server URL/公共 CA 配置。
- 未安装用户设备、未自动停止正常 Agent。明确的更新和 GUI 确认说明已写入 HANDOFF。

### Android 主画面回归

- [x] 接入带样式历史视口 FFI 与实时/回看隔离，验证会话和过期边界
- [x] Android 垂直手势与历史视口衔接，保留横向、输入和复制
- [x] emulator-5586 隔离实际手势验收、Android 构建与共享核心回归

2026-09-27 重启后复验：`TerminalScrollTest` 1 项通过（触摸历史、持续更新、横向、输入一次、鼠标滚轮、账号保留）；`DisplayConsistencyTest` 3 项通过；`mobile_core_reads_types_and_cannot_override_readonly_pair` 通过；fmt/diff check 通过。结果和截图归档 `.local/mobile-scroll-check/reboot/`。最终 APK 已安装 emulator-5586，模拟器保持运行，App 已回到正常启动入口。

2026-09-27 提交前工作区审查已完成：修正 Android 拖动跨网格跳行、迟到历史视口恢复旧尺寸，以及响应 view ID 校验；85 项 Rust 测试、5 项 Android 设备边界/绘制测试、完整手势测试与两端构建通过。详情见 [REVIEW.md](REVIEW.md)。
