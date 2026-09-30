# Android / iOS 共用 UI 参考

- Source: `/Volumes/Code/OpenDesignProjects/4c6b6741-85d2-4ded-b4c0-37377166a5c9`
- Captured: 2026-09-30，Playwright CLI / Chrome，CSS viewport 480 × 1000，deviceScaleFactor=1。
- 本图集保存为项目文件，不在 `.local`、临时目录或浏览器缓存中。Android 先实现；iOS 后续对照同一图集。
- 浏览器模拟状态栏/Home 条/设备外壳只用于原型展示，原生不重复绘制。演示账号、模型、终端数据不属于真实服务配置。

## 本轮权威依据

1. 用户本轮要求和此目录实际运行的 HTML/最终 CSS；旧 UI_IMPLEMENTATION_GUIDE.md 冲突内容不采用。
2. 当前计算色值：背景 `#090d16`，表面 `#101623`，控件 `#141c2c`，强调 `#38bdf8`，文字 `#f8fafc`，辅助 `#8391a7`。最终 CSS 含蓝色渐变主按钮、细分隔和分组卡片。
3. Global 为独立列表/多会话页面，保留现有实现；不回退成旧 Session/Global 切换器。
4. 登录服务器地址相关交互保留真实 App：默认地址摘要与可编辑入口；原型此处过时。其他 UI 以本图集对齐。
   用户进一步明确：服务器栏只统一视觉风格，现有摘要/修改交互保持。
5. **旧手机归档不实现，旧手机数据不导入/迁移。** 即使原型/截图中出现“旧手机归档”，也必须忽略该入口；已有当前账号的正常 Agent 历史仍属真实功能。
6. 页面结构/布局参考原型，保存、凭据、权限、目录、校验与配置修订必须接真实服务；不复制原型宽松校验或“演示保存成功”。
7. **产品名使用 `aTerminal`。** 用户明确已从 AI Terminal 改名，不照搬原型旧品牌。Android 和后续 iOS 均以该名称显示。

## 页面索引

| 截图 | 页面 | 点击路径 | 实现重点 |
| --- | --- | --- | --- |
| [终端主页](reference/01-terminal.png) | 终端主页 | terminal.html → index.html#terminal | 真实终端尺寸与独立工具入口 |
| [设置首页](reference/02-settings.png) | 设置首页 | 终端 → 设置 | 显示卡片、信息行、固定恢复入口、全屏半透明 |
| [LLM 大模型](reference/03-llm.png) | LLM 大模型 | 设置 → LLM 大模型 | 当前有效模型摘要、供应商/模型分组、独立绑定入口 |
| [编辑供应商](reference/04-provider.png) | 编辑供应商 | LLM → openai | 全屏字段、取消/保存、密钥为空保留 |
| [Azure 条件字段](reference/05-provider-azure.png) | Azure 条件字段 | 供应商 → 连接协议 Azure OpenAI | API version 标签与输入整体显隐 |
| [编辑模型默认态](reference/06-model.png) | 编辑模型默认态 | LLM → 模型 | 配置标识/供应商/模型、折叠高级能力与独立思考模式 |
| [模型高级参数](reference/07-model-advanced.png) | 模型高级参数 | 模型 → 模型参数与能力 | 采样、能力声明、思考预算等渐进展开 |
| [模型目录第一页](reference/08-model-catalog.png) | 模型目录第一页 | 模型 → 浏览模型目录 | 搜索/选择与下一页入口，原型数据仅演示 |
| [模型目录分页](reference/09-model-catalog-page2.png) | 模型目录分页 | 目录 → 下一页 | 保留搜索和分页的状态 |
| [默认模型绑定](reference/10-bindings.png) | 默认模型绑定 | LLM → 作用域绑定 | Global、Session 默认、当前终端覆盖，继承独立 |
| [终端读取](reference/11-terminal-reading.png) | 终端读取 | 设置 → 终端读取 | 首尾行数表单及 TUI 过滤说明 |
| [MCP 列表](reference/12-mcp.png) | MCP 列表 | 设置 → MCP | 内置只读、已添加列表/空态、JSON 导入 |
| [MCP 导入](reference/13-mcp-import.png) | MCP 导入 | MCP → 导入 MCP JSON | 全屏代码输入、错误反馈、取消保存 |
| [Skills 列表](reference/14-skills.png) | Skills 列表 | 设置 → Skills | 内置只读与 Desktop/手机文件夹安装入口 |
| [Skill 安装表单](reference/15-skill-install.png) | Skill 安装表单 | Skills → 从 Desktop 安装 | ID、绝对路径、Markdown 内容、取消保存 |
| [账号与设备](reference/16-account-devices.png) | 账号与设备 | 设置 → 账号与设备 | 账号概要、设备卡片、连接/刷新/密码与退出层级 |
| [统一工作空间](reference/17-workspace.png) | 工作空间侧栏 | 终端 → 工作空间 | 统一在线/离线会话；旧手机归档入口按用户要求忽略 |
| [Session 聊天](reference/18-session-chat.png) | Session Agent | 终端 → Session 对话 | 当前路径、连续消息流、16px 图标和紧凑输入；当前 HTML 保留输入 label |
| [Global 会话列表](reference/19-global-list.png) | 独立 Global 列表 | 终端 → 全局AI助手 | 多会话、未读、状态、新建与返回，不采用旧分段切换 |
| [Global 聊天](reference/20-global-chat.png) | Global Agent | Global 列表 → 工作空间巡检 | 独立对话标题、Desktop 上下文与返回列表 |

## 统一验收与后续 iOS

Android 同场景实现截图保存在 `android/`，标注设备、像素尺寸、密度/字体缩放、代码 commit 与真实/fixture 数据。iOS 后续也使用相同页面编号与点击路径建立对照。对大字体/小屏可做原生适配，保持信息层级、全屏路由、输入错误和保存动作可达。

源文件及截图的 SHA256 记录于 `reference/manifest.json`，防止把更新后的外部目录误认作本轮参考版本。截图中的演示信息不作为真实功能结果。
