# iOS 独立验收最终交接

- Status: Completed
- Updated: 2026-10-01
- 独立源码审查无剩余阻塞；最后复查 main 10ecef6、标签布局6dcec6b及原生editor/identity drawer/高级展开小diff。
- 本任务交付测试7e55a62/b62dd8a与review文档；协调者整合到main。本任务没有操作sim、共享服务、用户身份或Downloads，没有merge；source/tests最后保持不变。

## 自动化与实际证据

| 类别 | 最终结果与边界 | 证据 |
| --- | --- | --- |
| Host production logic | SettingsDraft 10组PASS；缓存正文搜索2组、binding reasoning、owner snapshot隔离/持久历史保留PASS；实际main UITest SDK离线typecheck PASS | 本任务build/ios-parity-checks及REVIEW.md，调用真实生产helper，不复制算法 |
| iPhone15 fixture UI | 17个不同testcase全部PASS：14首轮+Catalog/ClosedHistory/Login三项定向复测；涵盖设置路由、取消/失败/busy、Azure、目录、绑定、读取、扩展、scope草稿、正文搜索；没有真实LLM调用 | main build/ios-parity-fixture-final.log、ios-parity-fixture-retest.log；evidence/full-tests.json、retest-tests.json |
| 小屏 | SE两项PASS，含大字体/键盘/横屏actions；标签修正后的SE大字体/横屏另由协调者确认PASS | main build/ios-parity-small-final.log；evidence/small-tests.json、small-labels-tests.json |
| 真实Desktop路由 | 专用新iPhone15与预附着disposable PTY，Desktop设置读取与新UI路径PASS | main build/ios-parity-live-final.log |
| 真实MCP/完整Skill | UI round-trip约94秒PASS：MCP disabled HTTP localhost:9导入、timeout=12345编辑读回、启停/确认删除；Desktop完整Skill安装、有效SKILL.md编辑读回、启停/确认删除。只操作预生成的两个UUID | main build/ios-parity-live-final.log；evidence/live-final-screenshots-tests.json |
| Host RPC完整性 | 11个revision状态(20…30)、2个Skill版本；两版非Markdown资源SHA与baseline一致，无关配置SHA一致，UUID条目已删除，errors=0 | evidence/live-observations.json、live-baseline-hashes.json；本任务只读统计和哈希结果，未调用RPC |
| 真实键盘 | printf/输入/Enter/delete/Tab/CtrlC最终PASS，0skip。第一次Tab失败来自disposable zsh -f未compinit；协调者仅该独立PTY启用compinit -D -i后完整重测成功 | main build/ios-parity-keyboard-final.log；evidence/keyboard-final-screenshots-tests.json |
| Release | arm64 Release BUILD SUCCEEDED；最终日志仅既有静态libai_terminal_mobile.a debug-map重复对象警告 | main build/ios-parity-device-final.log |
| 最后标签语义 | 6dcec6b显示连接协议/供应商/思考/默认绑定及三scope名称；10ecef6补当前选项accessibilityValue，绑定/选项/RPC逻辑与测试断言保留。Azure/catalog、binding-final及SE标签大字体定向实测由协调者确认PASS | evidence/labels-final-screenshots-tests.json、bindings-final-screenshots-tests.json、small-labels-tests.json |

## 永久图集

20页iOS截图与测试摘要/manifest在 main `doc/task/1001-ios-parity/evidence`，01-terminal至20-global-chat对应永久0930共用参考；04/06/10及小屏截图已由协调者刷新。capture来源映射见SCREENSHOTS.md。fixture中的数据、消息和配置是测试数据；截图只证明对应UI场景，不能冒充真实RPC配置结果。

## 准确未测范围

- 未运行iOS真实LLM消息发送/完成、真实模型供应商/Azure凭据调用或真实目录discover；Session/Global消息与草稿UI为fixture，配置编辑关键合同另有host logic检查，真实配置读取路由已测。
- 未实际走iOS系统folder选择器/手机文件夹上传UI；本次真实Skill是Desktop绝对目录完整安装/编辑。文件枚举、二进制/空文件、symlink/大小边界有host deterministic checks，不能替代fileImporter实际选择。
- 没有真实硬件安装/运行；arm64证据为Release构建，实际交互为专用simulator。MCP导入/配置启停测试使用localhost:9，无真实MCP工具调用。

## 测试与数据合同

WorkspaceUITests只用DEBUG fixture：defaults/history/AgentCache/Keychain隔离，禁止normalCache/migration/network。main helper由协调者维护：正规.clear验空后输入；没有clear的单行控件右端定位+UTF16长度delete后验空；TextView没有clear失败中止。所有失败断言不回显输入、凭据或配置dump；本分支旧helper保留历史，整合应保留main已实测版本，不复制文件。

LiveServiceUITests需opt-in AI_TERMINAL_IOS_FIXTURE，最小JSON为session/typingMarker/可选caPemPath，无credentials；先验证唯一marker与指定session，再执行允许操作。扩展测试另需mcpId/skillId/skillPath，ID为纯UUID或合法前缀+36位带连字符UUID，互不相同；缺失/无效在launch前skip，安装前确认UUID不存在。只编辑这两个UUID，不登录/退出/清空身份，不send Agent；host负责配置/资源哈希与失败cleanup。键盘case单独在已验证的disposable PTY输入固定测试命令。
