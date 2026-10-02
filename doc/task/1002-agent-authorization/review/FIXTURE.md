# 授权验收 fixture 协议

唯一所有者：authorization-review。Android 测试源由 Android 执行者维护。`account_demo` 新增 `--authorization-test`，启动临时账号/Desktop/PTY，使用本地确定性模型；默认 Session cwd 是临时 Desktop 状态目录，bash hooks 显式启用。fixture 不修改用户账号设置或访问付费模型。

## 场景文本

通过 Agent send 发送 `AUTH_REVIEW:` 后直接跟 JSON；id 每个新场景唯一，1–80 个 ASCII 字母/数字/下划线/连字符。步骤按序执行，一个 tool call 得到结果后才发下一个。工具拒绝也算该步骤的结果；fixture 不偷偷重试被拒动作。

```text
AUTH_REVIEW:{"id":"once","steps":[{"tool":"run_command","arguments":{"command":"printf 'once\\n' >> auth-review-once.log"}}]}
```

问答用 `ask_user` 与合同中的字符串 options，完成文本固定 `AUTH_REVIEW_DONE:<id>`。

```text
AUTH_REVIEW:{"id":"question","steps":[{"tool":"ask_user","arguments":{"question":"Choose the fixture answer","options":["alpha","beta"]}}]}
```

命令/任务关联使用精确工具结果引用，值对象 `{"$fixture_ref":{"step":0,"pointer":"/command_id"}}` 会被替换成该步骤真实结果中的 JSON Pointer 值。缺少结果或字段时 fixture 报错，不伪造 ID。若工具真实结果在数组内，可使用 `/items/0/task_id` 等 Pointer。

```json
{
  "id": "command-result",
  "steps": [
    {"tool": "run_command", "arguments": {"command": "printf 'result\\n' >> auth-review-result.log"}},
    {"tool": "wait_command", "arguments": {"command_id": {"$fixture_ref": {"step": 0, "pointer": "/command_id"}}, "timeout_ms": 10000}}
  ]
}
```

Global 场景的终端工具 arguments 必须显式 session_id；Session 场景省略。批量 get/wait 的 task_ids 可以逐项引用前面 send_agent_message 的 `/task_id`。

## Android runner

```sh
cargo +stable build -p ai-terminal --bin aTerminal --example account_demo
python3 scripts/test-android-agent.py --serial emulator-5586 --authorization --output /private/tmp/authorization-ui-evidence
```

执行前约定共享模拟器窗口。runner 安装当前 worktree 的 Debug/appTest APK，并启动 fixture，写入 Android app 私有文件 `agent-ui-fixture.json`，运行 `com.yxf.aterminal.AgentAuthorizationUiTest`。APK 构建和 Rust mobile FFI 同步由协调者/Android 执行者负责。

测试输出 app 私有文件 `authorization-ui-results.json`，至少包含：

```json
{
  "passed": true,
  "real_encrypted_rpc": true,
  "expected_markers": {
    "auth-review-once.log": ["once"],
    "auth-review-denied.log": null
  }
}
```

runner 独立读取临时 Desktop 中各 marker：null 要求文件不存在，字符串数组要求内容 splitlines 后精确相等；同一次重复执行会因多一行失败。marker 路径必须落在该临时 state-dir 内。UI 测试也必须在等待卡片出现后、点击之前观察“尚未执行”，不能只验证结束时的文件。

模型每次请求把场景 ID、是否 analysis、实际工具结果写入临时 Desktop 的 `authorization-model-observations.jsonl`；runner 在成功或失败时都复制它。其记录数可验证等待期间没有无意义模型循环。`AUTH_REVIEW_DONE` 只是模型场景结束，PTY marker、审批状态、实际 RPC 返回和 shell 关联结果才是行为验收依据。

runner 保存 `results.json`、`authorization-pty-markers.json`、`fixture.log`、`instrumentation.log` 和截图。截图沿用 app 私有文件名前缀 `agent-ui-`，场景名支持 `authorization-once`、`authorization-deny`、`authorization-question`、`authorization-full`、`authorization-rules`、`authorization-readonly`、`authorization-timeout`。

## 独立 Rust 验收

可以复用新增 `account_demo_authorization/mod.rs` 的模型 responder，或直接启动 example。使用临时 state-dir 的 `Client` 管理 fixture，用真实 `Account` / `RemoteTerminal.agent` 做加密 RPC；两台临时设备验证重复决定和 revision conflict。直接 Local Client 测试只能注明本地 RPC，不能标作加密跨端验收。

本地设施构建通过不代表新授权实现通过。阶段 2 只在指定集成 commit 上运行，物理设备和线上供应商不在此次证据范围。
