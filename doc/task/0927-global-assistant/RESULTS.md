# 全局AI助手实现结果

## 修改范围
- `MainActivity.kt`：右侧同规格图标入口，终端入口只打开 Session，独立全局列表/聊天导航，系统返回逐级退回。全局聊天背景不透明且占满页面。
- `GlobalConversationPanel.kt`、`GlobalConversationStore.kt`：按服务器、账号、Desktop 缓存列表和已读序号；从真实服务端读取执行状态，分别显示执行中与未读。分页获取全部全局记录，回列表保留滚动位置。
- `AgentPanel.kt`、`AgentDraftStore.kt`：移除 Session/Global 切换；全局 RPC 使用 `agent_id` 和空终端 ID。草稿、图片、历史缓存、阅读位置和迟到回调按全局 ID 隔离。新会话空白，保留图片/语音及发送、追加、停止。输入区域、标题与返回按钮按参考截图调整，低可用高度隐藏辅助输入文案、作用域标签和路径，为消息与输入保留空间。
- `store.rs`：新增幂等创建和分页摘要，复用原 SQLite 事件、任务与 Agent 身份。
- `runtime.rs`、`remote_bridge.rs`：新增 `global_create` / `global_list`，保留账号/Desktop 边界，只读设备不能创建或发送。
- `GlobalAssistantTest.kt`、现有 UI/Rust 测试与 `account_demo.rs`：新增隔离、旧历史、导航、实际阅读和真实 RPC 回归；四条展示记录仅存在 instrumentation fixture 中。

## 旧数据兼容
旧 `(owner, desktop, None)` 绑定和原 `Scope.agent` 原样进入列表，保留消息、记录和任务，不复制或清空。新会话使用独立创建请求绑定；重复请求返回同一 Agent。旧空字符串草稿映射到 `global:<agent_id>`，标记完成后不再覆盖，附件仍使用原私有文件。终端 Session 数据和配置保持原有归属。未读为本机持久阅读状态，不新增跨设备已读同步语义。

## 验证
- Rust runtime 28 项、Desktop 23 项测试通过；包括旧历史重开、幂等创建、跨账号/设备隔离、55 条分页、只读授权。
- Android debug APK、测试 APK、lint 构建通过；lint 0 errors。`cargo fmt --check` 与 `git diff --check` 通过。
- Android 16 `emulator-5586` 的 7 项 UI 测试通过：独立入口/返回、空白会话、执行中与未读并存、实际阅读水位、迟到发送回原会话、草稿/附件隔离、分页、工具详情、横屏及 180dp 高度输入布局回归。最终日志见 `artifacts/global-assistant/instrumentation.log`。低高度回归发现布局回调内修改可见性后原高度仍被保留，已将响应式可见性调整移到布局遍历完成后，确保相关子布局重新测量，并保留严格的可见输入框和可滚动消息区断言。
- `scripts/test-android-agent.py --serial emulator-5586 --output artifacts/global-assistant/agent-e2e` 通过：真实 Android → 加密 RPC → 隔离 Desktop → 本地确定性 HTTP 模型；两个独立全局任务、各自历史、旧默认全局记录、创建幂等性均验证。原终端读取、图片上传/模型视觉字段/二进制证据回归通过。测试保持用户主账号偏好不变。
- 普通沙箱中的端到端测试因进程工作目录观测受限失败；在自动审批允许进程观测的同一隔离测试中通过。未通过删除检查掩盖失败。

## 交付与限制
- APK：`apps/android/app/build/outputs/apk/debug/app-debug.apk`。
- Desktop：`target/debug/aTerminal`。新功能需要同时更新 Desktop 与 APK；未替换或重启正在使用的生产 Desktop 服务。
- 截图与日志：`artifacts/global-assistant/`。这些是运行产物，不纳入 Git。
- 不存在浏览器保存横幅或提示，本次继续保留真实持久化。生产不会插入原型虚构聊天。
- 未测试用户付费模型回复质量、实体手机麦克风/不同输入法；推荐场景见 HANDOFF.md。
