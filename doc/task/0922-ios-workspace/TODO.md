# iOS 工作台执行

- [x] 原生登录、工作台、抽屉、设备/会话和账号管理接线
- [x] 显示设置持久化和原生语音草稿
- [x] AI 合同、隔离历史、搜索和继续对话
- [x] 同步绑定，模拟器构建与重点验证
- [x] 整理 HANDOFF 和 CLI 完成报告

验证证据：`build/ios-workspace-acceptance.xcresult` 四项 XCUITest，0 failures；`build/chat-store-checks` 数据隔离和请求边界通过；Debug/Release 模拟器构建通过，截图位于 `build/ios-workspace-screenshots/`。

## 第二轮

- [x] 局部浮窗、全屏终端主体/原列宽横滑、实时字号和边缘侧滑
- [x] 按账号最后终端恢复、会话/历史真实在线状态
- [x] AI 输入/监控/取消合同、事件持久化去重与关闭浮窗持续状态
- [x] 新 UI 和事件边界构建/自动化验证
- [x] 同步新原生库，在指定模拟器真实登录/PTY/AI监控验证
- [x] 更新第二轮结果和 CLI 报告

第二轮证据：`ios-live-final-resume.xcresult` 1项真实全流程通过（0 skipped）；`ios-floating-final-layout.xcresult` 4项UI回归通过；`ios-floating-final-release.log` BUILD SUCCEEDED；独立PTY内AI输出、监控恢复/取消、sleep30-Ctrl-C、关闭历史和退出均已验证。已通知协调者iOS服务可停止，专用Shell已关闭。
