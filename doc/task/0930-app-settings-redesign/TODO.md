# 按最新设计重构移动 App 设置与工作台 UI TODO

- Status: Completed
- Updated: 2026-10-01

## Checklist

- [x] 保存并核对完整的统一原型截图、索引与源文件指纹
- [x] 按最终深蓝亮蓝设计更新 Android 共享视觉与设置首页
- [x] 重构全屏配置表单、绑定、扩展和正确返回路径，保留真实业务能力
- [x] 加入并通过设置保存/错误/异步/导航的关键确定性测试和构建检查
- [x] 审查并 Git 集成改动，在模拟器验证设置各页、键盘、大字体与既有终端/Agent回归
- [x] 永久保存 Android 对照截图、测试结果及 iOS 后续统一参考交接

## Verification evidence

- `reference/` 已保存 20 张 PNG 与页面结构 YAML；`UI_REFERENCE.md` 包含点击路径/例外/两端对应要求，`reference/manifest.json` 记录 8 个有效原型源文件与全部截图 SHA256。
- 已以浏览器实际计算色值核对最终 CSS，避免沿用旧交接文档。侧栏旧手机归档入口明确忽略；登录服务器摘要/编辑交互保留，只统一视觉。
- Android 实施子任务 `20260930-234528-581-android-settings-ui` 在 `worktree/0930-android-settings-ui`，初步表单代码已经审查并反馈保存成功/刷新失败处理，等待完整实现与验证。
- `4b68623` 与审查修复 `635851e` 已通过 Git 快进到 main；独立 Review 的同ID目录新能力覆盖及迟到测试屏障问题已经修复并补用例。
- Kotlin app/test compile、lint 与 Debug/test APK build 通过；指定 `emulator-5586` 上 11 个设置用例、3 个显示/旋转/账号来源/登录用例，以及真实 encrypted RPC 的 AgentReadingUiTest 通过。日志在 `.local/app-settings-redesign/`，后续将复制简洁证据到永久任务目录。
- 真实正常账号 `LiveSettingsExtensionsUiTest` 通过，MCP/Skill CRUD 与完整资源保留、其他配置哈希不变；`evidence/live-extensions.txt`。小屏/大字体/IME 下保存/取消边界 y=572–704，`evidence/small-ime.json`。
- 共用原型及 Android 图集、SHA256 索引、Review、结果和 iOS 后续交接均已持久保存。Android 名称为 aTerminal，旧手机归档不提供，服务器地址原交互保留并统一视觉。
