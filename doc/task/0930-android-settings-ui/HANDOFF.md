# Android 设置 UI 交接

2026-10-01。本实施复用主计划用户批准“按 UI 计划执行”。未启动浏览器、模拟器或服务；未 merge。

## 实施

- 最终 CSS 蓝色共享主题、渐变主按钮、设置专用图标信息行/表单标签；设置首页显示卡片、左右数值滑块、真实有效模型摘要和固定恢复/自动保存底栏。
- 同一个 AgentSettingsPanel 的 Screen 栈、worker、snapshot；provider/model/目录/绑定/读取/MCP/Skill 全屏导航。保存固定可达，系统返回和顶栏回父页，离开清密钥，返回收键盘。
- 本地 ID/重复/真实 Rust context、输出、采样、协议思考映射校验；Azure 整组显隐；换供应商/模型清能力与思考；保留未展示字段、read_only、空 key 不写入凭据、expected_revision。冲突刷新 snapshot 但保留草稿，必须再次显式保存。已被其他客户端删除的编辑对象拒绝重新创建。
- busy 禁止重复提交；每个异步结果校验 Screen 身份；mutation 直接使用服务端成功 View，不追加 show，避免已保存但刷新失败被误判为未保存。
- 目录真实搜索/分页；MCP JSON 导入合并、编辑、启停和确认删除；Skill Desktop 路径安装、完整 SAF 文件夹分块上传、SKILL.md 编辑、启停/删除均保留。
- 账号设备卡片/状态/操作图标与设置来源返回；保留只显示在线非本机、原账户操作。登录服务器地址摘要与修改入口保持原行为。
- Session composer label 按最新 reference/18-session-chat.png 保留，无 AgentPanel 源码变化。Global 多会话、终端 ANSI、会话历史、协议不改。
- 未发现旧手机归档迁移调用；唯一相关调用为 AgentPanel.kt 的 `globalConversation.legacy -> AgentDraftStore.migrateGlobal`，是服务器原 Global 草稿兼容，按用户要求保留。不清理磁盘数据、不新增旧手机入口。

## 实际验证

通过：

```sh
ANDROID_HOME=/Users/carl/Library/Android/sdk apps/android/gradlew -p apps/android \
  :app:compileDebugKotlin :app:compileDebugAndroidTestKotlin :app:lintDebug \
  --offline --console=plain
git diff --check
```

复用了主 checkout `build/bindings/uniffi` 生成物到本 worktree 的忽略目录，没有复制凭据或主源码。lint 成功，有现存及新增程序化 UI 文本的国际化/弃用等 warning；不宣称零 warning。

新增 `AgentSettingsUiTest` 10 个受控回调测试，已编译，**未运行 instrumentation**。覆盖 provider 无效/重复 ID、Azure、busy/失败草稿/清 key、mutation 成功不依赖 show、revision 冲突显式重试、未展示字段和空 secrets、模型预验证/能力重置、目录分页迟到、绑定删除覆盖、reading 非法输入、MCP 导入编辑、Skill 编辑失败和成功父页、思考协议约束。测试使用 isolated Activity，不依赖真实账号、公网模型或 screen.pb。

已同步 `WorkspaceUiTest.displayPanelPersistsResetsAndSurvivesRotation` 的设置文案选择器、account 返回来源断言与 `AgentReadingUiTest` 的字段错误断言。原 Codex live workflow 测试未改。

## 协调者集成后验证

先 Git merge 本分支提交，再使用主 checkout 的 JNI/fixture 资产构建 APK 和 test APK；不要复制一组源码去 main。

1. 跑 `com.yxf.aterminal.AgentSettingsUiTest` 全部用例。
2. 跑 `WorkspaceUiTest#displayPanelPersistsResetsAndSurvivesRotation` 和 `#accountPanelShowsOnlineDevicesWithoutStaleOfflineRows`。
3. 用已有隔离 fixture 跑 `AgentReadingUiTest#readingSettingsAndAgentEvidenceRoundTrip`。
4. emulator-5586 对照主任务 reference 图集核对所有设置/账号页面；覆盖 IME、小屏与大字体下滚动和保存按钮、顶栏/系统返回、0/88/100 不透明度、连接/会话/远端列数保持。截图存主任务指定目录。
5. MCP/Skill 真实完整文件夹上传含非 Markdown 资源，以及编辑/启停/删除，需要协调者实际联调；本轮编译不能替代真实 RPC 验收。

主要残留风险是未经设备验证的测量/键盘/SAF 生命周期和测试选择器；Android 原生字号单位保留 sp，原型显示 px。iOS 后续同步，未改。提交标记 `[未Review]`，通过编译不等于代码审查或视觉验收通过。

## 2026-10-01 独立 review 修复 R1 / R2

- R1：增加 `capabilitiesFromSnapshot` 标记，模型/provider 重置或目录选择后不再回填旧隐藏能力。同 ID 再选目录也保留新的 streaming/temperature/top_p，缺失或 null 保持未知。仍沿用原声明时才合并新 snapshot 隐藏字段，并在采样预验证前完成合并。
- 新增 `reselectingSameCatalogModelPreservesFreshAndUnknownHiddenCapabilities`，断言同 provider/model ID 的目录声明保存为 streaming=null、temperature=false、top_p 缺失，同时保留 max_rounds。受控测试总数现在为 11。
- R2：迟到测试在同一个串行 executor 上排入后续任务，由它向 Activity UI 队列发送 latch 标记并等待完成；FIFO 顺序保证原 RPC 已返回且其 UI callback 已处理。随后断言父表单未变，再修改并保存，核对 payload。移除了仅依赖 waitForIdleSync 的假完成条件，没有固定 sleep。
- 实际验证：`compileDebugKotlin`、`compileDebugAndroidTestKotlin`、`lintDebug`（offline）及 `git diff --check` 通过。此 executor 未运行 instrumentation、未 merge、未操作设备；协调者集成新提交后执行上述新增/补强用例。
