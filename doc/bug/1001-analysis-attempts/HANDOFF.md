# 失败分析诊断交接

只修改 `crates/agent-runtime/src/host.rs` 与本目录文档；按本轮新增授权提交，subject 使用 `[未Review]`。未 merge、未操作实际服务/模拟器/Downloads。协调者自行通过 Git 集成并更新运行二进制，本 worktree 没有 Desktop CLI binary 构建。

旧版回读边界：原始终端 observation 已归档；被拒绝模型回复和 subtype 没有保存，无法补回。run 表仅保存 state；Host live error 是 generic `observation_analysis_pending`，没有独立暂停事件。分析前 projection、仅 usage 的 model_usage 也不能恢复被拒绝内容。详见初始只读报告 `/private/tmp/aterminal-interactive-tools-audit.md`。

修复后的每次拒绝挂在原 interaction，历史 `updates` 含 `analysis_attempt`（单个 run/interaction 内 1 或 2）、`error`、`run_id`、`record_id`（被分析 observation）、`rejected_reply_record_id`。同一历史的 `records` 列出 `source=analysis_attempt`。回复 record kind=associated_text/pending=false，继承账号、Session 访问限制和历史保留规则。正文仅原始回复文本前最多 8192 UTF-8 字节，metadata 提供 reply_bytes、reply_truncated、source、attempt/error 等；违规工具只记录前最多 8 个工具名，每个最多 128 字符，不存工具参数。error 文本最多 256 字符。不保存 request、provider 配置、HTTP header 或 reasoning。诊断不会成为新用户指令或待分析 observation，失败仍严格暂停。

在协调者的主仓库、现有业务实例正在运行时使用下面的**已有** CLI；本 executor 未执行这些业务命令。`agents` 为复数；`--state-dir` 必须指向实际运行实例，以下沿用本轮 `.local/local-dev/agent-next`：

```sh
target/debug/aTerminal --state-dir .local/local-dev/agent-next --json agents show --session 18f339c083bd6a0e
target/debug/aTerminal --state-dir .local/local-dev/agent-next --json agents history --session 18f339c083bd6a0e
target/debug/aTerminal --state-dir .local/local-dev/agent-next --json agents record '<rejected_reply_record_id>' --session 18f339c083bd6a0e --part body
```

从 history 的 `result.items[].value.updates[]` 找 analysis_attempt；原始 TUI 用同一 record 命令读取 `record_id`，失败分析回复用 `rejected_reply_record_id`。若返回部分历史，先读 value.record_id 所指完整历史 UUID，或使用 history 的 next_cursor 继续。CLI 内部会 Client::ensure，因此服务停止时不要把它当成完全不启动服务的离线查询。

不触碰服务的只读 SQLite 诊断方式：在主仓库执行下列命令。mode=ro 不创建数据库、不启动服务；仅读取指定 Session 的新诊断记录，不查询凭据库/配置。旧版无 `source=analysis_attempt` 时结果为空。

```sh
python3 - <<'PY'
import json
import sqlite3
from pathlib import Path

db_path = Path('.local/local-dev/agent-next/data/agent.sqlite3').resolve()
db = sqlite3.connect(db_path.as_uri() + '?mode=ro', uri=True)
sql = '''SELECT r.id,r.metadata,CAST(b.body AS TEXT)
FROM records r JOIN blobs b ON b.hash=r.hash
WHERE json_extract(r.scope,'$.session')=?
AND json_extract(r.metadata,'$.source')='analysis_attempt'
ORDER BY r.rowid DESC LIMIT 10'''
for record_id, metadata, reply in db.execute(sql, ('18f339c083bd6a0e',)):
    print(json.dumps({'rejected_reply_record_id':record_id,
                      'metadata':json.loads(metadata),
                      'reply':reply}, ensure_ascii=False))
db.close()
PY
```

验证：36 runtime unit tests + 5 model_boundary、fmt --check、crate clippy --all-targets -- -D warnings、git diff --check 全通过。拒绝分支 `host.rs:1406`/`:1437`、持久化 helper `:1493`；恢复回归包含真实空 TUI 行及全部 UI 没有搜索锚点的断言，无需 schema 迁移。

本轮 initial-attempt 与 recovery-attempt 都在空 Codex TUI record `01a0f544-a806-7375-af7c-dc7f2bb255cf` 暂停，尚未输入绘制任务；已核对主 checkout 的 evidence JSON（只读）。运行中的旧 binary 没有此诊断修复，必须集成并重启专用测试实例，再由协调者重测获得真实 subtype。本修复只恢复诊断可见性，不保证模型以后永不产生无效 JSON；已丢失的 0930/本轮旧回复不能补回。
