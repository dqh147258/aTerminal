> 本轮浏览器更正：实际加载的 workspace.css?v=17 后部 :root 覆盖了旧配色，最终 bg=#090d16、surface=#101623、accent=#38bdf8。下文“主题已经匹配”的判断仅对应旧交接文档，应以永久图集和 PLAN.md 为准。

# Android 设置与最新原型只读审计

- 日期：2026-09-30
- 子任务：20260930-230455-025-ui-settings-audit
- 源码基线：`749a5b2fc80130d03175c224b191a0d71578865d`，工作树 `0930-ui-settings-audit`；开始时 git status 干净。
- 本报告是协调者拟定新 UI 计划的证据输入，不是已批准执行计划。仅阅读源码/测试；未改源码、构建、运行测试、浏览器、服务或模拟器，未提交/合并。
- 已按 worktree-tasks 登记；阅读 plan-complex-tasks，遵守批准前只整理证据的边界。实施审批由协调者根据本次证据另行发起，旧 OpenRouter/wait 批准不覆盖新 UI。

## 结论

Android 的真实功能与基础布局已较完整，本轮宜集中重构设置的信息层级和表单承载，复用配置 RPC；不应按旧文档从头重做聊天、图片链路、统一侧栏或全局会话。

主要改动可收敛到 `NativeUi.kt`、`MainActivity.kt`、`AgentSettingsPanel.kt` 和针对设置的新回归测试。配置协议、Rust 运行时、终端渲染器、草稿与历史存储原则上无需改变。

## 设计依据与必须解决的冲突

原型根目录：`/Volumes/Code/OpenDesignProjects/4c6b6741-85d2-4ded-b4c0-37377166a5c9`。已读 `UI_IMPLEMENTATION_GUIDE.md`、`assets/settings.js`、`assets/workspace.js`、`assets/workspace.css`；未用旧 `app.js`。

1. **交接文档与当前运行时代码确实不同步。** 文档第 5 节要求 Session/Global 分段直接切换、Global 仍显示终端路径；当前 `workspace.js:25–44,161–231` 已有独立全局助手列表、多个会话、未读、返回列表和 Desktop 上下文。`AgentPanel.kt:105–131`、`MainActivity.kt:988–1028` 已匹配新结构。不要把缺少分段切换判为应修 bug。协调者应以本轮实际浏览和永久图集明确新 UI 计划的优先级。
2. 文档写全屏半透明；当前 CSS `.detail-panel{background:var(--bg)}` 为不透明详情背景，Global 也为独立不透明页。设置首页/Session 浮层与编辑详情的透明策略需在图集索引逐页标明，不能以统一 alpha 机械替代全部页面。
3. 原型校验只是演示，**不能直接复制为真实校验**。例如原型接受 context>=1、max_tokens<=context、top_p=0；实际 `crates/agent-runtime/src/config.rs:217–249` 要求 context 4096..4000000、0<max_tokens<context-2048、0<top_p<=1，并结合供应商能力验证采样与思考模式。
4. 登录服务地址交互以用户本次要求保留真实 App；不照搬原型登录字段/演示 hash。`MainActivity.kt:224` 现有登录流程不列入本轮重做。

永久视觉参考由协调者采集至 `/Volumes/Code/My/aTerminal/doc/task/0930-app-settings-redesign/reference/`。本子任务不生成重复截图；报告中的外观判断为源代码比对，不冒充截图或真机验证。

## 已匹配、应直接保留的部分

| 部分 | 已有证据 |
|---|---|
| 主题色 | `NativeUi.kt:13–25` 完整匹配雾蓝/石墨灰及状态 token，正文/输入为 sp；不需要另换主题体系。 |
| 设置导航骨架 | `MainActivity.kt:918–945,1030–1043` 已叫“设置”，已有显示、AI 与工具、工作空间分组，LLM/读取/MCP/Skills 独立入口，详情返回设置。 |
| 浮层与终端 | `MainActivity.kt:948–986` 除特殊按键外全屏，背景单独 alpha，终端 View/连接留在背后；显示字号 6–24、默认 16，透明度 0–100、默认 88。reset 仅作用显示。 |
| 配置读写 | `AgentSettingsPanel.kt:23–35` 使用 show/replace、expected_revision、secrets；服务端 revision conflict 后刷新提示。保存提示是“下次任务生效”。 |
| 供应商 | 同文件 92–111：新增/编辑/启停；六协议；Azure API version；密钥空白不写 secrets，已有凭据不被清空。 |
| 模型 | 113–199：配置 ID、供应商、model/deployment、context、max tokens、temperature/top_p、tools/vision、reasoning levels/budget/adaptive/disabled，五种思考模式，保存后绑定；高级区已有折叠。 |
| 目录 | 139–162：真正 discover RPC，search/cursor/refresh、分页、手填模型 ID；目录选择会带回可用 metadata；更换模型/供应商清理显式思考及能力控件。 |
| 绑定 | 54–68：global、session-default、session/<id>；当前终端可删除覆盖以继承默认。模型改名/换供应商时移除引用绑定中的旧 reasoning override。 |
| 读取 | 40–50：首尾 1–100、10/20 默认，保存服务端设置；动态 TUI 搜索过滤且原文保留说明。 |
| MCP | 76–80,200–205,252–267：内置终端只读；导入 JSON、查看/编辑、启停/删除、真实错误反馈。 |
| Skills | 82–86,207–250,254–258：Desktop 绝对路径安装；Android SAF 完整目录分块上传；SKILL.md 读取编辑、启停/删除；内置只读。限制为 256 文件、单文件 2 MiB、总 8 MiB、深度 8，不是原型仅保存一个 Markdown 副本。 |
| 账号设备 | `MainActivity.kt:847–916` 已有在线设备列表、刷新、连接 Desktop、移除确认、改密码、退出确认、旧版配对；只显示在线且非本机是现有业务/测试约定。 |
| 聊天核心 | `AgentPanel.kt` 已有无模型/权限行的紧凑输入、16dp 图标和44dp按键、send/append/cancel 同位置、真实图片上传/粘贴/查看、草稿身份隔离、分页与证据；不要重复“补齐图片协议”。 |
| 全局助手/侧栏 | `GlobalConversationPanel.kt`、`GlobalConversationStore.kt`、`MainActivity.kt:706–824,988` 已有独立多Global会话；会话侧栏按 device+session 去重，显示真实可用/离线/关闭状态，搜索/刷新/新建/关闭。 |

## 设置优先改动：具体缺口与最小方案

### P1 设置首页和 LLM 首页

- 当前首页每一入口是单行 `actionButton`，缺图标、主副标题、右箭头和分组连贯表面。原型 `workspace.js:138–140` 与 `workspace.css:11` 是 64px 起的信息行、16px 主文字/12px 辅助、16px 页面边距、24px组间距。新增设置专用 row/group 小 helper 即可，不宜全局改 `actionButton` 影响聊天/登录。
- 显示区原生滑块标题和数值拼在同一 TextView，区块零散；原型是一个显示卡片，标签/右对齐数值、两滑块。恢复默认位于底部固定区（确认图集），原生目前在滚动内容中。只调整结构，不改变 display 存储、zoom、不透明度语义和远端列数。
- LLM 首页原生先平铺三个默认绑定按钮，再供应商和模型按钮；原型 `settings.js:26–30` 是当前终端有效模型摘要卡→供应商信息行→模型信息行→单个“作用域绑定”入口。摘要按真实 `session/<id> > session-default` 解析，并把未绑定/供应商禁用如实呈现，不能写演示 gpt-4.1。
- 供应商行应展示协议（可用友好名称），模型行展示实际 model、配置 ID 和 provider；MCP/Skills行展示 enabled 状态；空列表给空态。标题不可只有内部 ID。
- 文件：`NativeUi.kt`、`MainActivity.settingsPanel()`、`AgentSettingsPanel.render()`。

### P1 全屏详情表单与导航

- 当前 provider/model/目录搜索/导入/Skill编辑均 `AlertDialog`；原型为 full-page 详情，标题返回、取消/保存、滚动字段和行内错误（`settings.js:18–24`）。仅保留危险操作确认弹窗和系统文件选择器。
- 最小做法：保留同一个 `AgentSettingsPanel` 实例、worker与配置 snapshot，在面板内部记录当前列表/表单/目录/绑定/扩展详情目的地；复用现有 body 内容，不为每个表单重新调用 MainActivity.panel() 或重新连接远端。面板提供返回当前父页的 handler，MainActivity 的系统返回也先让设置面板消费。
- `MainActivity.onBackPressed():1151` 目前 setting-detail 一律返回设置，无法表达“编辑模型→LLM”；关闭仍关闭整个浮层。账号入口从设置进入时当前返回直接关闭，新增来源信息让其回设置；从侧栏进入则保持合适的原路径。
- form 容器20–24dp分组、明确 label、48dp起输入、并排取消/保存，键盘压缩滚动区；大字体不能截掉底部保存/错误。仅把尺寸作为视觉基准，保持触控与可访问性。

### P1 保存与错误体验（有真实数据丢失风险）

- `setPositiveButton` 自动关闭：provider/model/MCP/Skills 表单在本地解析失败或异步RPC失败前已消失；model 的 catch 只把错误写到背景 status。应先本地校验，失败留在表单并聚焦首个错误；请求期间禁用重复保存，成功才返回/刷新，失败保留非敏感草稿。
- provider API key 当前提交时立即 `key.setText("")`，这是现有避免长期保存凭据的行为。重构不能把密钥放 SharedPreferences/文件/日志/截图；可仅在活跃编辑内存保留用于重试，离开时清除，并用“留空保留现有密钥”准确说明。
- 新增 provider/model ID 当前没有重复检查，可覆盖同ID配置；新增应拒绝重复，编辑ID维持只读。不要直接替换完整旧对象：旧 provider/model 里还有 UI 未暴露的字段，需要 copy 后变更相关字段。
- Azure field 当前只隐藏 EditText，label 留在页面（provider:100–103）；将 label+输入作为整体条件字段。协议 Spinner当前无独立 label，补标签与友好显示名但保持 wire value。模型 strength 应仅 level/budget 显示，采样/能力等低频项集中高级区；不能把 tools 能力误删为运行权限开关。
- 客户端仅预校验明确的真实约束并把服务端错误映射至字段；服务端仍权威。参考 `crates/agent-runtime/src/config.rs:132–264,300–357`，避免复制原型更宽的限制。特别是思考预算受 max_tokens 和协议约束。
- 冲突仍带旧 expected_revision，请求失败后 show 刷新；显示“配置已变化，请检查后保存”，保留可回看的草稿，需明确再次提交基于新快照，不能悄悄自动重试覆盖。当前只按异常 message含 revision识别，与本轮重构兼容即可，无须扩展协议。
- 页面请求/目录结果绑定发起时 provider、编辑对象和身份；离开/切设备后忽略迟到回调。继续现有 `MainActivity.agentSettings()` 捕获设备、账号、generation 的校验，不为了表单路由绕过它。

### P1 绑定、读取、MCP、Skills

- 绑定：从三个即时保存弹出列表改为独立表单，一次保存/取消。显示模型名+ID；Global默认、Session默认可“未绑定”，语义应删除binding key，不传空model_id；当前终端覆盖“跟随默认”也删除key（原型空字符串格式不是真实协议）。无当前会话时隐藏/禁用该覆盖项，不造 session/ 空键。
- 读取：保持现有字段和RPC，将错误从页顶概括提升为对应字段；保存成功返回设置或留页显示成功，按图集定；取消不能发RPC。
- MCP：内置行锁图标不可修改；用户扩展详情展示状态、启停、查看/编辑、删除确认，替换当前四选项菜单。JSON错误保留文本；维持导入合并语义和真实服务端schema，不擅自改变 credential references/transport。
- Skills：同样状态详情和删除确认；Desktop安装、手机目录导入保持两条真实流程。Desktop path不是可随意编辑已注册Skill的地址；编辑应复用 skill_read/skill_edit 的 SKILL.md 语义。文件夹上传保留全部文件与分块校验，不复制原型只读SKILL.md；不得用“本地演示保存成功”替代commit结果。
- extension 当前删除无确认（252–267）；原型有确认。增加该确认有依据，但不要为普通保存加批准步骤。

### P2 账号与设备

`MainActivity.accountPanel()` 当前姓名/服务器文字与两枚大按钮加设备堆叠；原型是头像概要、设备卡片/状态/图标、刷新与密码入口层级。仅改展示和返回路径，保留只显示在线非本机的过滤、Desktop连接限制、revoke/logout/password真实API、确认与账户epoch防串线。原型设备列表数量/名字不可复制。

## 其它 UI 差距与审慎范围

- **聊天重复输入标签**：`AgentPanel.kt:149–175` 在输入行上方常驻“发送任务或追加消息” TextView，虽已无模型/权限行，仍占一行；交接文档紧凑要求和原型 composer 不需要这行。可删除视觉标签，保留 EditText.contentDescription，兼顾小高度逻辑，不影响发送状态机。
- **聊天标题**：原生 Session 的 conversationTitle取初次路径最后一段，而原型取current.name；path本身会更新。是否需要改标题取真实 session名称，取决于当前协议是否有独立名称，不伪造。Global显示Desktop和状态目前符合实际原型，应保留。
- **侧栏**：已有统一列表、选中、当前目录、设备、状态及关闭按钮，已很接近新原型，不推荐重写。文档要求旧手机只读归档入口，但当前 openDrawer没有该入口，文档提及的 `LegacyAgentHistory.kt` 在本基线也不存在；是否仍有需要兼容的旧存储需先核实，再决定本轮是否补回，不能删除兼容数据。原型列表同样不应成为伪造真实会话状态依据。
- 全局助手多会话未读、Markdown、工具详情、图片链路已有实现与测试。当前次任务可只做必要视觉对齐；不重开协议/缓存工程。

## 最小执行顺序（供协调者拟计划）

1. 永久图集逐页索引明确首页/编辑页/扩展详情/账号页及Global新结构，记录文档冲突的决策；保留真实登录服务地址。
2. 添加设置专用信息行、分组、标签和表单动作组件，重排设置首页/LLM首页；不改通用聊天控件默认。
3. AgentSettingsPanel内实现有父页的全屏表单和绑定编辑，接系统返回；复用所有现有RPC/配置字段。
4. 逐项迁移 provider/model/目录/读取/MCP/Skills，落实输入校验、busy、成功返回、失败留表单、revision conflict。
5. 账号设备呈现及返回路径；仅在批准范围内去除聊天重复输入标签、补其他图集中明确的小差距。
6. 运行下述针对性验证并保存Android同场景截图与原型图集对照；iOS留后续。

## 必要验证与既有覆盖

此审计未执行任何测试；以下是建议实施验证，不是通过记录。

- 新增一个 `AgentSettingsUiTest.kt`（或同等现有测试扩展），用注入request回调+受控JSON+隔离Activity，无公网模型/真实凭据：覆盖列表→表单→取消/返回、重复ID/无效输入不发送、保存成功才返回、RPC失败保留内容、busy防重复、Azure标签整体隐藏、切模型清理能力/思考、discover分页和旧provider结果不污染新编辑。
- 同测试检查 wire payload：expected_revision、密钥空白不写secrets、旧配置未暴露字段保留、绑定继承/未绑定删除key、MCP/Skill动作准确、revision conflict刷新且不自动覆盖。可按风险拆成少数测试，不对每个颜色/控件堆镜像断言。
- `WorkspaceUiTest.displayPanelPersistsResetsAndSurvivesRotation`、`accountPanelShowsOnlineDevicesWithoutStaleOfflineRows` 已存在，可扩展设置深层返回/字号极值/大字体和小屏键盘可达；保持终端实例/选中会话/远端列数不变。
- `AgentReadingUiTest.readingSettingsAndAgentEvidenceRoundTrip` 已通过代码覆盖真实 encrypted RPC 读取参数→Agent anchors、TUI原文、图片→HTTP模型、独立Global。迁移后调整控件查找，针对读取和配置链路运行一次既有隔离fixture；不要把所有终端业务端到端测试重跑当作UI验收替代。
- 如改聊天结构，运行 `MobilePrototypeTest`（紧凑工具/详情）、`WorkspaceUiTest.chatComposerResizesForNativeKeyboard`、`WorkspaceReviewRegressionTest`（迟到send/缓存更新）与 `GlobalAssistantTest`（入口返回/多会话隔离）相关用例；未改则无需扩张测试范围。
- `DisplayConsistencyTest` 核心是终端渲染，不是设置表单覆盖；纯设置改动通常不需全套跑。已有 `crates/desktop-agent/src/config.rs` 的 revision原子恢复/旧snapshot测试可复用，只有共享配置校验改变才补跑Rust相关测试。
- 实施后Android编译/测试APK构建，加设置各页截图、IME、小屏/字体缩放、0/88/100不透明度。实际截图永久存reference的android子目录或同任务明确目录，索引注明设备、分辨率、密度、字体缩放、代码commit、状态与是否fixture。

## iOS 后续参照（本次不实施）

- `WorkspaceScreen.swift:139,193,332–364` 仍是“终端设置”部分高度浮窗，只包含显示；后续需设置首页/全屏承载与同一图集导航结构。
- `ChatPanel.swift:148–210` 现有 AgentConfigView把读取/供应商/模型/MCP/Skills混在聊天设置Form中，已有真实 show/replace/expected_revision、skill上传与编辑，不应再造后端。
- `AssistantModel.swift:94`→`aTerminalApp.swift:405–415` 已支持 configuration RPC；后续将配置入口从聊天拆至设置、增加独立绑定/表单路由，保持scope和身份校验。
- `WorkspaceStyle.swift` 对照同一token；图片/Global/聊天导航完整差距应作为iOS专门审计，不能仅凭Android已实现就宣告两端一致。首轮截图图集是统一视觉参考，后续iOS也需相同场景截图。

## 交付边界

报告可被协调者引用/迁入正式计划。无源码变化，无测试“通过”声明，无提交。新的UI实现仍待协调者根据图集和本报告拟定具体方案并取得用户审阅；本子任务的只读审计已完成。
