# 独立审查记录

**最终独立源码审查完成，无剩余阻塞。** 已审阅生产89fc011、1035586、2edd331、4400898、ae9b4cf、0e6980c、b2d3f3a、6dcec6b以及main10ecef6。R1–R12与后续原生输入、抽屉、高级展开、可见选择器标签及选中值语义均已修正。main590a4f9时生产源码字节与实现worktree一致；最后main10ecef6增加当前选项可访问性值，由独立子任务明确复核通过。pbx只加入必要源引用，签名设置未因实现子任务修改。helper由协调者main拥有，b4f8fa2的clear-first/单行验空路径已复核。测试提交7e55a62/b62dd8a由协调者集成；审查子任务未merge、未操作sim/服务/用户身份。设备执行与最终证据由协调者完成。

| ID / 优先级 | 具体代码与触发 | 影响 / 预期 | 证据 / 状态 |
| --- | --- | --- | --- |
| R1 / P1 | SettingsDraft.swift ConfigurationSession.run catch 只检查 active/serial；save 发出后 identity 的连接 epoch 改变，transport 再失败；或 revision failure 发 show 后重连 | 原连接错误/冲突状态仍写入当前设置页。catch 应具有与 success 一致的 valid barrier，释放 busy 但不消费旧 response/error | 已修正，direct闭页/重连/冲突show迟到测试PASS |
| R2 / P2 | ProviderDraft.value 使用 !apiVersion.isEmpty，Azure version="   " | 前端接受 Android 已拒绝的空白 version，产生无效保存/延迟反馈；应 isBlank 等价校验 | 已修正，纯空白version rejection PASS |
| R3 / P1 | AgentSettingsView.save 使用 provider.key.isEmpty 生成 secrets；key 只包含空格时发送凭据 | Android isNotBlank 会保留原凭据，iOS 会覆盖成无效空白 key；应复用 production helper，方便直接检查空/空白凭据行为 | 已修正，production ProviderDraft.secrets direct空/空白/非空检查PASS |
| R4 / P1 | fixture隔离与网络边界 | 禁normal cache/migration/network，保留正常身份命名空间 | 已解决：ChatStore.swift:5/25/32、PairingStore.swift:5/8；aTerminalApp.swift:140/148/187/210/250/278/285/295/302/322/475/492边界guard；无importLegacy调用；production与fixture/集成凭据路径分开 |
| R5 / P1 | cancel与Global epoch | 旧连接不能cancel/消费Global回调 | 已解决：AssistantModel.swift:113–116连接改变更新globalEpoch；cancel:263–271捕获connection与epoch；Global list/create:416/422–428/434/439–444每页/回调connection屏障；pending集合defer释放 |
| R6 / P2 | permission Toggle提交状态 | 禁提交中改requestID | 已解决：ChatPanel.swift:95随submitting/只读禁用 |
| R7 / P1 | 统一正常历史列表 | 当前sessions+跨Desktop snapshots+正常archives | 已解决：WorkspaceScreen.swift:222–246按完整scope合并，:268–270正常history入口；AssistantModel:119–131持久metadata/闭离线fixture，:278–298服务archive同步。新增fixture.closed/offline可达性与草稿隔离UI检查 |
| R8 / P1 | Session历史只读 | 闭合/exited/detached/离线禁止send/stop | 已解决：AssistantModel.swift:86–100 writeReason用于canSend/canCancel；ChatPanel:51/54/95/108显示原因并禁用。新fixture测试输入非空draft后仍断言send/stop/permission禁用 |
| R9 / P2 | Workspace正文缓存搜索遗漏 | 正文独有词找不到正常会话 | 1035586已修：AgentCacheSearch只读pages/current generation/最多3页、scope白名单，正文白名单不扫legacy/metadata；AgentSearchSession debounce/identity/query/serial屏障；UI新增cedar-body-only-731只匹配fixture-closed的检查 |
| R10 / P2 | 同ID能力改变后binding推理override失效 | Rust validate拒绝replace | 1035586已修：实际SettingsValidation.reasoning过滤不兼容override，保留兼容与unknown字段；本任务10组checks内新增同ID低等级移除、高等级保留回归 |
| R11 / P1验收阻塞 | AgentSettingsView.content/根VStack、ChatPanel等父accessibilityID覆写叶子ID | fixture首轮16项13fail，已加载按钮不能按合同查找 | 已用xcresult二进制AX snapshot证实：添加供应商=settings.llm.page、返回/刷新=settings.page、恢复默认=settings.home、停止/permission=chat.session。生产2edd331修复后协调者smoke4项3PASS，余Azure输入caret错误由本任务helper修正 |
| R12 / P1身份边界 | 无owner的sessionSnapshots可能跨账号展示 | A密码改变/自revoke后登录B有旧metadata | 4400898已修：ChatStore.swift:63–77 OwnedSessionSnapshots按完整ChatIdentity绑定/读取/写入，拒绝nil/旧owner；aTerminalApp:160在发布新身份前bind并清旧presentation，:171/271/291/307/404/421/436回调身份屏障，logout/password/self-revoke清volatile owner。持久正常历史不删。实际owner checks及main源码字节比对PASS |

## 已通过的关键逻辑

生产 SettingsDraft.swift 直接链接，没有复制算法：provider connection/hidden refs 保留；模型未暴露字段与 refreshed hidden caps；同 ID catalog selection 完整覆盖能力；read_only 与 binding identity-change reasoning 清理；token/sampling/reasoning 校验；MCP/Skill ID/绝对路径合同；完整 Skill binary/empty file 枚举与 symlink/size/missing SKILL.md 拒绝；busy 单飞；save 返回 snapshot 不额外 show；provider 请求错误中密钥内容替换；revision conflict show 后等待显式重试。

10组production logic及完整UI XCTest simulator-SDK离线typecheck通过；新增catalog cancelRead成功/失败迟到serial检查。fixture UI已覆盖关闭/离线正常history只读、统一workspace、Global/Session/第二Global草稿、绑定继承、读取、MCP确认删除/Skill编辑；最终合同global.list返回=`global.back`、Skill edit成功返回详情，已同步。逐页查看20张永久参考并加同编号capture，见SCREENSHOTS.md；未实际sim/真实RPC。

协调者执行的首轮fixture log为main build/ios-parity-fixture-first.log（16项13fail，R11父ID遮蔽），smoke log为main build/ios-parity-smoke.log（4项3PASS，Azure endpoint=https://fixture.invalid/v1/v1）。本任务只读结果并提取AX fixture快照，未操作sim。UITestInput共用helper改为Select All后替换、空值删除选区；失败只给通用消息，不回显值。待协调者重跑确认helper。

输入smoke v2证明SelectAll+doubleTap菜单仍不可用（2项均失败）；main10f2add改为field.typeKey("a", modifierFlags: .command)。已独立核对XCUIElement.h:113–130与main diff：聚焦、placeholder识别、空值delete、无秘密精确布尔断言均保留，main完整UITest离线SDK typecheck通过。此helper由协调者拥有，本任务未再改动。v3实际smoke由协调者执行，不以typecheck宣称通过。

补充复核：已直接链接并重跑实现方AgentCacheSearchChecks（2组PASS，scope/current generation/Unicode/非首页cursor/只匹配正文/legacy排除/cancel与debounce迟到屏障）、BindingReasoningChecks（PASS，同ID/高级能力/兼容override/unknown/protocol/output）、SessionSnapshotOwnerChecks（PASS，账号/服务器/旧回调/nil隔离及持久历史不变）。这些结果与本任务自有10组direct互补，不等于真实Desktop RPC验收。

## 原生输入与最终小diff复查

- ae9b4cf，AgentSettingsView.swift:227–253/436–520：普通字段与code editor明确clear控件，readonly/busy继承父禁用；SecureField key未增加明文操作。UITextView普通Binding echo不重写文本，外部变化按UTF16 clamp选区且保存offset；markedText时普通外部写入延后；显式clearVersion唯一主动unmark/reset选区路径，applyingExternalText阻止delegate回填，dismantle清delegate和focus。无RPC/修订/凭据合成改动，无新增阻塞。
- 0e6980c，WorkspaceScreen.swift:63：drawer转场从move leading改identity，去除已观察到停在x=-322的屏外transform；尺寸、leading、状态、modal及账号边界不变。真实Desktop路线由协调者实测；本任务只读live-drawer截图与源码。
- b2d3f3a，AgentSettingsView.swift:273–291：model.advanced标识仅挂44pt独立Button，展开字段VStack不再继承父ID；advanced Bool、busy禁用、字段绑定/保存逻辑保留，VoiceOver读出展开状态。Catalog定向复测证明字段可访问。
- main b4f8fa2 UITestInput：先对应.clear按钮，再验空，再输入并精确布尔断言；没有clear的fallback仅允许单行/secureTextField，用右端坐标+原UTF16长度delete并再次验空；TextView没有clear直接fail，避免部分删除后拼接JSON。失败不回显值。WorkspaceSearch/Login定向复测通过；本任务未改协调者helper。
- f93fac8 testAgentBodySearchFindsClosedHistory：正文独有token筛选同时排除active/offline，进入closed历史并读到同token；已有closed/offline测试用非空draft检查只读，避免复制测试业务。冻结main完整UITest离线SDK typecheck通过。

只读日志确认：native-editor两项WorkspaceUITests fixture UI PASS；fixture-final首轮14项PASS，Catalog/ClosedHistory/Login三项fixture-retest PASS，合计17个不同fixture testcase均通过。此为协调者设备执行证据，不能写成单次17/17整套通过，也不能替代LiveService RPC。live-final第一个真实Desktop路线已PASS；实际MCP/Skill UUID操作与host资源/配置哈希由协调者另结算。

## 最终结算（协调者归档）

6dcec6b以现有FieldShell补齐连接协议、供应商、思考模式、默认绑定及三种scope的可见名称；10ecef6保留实际Picker标识并补当前选项的accessibilityValue。选项值、Binding和保存协议均不变。独立审查子任务已明确确认10ecef6语义修正通过；标签后Azure/catalog定向通过、binding含当前值/继承还原通过，小屏大字体/横屏复测通过。

最终17个fixture场景与3个真实Desktop场景都有通过结果。真实MCP/Skill表单完成修订20→30的导入、编辑、启停和删除；两个不可变Skill版本的文本/二进制资源哈希一致，无关配置哈希一致，UUID条目已移除。专用PTY字符/Enter/delete/Tab/Ctrl-C通过；隔离zsh首次未加载compinit导致Tab预期失败，fixture环境补全初始化后复测通过，没有改用户Shell配置。最终arm64 Release与x86_64 Debug构建通过。未执行实际iOS LLM/图片发送、系统文件夹选择/手机上传或用户密码/设备撤销，未用fixture成功替代真实RPC结论。永久20页截图、最终逐场景结果与资源观测位于协调目录`doc/task/1001-ios-parity/evidence`。
