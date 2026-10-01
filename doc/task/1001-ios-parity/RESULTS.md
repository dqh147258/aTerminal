# iOS 功能与界面同步结果

2026-10-01。实现、集成及功能验收完成；最终源码范围为 `10ecef6`，后续提交为审查与验收文档。

## 已同步

与当前 Android 共用深蓝/亮蓝样式、设置分组、全屏配置编辑和固定取消/保存区域。包含供应商、Azure 条件字段、模型参数与能力、目录搜索分页、Global/Session 默认及当前终端覆盖、终端读取、MCP、完整 Skills、账号设备入口、统一工作空间与正常 Agent 正文搜索，以及独立 Global 多会话和 Session 对话。保留最近工作目录选择、现有登录服务器摘要/修改、正常历史、草稿、图片、证据和原有凭据；移除旧手机归档入口及自动导入，不删除历史文件。

配置采用生产 RPC 与修订号，保存单飞、取消和连接身份屏障、失败草稿保留、空/纯空白密钥保留、冲突刷新后明确重试。模型能力改变会保留兼容的绑定思考参数并清理不兼容项。内存会话快照显式绑定 server/account，关闭与离线历史只读。MCP/Skill 使用原生纯文本编辑器，禁用智能引号/破折号，提供一致的清空操作；所有下拉框有可见名称及当前选项的可访问性值。

## 实际验证

| 范围 | 结果与证据 |
| --- | --- |
| 17 个 fixture UI 场景 | 全部最终通过：完整批次 14 个通过，3 个修复后定向复测通过；后续协议/模型/绑定标签回归也通过。原失败及复测分开保留，未把失败批次标为全部通过。 |
| 3 个真实 Desktop 场景 | 连接及只读设置路由、UUID MCP/Skill 生产表单、专用 PTY 输入通过，无 skip。输入覆盖字符、回车、删除、Tab 和 Ctrl-C。Tab 首次因隔离的 `zsh -f` 没加载补全而失败，启用 `compinit -D -i` 后复测通过，不改用户 Shell 配置。 |
| 小屏及键盘 | 独立 iPhone SE / iOS 17.5 的大字体、横屏键盘和 busy 保存操作通过；最后可见标签改动后再次验证大字体/横屏通过。 |
| 真实配置完整性 | 观察修订 20→30 共 11 个状态；MCP 导入、timeout 编辑、启停和确认删除；完整 Skill 安装、Markdown 编辑、启停和确认删除。两个不可变版本的文本及二进制资源 SHA256 一致，无关配置 SHA256 一致，两个 UUID 条目已删除。 |
| 生产逻辑 | 直接链接实际 Swift 源码的 10 组配置检查、2 组正文缓存搜索、绑定思考兼容性、账号快照隔离及现有 ChatStore 检查通过。未复制实现替代测试。 |
| 构建 | x86_64 Debug 模拟器 App/XCTest 与 arm64 Release iOS App 构建通过。Release 仅有既有静态库的重复 debug-map 对象警告，无编译或链接失败。 |

20 个独立场景的最终状态见 [test-summary.json](evidence/test-summary.json)，真实配置及资源观察见 [live-observations.json](evidence/live-observations.json)。完整 xcresult 和构建日志留在主 checkout 的忽略目录 `build/ios-parity-*.xcresult` / `build/ios-parity-*.log`。

## 界面对照

按 [共用 20 页参考](../0930-app-settings-redesign/UI_REFERENCE.md) 保存 [01–20 页截图与指纹](evidence/manifest.json)。普通页面设备为专用 iPhone 15 / iOS 17.5 / 393×852 pt；小屏为 iPhone SE / 375×667 pt。全部图集为隔离 fixture 数据，真实 RPC 结果单独记录。协议、模型与绑定截图已在最后可见标签修复后更新，键盘场景展示固定保存区域。保留 aTerminal 品牌及现有服务器摘要交互。

## 验证边界与运行状态

真实测试没有修改供应商密钥、模型绑定或账号密码，没有撤销用户设备，没有向用户或动画 PTY 输入。iOS 实际 LLM 消息/图片发送、MCP 工具调用、系统文件夹选择与手机上传未在本轮执行；完整 Desktop Skill 包已经真实 RPC 验证，文件枚举/大小/符号链接/二进制保留另有生产逻辑检查。模拟器测试不等于真机运行；arm64 为构建验收。

原动画 Session `bb62538b3469573d` 与共享 Desktop/server/Android 保持运行。本轮专用输入 Session `6aed449b3e74a86d` 与专用 iPhone 15 模拟器保留供查看；临时登录文件已删除，凭据未进入 Git、截图或命令参数。任务终端、Worktree 和分支清理见 [CLEANUP.md](CLEANUP.md)。

## 独立审查

实现和验收通过 worktree-tasks 分开执行，重要问题及修复记录见 [REVIEW.md](../1001-ios-parity-checks/REVIEW.md)。早期测试集成提交保留 `[未Review]` 历史标记；最终审查结果以该记录为准，测试通过与代码审查分开记录。
