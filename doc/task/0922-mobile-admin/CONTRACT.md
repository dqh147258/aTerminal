# 移动 AI 接口合同

协调者负责实现并生成绑定。两端可先开发 UI，待通知后复制更新的生成绑定/原生库。

新增 UniFFI 方法：`RemoteTerminal.assistant(session_id: String, request_json: String) -> String`。
Swift 为 `assistant(sessionId:requestJson:) throws -> String`，Kotlin 为 `assistant(sessionId, requestJson): String`。
方法运行于后台 worker，结果为 JSON。`session_id` 必须明确给出，不依赖当前选择，以免切换会话导致串线。

请求：

```json
{"action":"status"}
{"action":"send","request_id":"随机 UUID","message":"解释当前错误","include_screen":true,"messages":[{"role":"user","content":"前文"},{"role":"assistant","content":"回答"}]}
{"action":"poll","request_id":"之前提交的 UUID"}
```

响应：

```json
{"available":true,"state":"idle","message":"","request_id":"","reply":""}
{"available":true,"state":"running","message":"","request_id":"UUID","reply":""}
{"available":true,"state":"completed","message":"","request_id":"UUID","reply":"模型回复"}
{"available":true,"state":"failed","message":"错误原因","request_id":"UUID","reply":""}
{"available":false,"state":"unavailable","message":"Desktop 尚未配置 AI","request_id":"","reply":""}
```

`send` 快速返回；`poll` 每 1 秒一次，离开会话停止 UI 轮询，回到会话可续查。请求 ID 用于去重，同 ID 不可变更内容，不自动重发结果未知请求。每条用户消息最长 4000 字符，前文最多 12 条且整体请求小于 16000 UTF-8 字节。Desktop 模型请求超时约 60 秒。

`include_screen` 默认为 false，对应显式开关“附带当前终端”。开启时发送当前屏幕文本给模型；不要自动后台监控，不声称当前 AI 正在监控。对话按服务地址、账号、device_id、session_id 隔离，在客户端私有存储持久化，历史可以离线阅读；退出账号清除可见状态，不允许下一账号读前一账号历史。凭据只在 Desktop，客户端无模型 key 设置。

模型仅返回文本，不自动执行。提供复制回答；用户需要执行时进入既有终端输入流程并确认，不从 Markdown 自动运行代码。语音仅填入草稿，不自动提交。未配置/断网/权限拒绝均有真实状态，不放演示对话冒充真实数据。

Desktop 配置：`AI_TERMINAL_AI_BASE_URL`（以 `/v1` 结尾的兼容端点）、`AI_TERMINAL_AI_MODEL`、`AI_TERMINAL_AI_API_KEY`。HTTPS 或回环 HTTP；密钥不经过 Server 或 Mobile。
