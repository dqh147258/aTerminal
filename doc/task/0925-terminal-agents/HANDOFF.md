# 验证边界与人工验收参考

- Updated: 2026-09-26
- 类型：实现后人工验收参考，不要求用户更新清单。

初始阶段使用临时数据库、临时 HOME、隔离 PTY、本地模型/MCP 桩和原生构建验证。没有安装/启动手机应用、连接真机、调用真实供应商或部署/重启用户现有 Desktop。原有 Linux/macOS/Windows CI 矩阵保留；未提交/推送触发远程 CI，因此 Linux/Windows 运行结果仍未知。

已验证：macOS Desktop 完整工作区回归；实际 bash/zsh PTY、人工输入/授权隔离与持久 Agent 路径；Android 三 ABI 与 debug/测试 APK、lint；iOS device/simulator 静态库和 x86_64 模拟器 App。编译成功不代表已做手机交互验收。

构建产物：

- Android：`apps/android/app/build/outputs/apk/debug/app-debug.apk`
- Android 测试包：`apps/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk`
- iOS 模拟器：`build/xcode/Build/Products/Debug-iphonesimulator/aTerminal.app`
- Desktop：`target/debug/aTerminal`

如需真机验收，先确认使用的设备与测试 Desktop，再运行以下用户场景：

1. 配置 Provider→选择模型→修改思考强度→保存默认/Session 覆盖。CLI 与 App 回读应一致，保存和目录浏览不产生生成请求；活动 Run 不换模型。
2. 当前终端发送“读取并解释输出”，再追加约束。检查工具原文/图片和历史；全局向两个 Session 委托时互不串写。停止不发送 Ctrl-C。
3. 移动端断线/关闭浮窗后任务继续；重连显示稳定 ID 的持久历史。用户在 Desktop 输入或 resize 时旧 Agent 停止写入。只读设备不能发送/管理配置。
4. 在超过 150 条历史中向下加载更早页；停留旧页时新消息不将页面跳回底部。执行历史清理后分页明确重置；离线显示缓存状态，重连后删除旧 generation。
5. 查看截图与同 revision 文本；中文、组合字符、光标和 alternate screen 内容应一致。截图是离屏终端渲染，并非宿主桌面照片。
6. 上传/编辑 Codex Skill，停用/删除后下一 Run 不再使用；内置项不可修改。旧手机归档是独立只读来源，源文件保留，不被上传到 Desktop。

用户随后提供了 ModelScope 配置并授权实测，`Qwen/Qwen3.8-27B` 的真实流式工具循环、分析、写入、UUID 回读和请求去重已通过，详见 [LIVE-MODELSCOPE.md](LIVE-MODELSCOPE.md)。成功运行 cached_input_tokens=0，不能宣称缓存命中收益。其他供应商、显式思考参数、真实视觉与真机交互仍待相应环境验证。

Windows 需重点验证 PowerShell profile/prompt 保留、进程组取消和私有 ACL；Linux 需验证 /proc 启动身份/cwd 与真实前台 pipeline。没有可靠环境时不要把 macOS 测试或移动原生编译替代这些运行验证。

默认数据根 `~/.aTerminal`。旧活跃 daemon 会被复用，不为迁移关闭其 Shell；迁移保留源。旧 AI 环境需要 `config import-legacy-env` 显式导入，新 Runtime 不自动启动旧监控。回退前保留数据库和配置备份，不自动重放旧动作。

ModelScope 已写入当前 Desktop，默认模型别名 `modelscope-qwen`。该轮仅在确认默认 Desktop 无会话/无活动 Agent 后载入了分析修复，没有关闭用户 Shell。

用户随后指定 `emulator-5586` 并授权本地测试。Android 16 的生产 Agent 面板已通过隔离账号/服务/PTY 的仪器实跑，覆盖首尾配置、加密 RPC、对话/历史及原文查看，主账号/连接数据保持不变；详见 [ANDROID-AGENT-UI.md](ANDROID-AGENT-UI.md)。Android 物理机、iOS 真机和 Linux/Windows 运行边界仍未覆盖。

移动端登录追加已完成：Android 六阶段跨进程登录/离线/登出测试及 Activity 重建回归通过，iOS 构建通过。iOS 用户侧可验证登录后终止重启、离线重启及退出后立即终止重启；预期分别为恢复身份、保留身份和保持未登录。本轮没有将 iOS 编译结果当作运行验证。详见 [MOBILE-LOGIN.md](MOBILE-LOGIN.md)。

自动连接与首帧首页已完成。Android 已覆盖 Desktop/Terminal 后上线、首帧绘制和持久登录；当前 emulator-5586 已连接用户原有 Terminal。iOS 同步实现并通过 build-for-testing，仍需对应平台运行验证。现场模拟器 TLS 转发异常经保留数据重启恢复，未修改宿主代理、证书或服务器地址。详见 [AUTO-CONNECT.md](AUTO-CONNECT.md)。

移动输入与布局已完成：首次进入不自动弹系统键盘，特殊按键用后收起，6–24 字号与 0–100% 背景不透明度，横屏侧栏、全屏 Agent 与横屏 IME 避让。Android 已做真实 PTY、IME、旋转和实际工作空间验证；iOS 同步代码与测试包编译通过，设备运行边界仍保留。详情见 [MOBILE-INPUT-LAYOUT.md](MOBILE-INPUT-LAYOUT.md)。
