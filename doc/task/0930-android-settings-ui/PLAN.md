# Android 设置 UI 实施

- Status: Completed
- Updated: 2026-10-01

## 目标与范围
实施主计划批准的 Android 共享主题、设置首页、LLM/读取/MCP/Skills 全屏表单、账号展示与返回。保持真实 RPC、配置字段、凭据和终端语义；不改 iOS/Rust，不启动服务、浏览器或模拟器。

## 当前状态与证据
`AgentSettingsPanel.kt` 编辑弹窗自动退出、冲突重渲染丢草稿，默认绑定平铺；`MainActivity.kt` 设置详情无内部返回；`NativeUi.kt` 配色过时。依据主 checkout `doc/task/0930-app-settings-redesign/{PLAN,UI_REFERENCE,AUDIT}.md` 与永久 reference PNG。

## 方案与执行
用户已明确批准主计划“按 UI 计划执行”，本子计划复用该批准范围。单一实施者依次建立设置信息行和主题、面板内部父页导航与忙碌/异步身份、迁移全部现有配置流程、加入契约回归并编译。主协调者负责集成、模拟器与图集验证。仅提交本任务文件，subject 使用 `[未Review]`。

## 验证
静态编译与 diff 检查；受控 RPC instrumentation 覆盖导航、局部校验、失败草稿、busy、Azure、模型重置、绑定删除覆盖、旧字段/空密钥/修订及目录迟到。没有运行设备测试不得声称通过。

## 风险与回退
异步响应与页面身份、冲突后草稿合并、系统文件选择器回调是重点。保留完整文件夹上传限制和原协议，按 Git 提交可回退。

实施与受控测试已完成，Debug/AndroidTest Kotlin 编译、lint 与 diff 检查通过。设备 instrumentation/视觉截图由协调者集成后执行，详见 HANDOFF.md；不宣称设备测试通过。

2026-10-01 按独立 review 修复 R1（同 ID 目录能力优先于旧 snapshot）及 R2（迟到测试串行 worker/UI 屏障）；原批准范围内修正。

## 未决问题、歧义与确认
None.
