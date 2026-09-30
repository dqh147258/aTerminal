# Android 实现与统一设计对照

- Capture date: 2026-10-01
- Device: `aiterminal_api36_test` / `emulator-5586`, Android 16
- Resolution: 1080 × 2340, density 440 dpi, font_scale=1.0
- Code baseline: `635851e`，另含协调者补齐的 ATerminalTheme 和 aTerminal 品牌文案；最终提交见 RESULTS.md。
- 正常页面使用真实本地测试账号和 Desktop，模型仍为 OpenRouter Space Bunny；仅登录截图使用 isolated_ui，不退出正常账号。终端 80×25，字号/不透明度保持既有 11 sp / 73%。
- PNG 保存在本任务 `android/`，不会依赖临时目录或浏览器缓存。参考原型保存在 `reference/`。

| Android 截图 | 页面 / 对应设计 | 核对结果 |
| --- | --- | --- |
| [01-login](android/01-login.png) | 登录 | aTerminal 名称；现有服务器摘要/修改入口；最新蓝色风格 |
| [02-login-server](android/02-login-server.png) | 登录服务器编辑 | 现有展开编辑/保存方式；只改视觉，不照搬旧原型 |
| [03-terminal](android/03-terminal.png) | 01-terminal | 实际连接、同一PTY、远端列数保持 |
| [04-settings](android/04-settings.png) | 02-settings | 显示卡片、图标信息行、固定恢复/自动保存；保留当前显示偏好 |
| [05-llm](android/05-llm.png) | 03-llm | 实际有效模型、供应商/模型分组 |
| [06-provider](android/06-provider.png) | 04-provider | 全屏编辑、ID只读、空密钥留原凭据、固定取消保存 |
| [07-model](android/07-model.png) | 06-model | 模型标识、目录、高级参数折叠、思考与绑定 |
| [08-model-advanced](android/08-model-advanced.png) | 07-model-advanced | 真实能力和数值显示，内容可滚动 |
| [09-llm-bindings-entry](android/09-llm-bindings-entry.png) | 03-llm | 独立作用域绑定入口 |
| [10-bindings](android/10-bindings.png) | 10-bindings | 真实Global/Session默认与当前终端继承 |
| [11-terminal-reading](android/11-terminal-reading.png) | 11-terminal-reading | 10/20实际值、校验/TUI说明 |
| [12-mcp](android/12-mcp.png) | 12-mcp | 内置只读与空态 |
| [13-mcp-import](android/13-mcp-import.png) | 13-mcp-import | 全屏JSON编辑、取消保存 |
| [14-skills](android/14-skills.png) | 14-skills | 内置只读、Desktop与手机文件夹入口 |
| [15-skill-install](android/15-skill-install.png) | 15-skill-install | 完整Desktop路径安装全屏表单 |
| [16-account-devices](android/16-account-devices.png) | 16-account-devices | 实际在线设备、图标操作、返回设置来源 |
| [17-workspace](android/17-workspace.png) | 17-workspace | 正常在线/关闭会话统一；无旧手机归档 |
| [18-session-chat](android/18-session-chat.png) | 18-session-chat | 真实SVG任务历史、当前路径、原作用域草稿与输入保持 |
| [19-global-list](android/19-global-list.png) | 19-global-list | 真实独立多会话结构与状态 |
| [20-global-chat](android/20-global-chat.png) | 20-global-chat | 真实Global历史、Desktop上下文与返回 |

后续 iOS 同步时使用 UI_REFERENCE.md 的同一页面结构及这里的实现语义；旧手机归档、旧服务器栏与旧 AI Terminal 品牌不采用。

## 已执行的真实扩展功能

LiveSettingsExtensionsUiTest 在正常账号生产表单完成 MCP 导入/编辑/启停/确认删除，以及完整 Desktop Skill 安装/读取/编辑/启停/确认删除；非 Markdown 资源 SHA256 在编辑前后保持。只清理本轮 UUID 条目，providers/models/bindings/credentials 等配置完整哈希保持不变。结果 evidence/live-extensions.txt。系统 SAF 文件夹上传本轮未单独走实际文件选择器，现有完整上传代码保留。

小屏/IME压力截图：[21-small-font-ime.png](android/21-small-font-ime.png)。800×1600、font_scale=1.3，取消/保存完整可见，键盘已确认打开；原设置已恢复。
