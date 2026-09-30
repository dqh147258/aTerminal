# Android 设置实现只读代码审查

- 日期：2026-10-01。
- 审查目标：`/Volumes/Code/public-worktree/aTerminal/0930-android-settings-ui`。
- 实际读取状态：owner 已提交，开始及结束 `git status --short` 均为空；HEAD `4b6862347006c88fc28d74d34a4a498e1ea0abb8`（`[未Review] Redesign Android settings as full-page configuration flows`）。本次比较父提交 `749a5b2fc80130d03175c224b191a0d71578865d` → `4b68623`，不是未提交 diff。
- 范围：读取批准后的 main `doc/task/0930-app-settings-redesign/PLAN.md`、`UI_REFERENCE.md`及永久模型页截图；最终深蓝/亮蓝主题、保留 composer label 和独立 Global、多会话；不要求旧手机归档/迁移，不改变登录服务器摘要/修改交互。
- 初审结论：存在一项应修复的功能问题与一项应补强的关键测试；已 informational message 报协调者。下方保留初审证据。**2026-10-01复审：R1/R2已在635851e解决；main额外主题2文件未发现API25兼容或需修正问题，详见末尾复审。**

## 初审问题（均已修复，保留历史证据）

### R1 — P2：同一模型重新选目录后，最新隐藏能力被旧快照覆盖

文件：`apps/android/app/src/main/java/com/yxf/aterminal/AgentSettingsPanel.kt:332–334`，关联 `269–278`。

目录选择会将返回的 capabilities 整体复制到 caps。保存时只要最终 provider/model 字符串与进入编辑时相同，就重新从 candidate 原配置取 streaming/temperature/top_p 并覆盖 caps。结果是选择同 ID 模型的**最新目录声明**会被旧声明取代。

具体路径：现有模型 temperature=true → 打开编辑 → 从目录重新选择同 provider、同 model ID，目录返回 temperature=false → 不填采样参数，保存 → payload temperature 能力恢复为旧 true。下次任务/编辑使用错误能力。streaming/top_p 同样受影响。这与“保留未展示字段”不同：目录刚刚提供了这些字段的新值，不能无条件覆盖。

建议最小修复：记录能力来源是否已因模型/provider变更或目录选择重置；只有仍沿用原声明时从冲突刷新后的 snapshot 合并未展示字段，目录选择后的新声明优先。缺失目录能力也应保持未知，不重新补旧假设。加一个同 ID 目录选择用例，断言保存 payload 采用新的 hidden capabilities。

### R2 — P2（验证）：迟到回调测试可能在后台完成前通过

文件：`apps/android/app/src/androidTest/java/com/yxf/aterminal/AgentSettingsUiTest.kt:158–159`。

`late.countDown(); instrumentation.waitForIdleSync()` 只释放 worker 并等 UI 当前空闲，不保证 worker 已返回 JSON 并投递/处理 runOnUiThread。调度较慢时，父页断言在迟到结果抵达前就通过；之后 `@After` 关闭panel，回调因closed被忽略，不能证明页面身份保护生效。

建议最小修复：增加确定性屏障证明原请求返回后的UI callback已排队并完成，再断言父页不变。可在同一串行worker上发起可观测后续请求，等待其UI结果，利用同一队列顺序保证前一个回调已处理；不要用固定sleep。现有测试断言也可补父表单仍可继续编辑/保存，避免仅验证旧页面未消失。

## 已核对、未发现必须修问题的部分

| 项目 | 代码证据与判断 |
|---|---|
| 全屏导航 | AgentSettingsPanel Screen栈复用worker/snapshot；back弹一层，返回根时刷新。MainActivity:1063–1080顶栏返回先消费设置栈，1187系统返回同路径；关闭清理整个panel。未重建terminal/连接。 |
| 账号来源 | MainActivity:849起参数fromSettings，刷新时无参wrapper仅在当前account页继承来源；设置入口明确true；侧栏入口false；系统返回从设置来源回设置。 |
| busy/旧回调 | run:79–105以Screen对象+serial校验；控件仅恢复原本enabled的集合，避免错误启用只读ID。MainActivity配置闭包RPC前后均校验connected/device/account/generation。目录页独立Screen，返回后旧结果被丢弃。 |
| 保存成功/刷新失败 | mutation成功直接使用实际返回View，未再发show；Rust ConfigService replace、skill_install/edit/upload_commit确实返回View。新增success测试能验证后续show不可用不影响已保存返回。 |
| revision冲突 | 仍带expected_revision，冲突show更新snapshot，不自动重发；保留草稿并要求显式再保存。provider/model基于最新copy更改，validId拒绝被删除的编辑对象。冲突刷新失败仍保留表单与旧revision，后续不会绕过冲突保护。 |
| key/旧字段 | provider按最新对象与connection增量更新，secret_ref/catalog_url/credential_revision不主动重建；空key不写secrets。key禁用状态保存/自动填充，离开栈clear清空，非空值仅内存及原RPC。model保留max_rounds/max_seconds/read_only。R1是目录capabilities的局部例外。 |
| 真实验证 | SettingsValidation与Rust reasoning mapping核对：level协议、budget范围和max_tokens、Anthropic采样互斥、adaptive/disabled一致；context/output/top_p与服务端范围一致。provider/model ID及Azure整组显隐正确。服务端仍负责完整schema验证，客户端未替代服务端。 |
| 默认绑定 | 独立页空值删除key；未变模型保留原binding reasoning；切模型清理相关reasoning；无session不生成session/空键。 |
| MCP | stdio/streamable_http、command/url、timeout/args/env refs限制与Rust规则一致；JSON解析/服务端失败保留编辑器；导入合并、原JSON编辑、启停和删除确认仍接replace。 |
| Skills | Desktop安装、SAF完整目录遍历/分块upload_begin/chunk/commit、SKILL.md read/edit、启停删除均保留；upload结果按页面校验，revision冲突刷新；非Markdown资源不会被原型式截断。未运行实际SAF联调。 |
| 主题和范围 | NativeUi token是批准后的#090d16/#101623/#38bdf8等；AgentPanel未改，composer label、独立Global及历史机制保留；没有新增旧手机归档入口。登录结构未改，共享色值生效。 |

## 建议项（不作为本次阻断）

- `NativeUi.settingsRow()`对可点击行设置`contentDescription=title`，模型/供应商状态副标题不包含在该描述中。可在设备可访问性验收时确认TalkBack是否能读到副标题；如被标题覆盖，组合title/subtitle并声明按钮语义。这里只确认代码结构，未声称已重现读屏缺陷。
- `reading()`保存按钮在滚动内容内，其他编辑页使用固定actions；大字体+IME场景应包含读取页，确认滚动到底仍可提交。未运行布局测量，不把潜在遮挡当已确认bug。
- 手工编辑MCP对象属于完整JSON替换，冲突后显式重试仍会用原编辑文本覆盖该对象；当前提示“请检查草稿”符合已有约定，若后续希望字段合并应另定冲突产品策略，不建议本轮擅改。

## 所读变更与验证边界

重点逐段读：`AgentSettingsPanel.kt`（全文件）、`SettingsValidation.kt`（全文件）、`NativeUi.kt`（全文件）、`MainActivity.kt`变更及相关上下文、`AgentSettingsUiTest.kt`（全文件）、`WorkspaceUiTest.kt`变更、`AgentReadingUiTest.kt`变更，以及owner `HANDOFF.md`。

对照：`crates/desktop-agent/src/config.rs`的mutation响应/修订/凭据规则，`crates/agent-runtime/src/config.rs`及`extensions.rs`的真实字段与验证；主计划和图集索引。17个变更文件中的新增图标与任务文档仅检查提交范围，不宣称逐像素视觉验收。

本审查只执行Git只读检查和源码/文档读取，`git diff HEAD^ HEAD --check`无输出。**未运行构建、lint、测试、服务、浏览器、模拟器，未改owner文件、提交或合并。** owner记录编译/lint通过、10个测试仅编译未跑，这个限制表述准确；本报告不把它升级为测试通过。

初审时要求实施者修R1及R2后在新提交做针对性复审，并由协调者执行已批准的instrumentation/设备与截图验收；初审只读review任务已完成。

## 2026-10-01 针对性复审：635851e 与 main 主题

### 所读状态与范围

- owner工作树干净，HEAD `635851e5182eb5a802ad1503727b36b2c02cd038`；复审diff为 `4b68623 → 635851e`，只聚焦R1/R2的实现和测试。
- main `/Volumes/Code/My/aTerminal` HEAD同为635851e；额外读取未提交 `apps/android/app/src/main/AndroidManifest.xml` 和未跟踪 `apps/android/app/src/main/res/values/styles.xml`。main另有测试/文档变更，本轮未审查、不触碰。
- 本次主题内容摘要：保留`android:style/Theme.Material.NoActionBar`为父主题，application改用`@style/ATerminalTheme`；设置windowBackground/colorBackground、colorAccent、colorControlActivated/Normal/Highlight、textColorPrimary/Secondary，均为android框架属性与字面颜色。
- styles SHA256：`ee0fdb7fdb8374d7a3732adda10db897626ac9aef2ba26237d27304748495e65`。
- Manifest SHA256：`f196f1c5e921de36929def0edc19a91bab4e94e4e703041d5001612f67093ee7`。

### R1：已解决

`AgentSettingsPanel.kt`新增`capabilitiesFromSnapshot`，用户改model或目录选择（包括同ID）均置false。仅未重新声明能力的编辑才从最新snapshot合并hidden capabilities，并且合并移到sampling校验之前；最新snapshot缺失字段时也移除旧值。

因此目录的false、null以及缺失能力都不会再被原snapshot覆盖。新增`reselectingSameCatalogModelPreservesFreshAndUnknownHiddenCapabilities`同时断言同provider/model ID、streaming=null、temperature=false、top_p缺失及max_rounds保留，覆盖原具体缺陷和未知能力语义。未发现本修复引入的范围外行为问题。

### R2：已解决

`AgentSettingsUiTest.awaitWorkerUi()`在同一个单线程worker中排入后续任务，由该任务向UI队列投递latch标记；此前RPC的UI回调必定先入UI队列。等待标记完成后才检查父表单，并进一步改模型、保存和断言payload。这建立了原测试缺失的完成顺序，避免仅等待UI暂时空闲而提前通过。

使用反射取现有worker只在测试中，不新增产品同步/测试hook；8秒超时会显式失败，不以sleep掩盖调度。

### 主题/API25：未发现需修正问题

- 本机未安装android-25平台资源；通过已安装`platforms/android-35/data/api-versions.xml`核对API引入记录：`Theme_Material_NoActionBar`、`colorAccent`及三个`colorControl*`均`since=21`，window/background和两项textColor属性更早已有。应用minSdk=25，无新增高版本属性，无需values-v26/v29等分支。
- 父主题与原Manifest相同，保留暗色Material/NoActionBar语义；Manifest差异仅application主题引用，未改权限、activity、windowSoftInputMode、configChanges或导出行为。
- `#1F38BDF8`是约12% alpha亮蓝highlight，颜色格式有效；primary/secondary文字适配既有暗背景。框架默认控件/光标继承accent，显式程序化tint仍优先，符合本次集中消除旧薄荷色目的。
- theme范围是应用及其框架控件背景/文字/状态色，不改变终端ANSI数据、配置RPC、连接或导航逻辑。未发现不合理的行为性覆盖。
- 这仅是静态兼容与影响检查，不宣称API25真机渲染、所有Dialog tint或所有禁用状态已视觉验证。

### 复审验证边界与结论

`git diff 4b68623 635851e --check`无输出。未运行构建、设备、服务或测试；未修改实现、提交或合并。

协调者报告设备已通过11个设置、3个Workspace、1个真实encrypted RPC测试；本复审记录为协调者提供的执行结果，没有冒充独立重跑或日志核验。

**本轮范围内无剩余必须修问题：R1/R2关闭，主题两文件静态审查通过。** 此结论只覆盖上述提交diff和主题文件版本，不覆盖main其它未提交内容或完整设备视觉验收。
