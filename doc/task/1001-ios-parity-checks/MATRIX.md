# 功能差异与验证矩阵

- 基线：checks `ff3c490`，2026-10-01；实现仍在另一 worktree 进行。
- 权威：永久 `0930-app-settings-redesign/{UI_REFERENCE,ANDROID_REFERENCE,IOS_HANDOFF}.md`；当前 Android `MainActivity.kt`、`AgentSettingsPanel.kt`。
- 标记：**F** DEBUG fixture UI；**D** 直接调用 production logic 的确定性检查；**R** 隔离真实 Desktop RPC；**S** 协调者实际 simulator/截图。未执行不能算通过。

| 参考页/功能 | Android 已有语义 / 基线证据 | iOS 基线差异 | 关键验收 / 类型 |
| --- | --- | --- | --- |
| 01 terminal / 登录 | aTerminal 品牌、服务器摘要/修改；终端不改远端列数；MainActivity 工具栏 | 需统一视觉，现有终端行为应保持 | 品牌/登录验证；原 PTY 和最近目录/显示偏好保持 F/S/R |
| 02 settings | MainActivity.settingsPanel 分组卡片，LLM/读取/MCP/Skills/账号，固定恢复入口 | WorkspaceScreen.settings 仅显示滑块，无独立配置路由 | 首页五入口；账号返回设置；返回/关闭来源；滑块持久化、恢复默认 F/S |
| 03 LLM | AgentSettingsPanel.render 当前作用域有效模型，供应商/模型分组和独立绑定 | ChatPanel.AgentSettingsView 把读取/供应商/模型/MCP/Skill 混成 Form | 独立 LLM 根页、模型/供应商分组、摘要来自真实绑定 F/R |
| 04 provider | provider 已有 ID 禁改；connection 原对象复制；HTTP(S) 无 userinfo；空 key 保留 | ID 可编辑，connection 重建会丢 unknown；缺少前端校验 | 取消不 replace；失败 endpoint 草稿保留；空/空白 key 无 secrets 条目；root/provider/connection unknown 保留；秘密退出清除 D/F/R |
| 05 Azure | Azure api_version 输入/标签整体条件显示，非 Azure 不使用残留值 | 条件显示已有，但 save 只检查非空，非 Azure 仍可能发送残留值 | Azure 空 version 阻止 RPC；切其他协议字段隐藏且 payload 清除；不丢 connection unknown D/F |
| 06–07 model | 高级能力折叠；sampling/推理模式按能力/协议校验；Tools 影响 read_only；identity change 清旧 binding reasoning | 全展开；缺少完整校验，保存重建 caps 会丢隐藏字段/unknown | 保留未改字段；context 4096…4000000、0 < output < context−2048；temperature 0…2、topP 0 < x ≤ 1；budget/level/protocol 合同；无 tools 不可 writable D/F/R |
| 能力覆盖 | capsFromSnapshot：只有未换身份/未选目录继承 refreshed hidden；同 ID 目录也以选择的 caps 为准 | selectCatalog 只映射部分能力；saveModel 重建 caps；刷新可能覆新值 | hidden streaming/temperature/top_p 与 unknown；同 ID 目录新 caps 覆旧 caps；手动换 provider/model 清能力/推理；冲突刷新不覆目录来源 D |
| 08–09 catalog | catalog 独立页，搜索改变丢 cursor；分页保留 query/provider；屏幕 serial 屏障 | 内嵌目录，query 改变不失效 cursor；无 query serial | page1/page2 cursor/provider/query 合同；query/provider 改变后旧响应丢弃；返回/取消后旧响应不可更新 model；选择回原 model D/F |
| 10 bindings | global/session-default/session/<id>；当前继承删除 override；未改保持原对象，改变模型清旧 reasoning | 只有 save-model 时范围选择，无独立页/继承管理 | current 空选择仅删除 current；global/sessionDefault 不变；未改 binding unknown/reasoning 保留；改变模型不复用不兼容 reasoning D/F/R |
| 11 reading | 1…100 整数，更新原 terminal_reading 保留 unknown；TUI 说明 | Stepper 限值已有，重建 reading 会丢 unknown | head/tail 边界；失败保留输入；保存 expected_revision；unknown 保留 D/F/R |
| 12–13 MCP | 内置只读，列表/详情/编辑/启停/确认删除；合并 mcpServers 完整 entry；参数/引用/超时限制 | Menu 混合操作；delete 无确认；导入缺 UI 校验 | 禁 builtin ID；空/无效 JSON 不 RPC；stdio 与 HTTP transport 合同；env/headers secretRefs/args/timeouts/unknown 保存；取消确认不删除 D/F/R |
| 14–15 Skills | 内置只读；完整 Desktop 文件夹安装；系统文件夹上传 begin/chunk/commit；skill_read/edit SKILL.md | 已有路径/install/import/editor，但无独立列表/详情和确认 | ID/绝对路径校验（含 Windows Desktop）；完整资源不丢；skill_read/edit command/path/revision；SKILL.md ≤512 KiB；enable/delete 只影响本条 D/F/R |
| Skill folder | Android upload 最多256文件、8层、2MiB/文件、8MiB总计；49152-byte chunk；空文件上传 | 现有 upload 需确认同限制、安全路径/符号链接边界和 security-scoped URL | binary/空文件嵌套资源原字节；拒绝越界路径/过大/符号链接；一次失败不 commit；成功 snapshot 替换；资源哈希协调者 R，payload/限制 D |
| 16 account | 设备只显示在线 Desktop；从 settings 进入/返回；账号操作有层级 | 旧 drawer 中帐号/设备/退出混合，设置无来源 | 账号页从 settings 返回 settings；无退出现有用户；F/S，正常身份只读 |
| 17 workspace | MainActivity.openDrawer 统一线上/离线/关闭正常会话，与历史同 row 相关动作；搜索 | WorkspaceScreen.drawer 终端/AI历史 segmented | 无 segmented；online/closed/offline 可搜索；选中当前终端；历史/草稿隔离；无旧手机归档 F/D/S |
| 18 Session | session scope；路径，消息/图片/证据/草稿，关闭/离线历史只读 | ChatPanel 用 Session/Global segmented；draft 单字符串 | 独立 Session；草稿按 server/account/device/session；send 失败不清；证据/图片/历史目标隔离；切页迟到屏障 D/F/R |
| 19–20 Global | 独立 list/create/multi conversation/state/unread；Global chat 返回列表 | AssistantModel global Bool，scope.session=""；没有 conversation_id | list → A/B 独立 title/message/draft；会话 ID 出现在 RPC；切换 B 后 A 迟到结果不可污染；关闭回列表 F/D/R |
| legacy removal | UI_REFERENCE 明确不提供/不导入旧手机归档；当前正常历史保留 | ChatPanel 旧归档；AssistantModel.legacy 调 importLegacy | 旧入口/自动调用消失；不删磁盘/SQLite/正常历史；共享 legacy API 是否移除依调用关系判断 D/S |
| 保存状态/修订 | run 防重复，origin screen + serial + MainActivity 连接 generation；mutation 直接采用返回 snapshot；conflict show 后显式再 save | perform 无 busy guard，只比 target；save 后 load；无 conflict retry | busy单飞；失败 draft；成功仅用返回 snapshot；conflict 1次replace+show，不自动第二次；显式重试最新 revision；取消、关页、同目标 reconnect 后旧 success/error 不变新页 D/F/R |
| 可达性/安全隔离 | 原生 full-page，固定取消/保存，键盘/大字体压力；实际扩展哈希验收 | 老 Form 内部按钮；fixture defaults 隔离但 cache/history 未隔离；旧 live 强制logout | 小屏/动态字体/IME 按钮可点击 S；fixtures 不接网络、不写正常 prefs/cache/keychain；断言不输出秘密；真实 RPC 用专用身份/PTY，无 logout D/F/R |

## 验证分工与当前边界

本任务拥有 UI Tests/新增 Checks，先按实现者接口编写并只运行允许的静态/host logic 检查。实现者 production logic 未到位时不得用自制模拟 reducer 宣称覆盖。主 checkout 的 App/XCTest build、实际 simulator 和同编号截图由协调者执行。现有 Rust tests/Android 实测提供协议依据，但不证明新 Swift 生产路径正确。

真实 RPC 最小范围：单次配置读取；UUID MCP 安装/读/编辑/启停/确认删除；UUID 完整 Skill 安装/read/edit/资源哈希/启停/删除；只清理本轮 UUID，原配置哈希保持。LLM 仅对隔离配置做 replace/revision/凭据行为验证，不动用户当前模型/账号。
