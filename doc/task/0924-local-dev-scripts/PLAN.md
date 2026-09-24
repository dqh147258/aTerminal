# 本地一键启动与多平台构建

- Status: Complete with Server image build pending network availability
- Updated: 2026-09-24

## 目标与范围

提供一键启动/关闭真实 LAN 调试栈，以及任选一个或多个平台的产物编译脚本。复用 Desktop 和 Android 的既有测试账号设备身份，避免再次出现一个实体设备对应多个在线记录。保持 AI 交互为空壳。

## 当前状态与证据

本机 Server 使用 `192.168.0.36:7200`，Android x86 设备为 `127.0.0.1:62001`。规范 Desktop Agent 的状态目录为 `.local/local-dev/agent-next`，凭据目录为 `.local/local-dev/config-next`。先前重复在线设备由多个独立 Desktop Agent 造成；测试账号密码仅保存在 Git 忽略的 `.local/local-dev/account.json`。

## 方案与执行

- `scripts/build-artifacts.py` 接受 `desktop`、`android`、`ios`、`server`、`all` 的单选或多选；Android 覆盖 arm64-v8a/x86_64/x86，iOS 覆盖设备和当前 Mac 的模拟器。
- `scripts/local-dev-up.py` 启动既有 LAN Compose 服务，确保测试账号，构建 CLI，安装 PATH 命令，复用 Desktop Agent，安装 Android Debug App 并执行测试账号自动登录。
- Android Debug 专用入口复用普通 App 同账号身份，仅在首次登录时创建手机设备；临时密码文件由 App/脚本清理。
- `scripts/local-dev-down.py` 停止 App、规范 Agent、项目容器；保留账号、设备身份和数据卷。`deploy/LOCAL-DEBUG.md` 说明启动、关闭、账号和构建；README 仅保留文档链接。

## 验证

编译脚本的多平台 dry-run、非法平台检查、Desktop/Android/iOS 实际编译、Android 真机自动登录、重复启动设备 ID 对比、LAN health 与无 `adb reverse` 检查。Server 镜像构建进入 Docker 内的 Cargo 阶段，但 crates.io 索引无进展后中止；关闭脚本仅检查语法和命令路径，因为规范 Agent 中仍有运行的 Shell，不为测试将其终止。详细记录见 `RESULTS.md`。

## 风险与回退

停止 Desktop Agent 会结束它的 PTY 中仍运行的 Shell；执行前需确认其工作已保存。Android 自动登录仅在 Debug 构建且明确传入 intent 参数时启用。保留旧手动调试入口作回退；若需恢复，可重新安装上一个 APK 并使用现有私有凭据。

## 未决问题、歧义与确认

None.
