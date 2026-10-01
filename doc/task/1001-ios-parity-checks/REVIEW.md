# 独立审查记录

已独立审查最终生产提交 `89fc011`（`[未Review] 同步 iOS 工作空间与 Agent 设置功能`）及完整新增SettingsDraft/AgentSettingsView。**R1–R8没有剩余阻塞；完整功能parity还发现R9/R10两项P2，已即时交协调者。** 本任务只提交测试/脚本/文档，不merge生产代码。App build和实际UI/真实RPC由协调者执行。

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
| R12 / P1身份边界 | WorkspaceScreen.workspaceCandidates将无owner的sessionSnapshots绑定当前identity；aTerminalApp.changePassword/revoke/loadDevices不清旧快照 | A密码改变/自revoke后登录B，B列表可见A的title/cwd并伪造B scope | 已给协调者具体路径/行号，修复中；应在确认identity变化时清volatile snapshots或owner-scope索引，不删持久正常历史 |

## 已通过的关键逻辑

生产 SettingsDraft.swift 直接链接，没有复制算法：provider connection/hidden refs 保留；模型未暴露字段与 refreshed hidden caps；同 ID catalog selection 完整覆盖能力；read_only 与 binding identity-change reasoning 清理；token/sampling/reasoning 校验；MCP/Skill ID/绝对路径合同；完整 Skill binary/empty file 枚举与 symlink/size/missing SKILL.md 拒绝；busy 单飞；save 返回 snapshot 不额外 show；provider 请求错误中密钥内容替换；revision conflict show 后等待显式重试。

10组production logic及完整UI XCTest simulator-SDK离线typecheck通过；新增catalog cancelRead成功/失败迟到serial检查。fixture UI已覆盖关闭/离线正常history只读、统一workspace、Global/Session/第二Global草稿、绑定继承、读取、MCP确认删除/Skill编辑；最终合同global.list返回=`global.back`、Skill edit成功返回详情，已同步。逐页查看20张永久参考并加同编号capture，见SCREENSHOTS.md；未实际sim/真实RPC。

协调者执行的首轮fixture log为main build/ios-parity-fixture-first.log（16项13fail，R11父ID遮蔽），smoke log为main build/ios-parity-smoke.log（4项3PASS，Azure endpoint=https://fixture.invalid/v1/v1）。本任务只读结果并提取AX fixture快照，未操作sim。UITestInput共用helper改为Select All后替换、空值删除选区；失败只给通用消息，不回显值。待协调者重跑确认helper。
