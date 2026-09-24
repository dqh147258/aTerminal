# 第二轮移动端与 AI 合同

用户最新要求已授权执行与 Android 真机测试；按此文覆盖首轮 CONTRACT 的“仅建议”和“全屏设置/聊天”。复用原 `assistant(sessionId, requestJson)`，不新增 FFI 函数签名；协调者更新原生库。

## UI

登录页只保留服务、账号、密码和登录（旧配对入口可移至账号的高级操作，不占登录页）。主屏以 Terminal 为绝对主体，远端列宽不重排，可横向滚动。设置与聊天是局部半透明浮窗，竖屏约 60%-75% 可用高度，横屏/键盘下自适应但始终保留终端可见区域，可关闭且不导航到新页面；设置字号和透明度滑块即时生效。文字区可用薄实色底提升可读性，不能把整个浮窗变成不透明整页。

侧滑手势打开工作空间菜单；最后设备/会话按服务+账号持久化，登录/启动后自动恢复在线目标；已关闭会话不假装在线，不随意新建。历史按设备+会话标记在线、已关闭/离线或待确认，未查询过的其他设备会话不能假称在线。保留 Ctrl-C、Enter、键盘开关和用户可发现的退出/设备选择。

## AI 请求

```json
{"action":"send","request_id":"UUID","message":"在 Terminal 中输入 ls /Volumes/Code 然后回车","include_screen":true,"allow_input":true,"monitor":true,"messages":[]}
{"action":"poll","request_id":"UUID"}
{"action":"status"}
{"action":"cancel","request_id":"UUID"}
```

`allow_input` 授权本条聊天消息一次终端文本/Enter 输入；UI 可用“允许操作”开关，默认开启，正在请求时冻结。共享 FFI 在当前会话上取得控制并捕获控制 epoch/输入序号；Desktop 在真正输入前再次检查，人工输入/抢占/切会话使过期 AI 写入失败。两端仍须在原串行 worker 复核 account/device/session，不得将旧消息发到另一终端。

`monitor` 默认开启，替代旧“附带当前终端”控件，文案“监控当前终端”；开启时 `include_screen=true`。用户发送聊天后 Desktop 开始本次有界监控，读取屏幕变化并通过模型给文字说明；最多 5 分钟，最多 30 次模型观察，随后明确暂停，可再发消息继续。关闭浮窗继续监控，切终端停止旧 UI 轮询但 Desktop 保留任务，重新打开/恢复时按原 ID 查询。取消立即禁止后续输入/观察，不发送 Ctrl-C，不声称撤回已入队输入；Ctrl-C 仍由独立显式按钮执行。

## AI 响应

保留 `available,state,message,request_id,reply`，新增 `events`、`monitoring`。状态包含 `running`、`monitoring`、`stopping`、`stopped`、`completed`、`failed`、`unavailable`。`status` 返回当前会话最新活跃任务（存在时），否则 idle。`send`/`poll` 快速返回，不阻塞终端。

```json
{"available":true,"state":"monitoring","message":"正在监控终端","request_id":"UUID","reply":"已写入文本并提交；正在观察输出。","monitoring":true,"events":[{"id":1,"kind":"input","text":"已将文本写入终端并发送 Enter，尚不代表命令完成。","revision":0},{"id":2,"kind":"observation","text":"根据当前屏幕，目录列表已经输出。","revision":42}]}
```

事件 ID 每个 request 单调递增，响应携带最近最多 16 条；客户端为每个 request 持久化 `last_event_id` 并去重追加助手消息。有 events 时不要再重复追加 reply。`running/monitoring/stopping` 持续每秒 poll；completed/failed/stopped/unavailable 停止本次 poll。出现 monitoring 时允许新聊天消息（后端替换旧监控）和停止监控按钮；取消结果持久化。只有确认的会话退出可显示退出码，不能把屏幕静默或模型猜测标为任务成功。

主屏底部或侧边可显示当前 AI 状态。需要终端在线/控制状态与模型状态分开，后台断连后不要继续宣称手机在线。语音仅填草稿，取消和权限错误状态保留。

## 测试分工

协调者提供主仓库更新后的 Native 库和新的 `account_demo --bench --assistant-test`：独立账号服务、真实 Desktop PTY、确定性模型接口，凭据只写私有 fixture 文件。Android、iOS 各使用自己的 fixture 目录与端口，不共用控制会话。

Android 用户已授权连接真机 `dmronjvo9pwsbinf`；iOS 用模拟器。先做现有原生逻辑/布局检查，收到夹具就绪通知后跑真实登录/PTY/AI。真实模型供应商尚在询问配置，未提供时准确记录确定性模型验证，不伪造线上 AI 成功。不要复制或输出既有用户凭据，不 `pm clear`/卸载，不操作其他应用。不要提交、合并或删除工作树。
