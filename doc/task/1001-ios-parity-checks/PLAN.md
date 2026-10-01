# iOS 独立差异审查与验收支持

- Status: Completed
- Updated: 2026-10-01

## 目标与范围

对照 Android 当前实现和永久 UI 参考，建立功能差异/验证矩阵，独占更新 `apps/ios/UITests/WorkspaceUITests.swift`、新增关键确定性检查和隔离 runner。独立审查实现 worktree 的最终差异并向协调者报告。不得修改 production Swift 或 project.pbxproj，不操作模拟器、服务、用户账号或 Downloads；不 commit/merge。

## 当前状态与证据

基线 `ff3c490`。`WorkspaceScreen.swift` 仍为终端/AI 历史分段侧栏，`ChatPanel.swift` 仍有 Session/Global 切换和旧归档入口，配置为混合 Form。旧 UI 测试断言占位助手且真实服务测试主动退出登录。依据为当前 Android `MainActivity.kt`/`AgentSettingsPanel.kt` 与 `doc/task/0930-app-settings-redesign/{UI_REFERENCE,ANDROID_REFERENCE,IOS_HANDOFF}.md`。

## 方案与执行

复用中央 `/Volumes/Code/My/aTerminal/doc/task/1001-ios-parity/PLAN.md` 批准范围；本次 assignment 明确授权测试和审查，无新增许可问题。

1. 先交付 accessibility 标识/注入接口建议及重大差异。
2. 建立有限矩阵，分开 fixture UI、确定性 production logic、真实 RPC 和协调者模拟器验证。
3. 实现接口确定后更新 UI 测试，新增少量能调用生产逻辑的关键检查，不复制实现算法。真实服务测试须使用专用身份/PTY，无退出或清空身份，不在断言中回显秘密。
4. 实现完成后只读审查另一 worktree 的 diff；运行允许的本地确定性/语法检查，交付明确问题和未实测边界。
5. 协调者追加授权：复用已有fixture路由capture全部20页编号，review完成后提交本任务测试/脚本/文档，不merge；随后单独增加opt-in UUID MCP/Skill真实表单round-trip，资源/配置哈希与失败cleanup由协调者host RPC负责。

## 验证

取消、busy/失败草稿、空密钥/unknown 字段、Azure、能力覆盖/目录分页/迟到屏障、绑定继承、读取、MCP/Skill 合同。协调者负责主 checkout 构建和实际 simulator 测试；本任务不以 fixture 或编译通过冒充真实 RPC/界面实测。

## 风险与回退

无生产编辑。追加review原生editor/clear ae9b4cf、identity drawer 0e6980c、高级展开Button b2d3f3a及冻结main590a4f9，全部production Swift与实现worktree一致，无剩余源码阻塞。SettingsDraft direct10组及正文/绑定/owner checks通过，main最终helper与完整UITest离线typecheck通过。只读协调者日志确认17个不同fixture测试（14首轮+3定向复测）均PASS，真实Desktop路由PASS；MCP/Skill UUID/hash由协调者另验证。已交付7e55a62/b62dd8a，未由本任务merge或操作设备/服务/身份。

## 未决问题、歧义与确认

None.
