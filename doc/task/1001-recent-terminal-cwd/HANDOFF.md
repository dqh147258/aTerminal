# 协调者交接

实现与定向验证已完成，详见 RESULTS.md。协调者已确认代码及验证已 review，并授权本轮以 `[未Review]` subject 提交约定源码与必要文档；本执行者只提交，主 checkout 的 Git 合并由协调者处理。主 checkout 的服务、真实 test-1001 Session、AndroidTest harness 与模拟器仍由协调者统筹。

## 建议验收

1. 在真实 Desktop Terminal 进入不同目录，等待一轮采样（约 2 秒加 OS 查询），手机打开新建会话应显示最近目录；同目录重复使用应去重，另一个 idle 终端不会改变排序。
2. 选择含空格/特殊字符目录后创建，Desktop 新会话的真实 cwd 应一致；输入为空或点“默认目录”仍使用原 Desktop 默认 cwd，不自动采用第一条最近目录。
3. 先记录目录后删除它，再从最近项选择创建：应显示 Desktop 错误、保留路径、恢复按钮；不能偷偷创建默认目录。读取历史失败仍可手填或默认创建。
4. 检查小屏、横屏、大字体、键盘打开时列表可滚动、取消/创建可达；Android 对照 0930-app-settings-redesign 的信息行/蓝色主题。iOS 复用仓库现有 WorkspaceStyle，不全局重绘旧设置主题。
5. 切换账号/设备、关闭弹窗或断开连接后，迟到列表/创建回调不得填入另一连接；重开应读取当前 Desktop/账号记录。
6. 使用旧 Desktop 验证新 UI 的空最近列表与默认/手填创建；使用旧 mobile 验证新 Desktop 的 List/Create 行为不变。必要时用隔离 fixture 复核持久化跨 Desktop 重启，不必为此干扰当前真实服务。

## 边界

- 后台采样不捕获所有极短暂 cwd；OS 身份/cwd 不可用时不记录猜测值。Windows 当前只有成功启动 cwd，沿用现有 process::cwd 不支持的事实。
- 删除后的最近目录保留供创建时返回明确失败；本轮没有管理/清空历史 UI。
- Rust、Kotlin、Swift 验证通过不等于模拟器视觉或完整链接验收。需要重新生成 UniFFI 并配套构建 native 库，再安装测试。
- AndroidTest 的 `TerminalAgentWorkflowUiTest.kt` 未修改。
