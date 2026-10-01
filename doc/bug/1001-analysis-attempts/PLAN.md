# 持久化失败终端观察分析诊断

- Status: Completed
- Updated: 2026-10-01

## 目标与范围

仅修复 agent-runtime 的失败分析诊断丢失：两次失败的 attempt、具体 error subtype、被分析 record UUID 与有界原始回复可从 durable record/历史回读。严格观察分析屏障、两次尝试、取消和恢复不重放写动作的行为保持现有合同。不修改 Desktop/Android、不操作真实服务。按本轮新增授权只在本 worktree 提交，主 checkout 的 Git 同步由协调者执行。

## 当前状态与证据

0930 真实验收在 read_terminal record `01a0f2df-f413-70e0-867b-8686aec86d17` 分析暂停（`doc/task/0930-space-bunny-wait/evidence/initial-attempt/android-workflow.json:18`）。Host `analyze` 仅在局部 entries 中保留 rejected reply/具体 error，成功时才 checkpoint；两次失败后只 `observation_analysis_pending`，故普通历史与 projection 无失败原因。原始终端 observation 仍已归档。

## 方案与执行

2026-10-01 协调者传达用户扩展阻塞工具的授权，并明确要求实施本最小修复、补回归、记录计划后直接执行。本轮授权覆盖以下步骤，无新待确认决定。

同日协调者确认诊断 diff 已 review，授权针对性回归后提交 `host.rs` 及本目录必要文档，subject 明确使用 `[未Review]`。本轮真实 Android 初次与恢复都停在空 Codex TUI observation `01a0f544-a806-7375-af7c-dc7f2bb255cf`，新诊断代码尚未运行；已核对主 checkout 的 initial-attempt/recovery-attempt JSON，并将实际空 TUI 行加入恢复回归，未操作运行服务。

1. 在 Host 两个拒绝分支保存 `associated_text` record，关联原 interaction；元数据为 run/observation UUID、attempt（单轮 1–2）、error subtype、截断标志和必要违规工具名。正文只保存 UTF-8 安全截断的回复文本（最多 8192 字节），不序列化 request、provider 配置、凭据、完整工具参数或 reasoning。
2. 用现有 unit_update/history records 暴露每次失败的 subtype 与原始回复 UUID；无需数据库迁移、新工具或改变 projection/恢复流程。
3. 补两次不同失败原因重启仍可回读、原始文本上限/Unicode 边界、分析违规工具不执行、恢复不重放写入的关键回归。

完成：`host.rs:1406`/`:1437` 两个拒绝分支调用 `:1493` 的持久化 helper。原 analysis_pending、两次重试与恢复流程未更改；新诊断只附着既有 interaction。恢复回归加入实际空 TUI 的 Unicode 行与全部 UI 不可形成搜索锚点的断言；另有有界回复及写入不重放回归。源码仅此一个文件，无数据库迁移。

## 验证

运行 agent-runtime 关键回归与完整 crate 测试、fmt --check、crate clippy。检查 diff 仅 agent-runtime 与本计划文档。真实服务/手机验证由协调者执行；本 executor 不访问真实服务或 Downloads。

验证：`cargo +stable test --locked -p ai-terminal-agent-runtime`（36 unit + 5 model_boundary + doc-tests）；`cargo +stable clippy --locked -p ai-terminal-agent-runtime --all-targets -- -D warnings`；`cargo +stable fmt --all -- --check`；`git diff --check`。诊断读取命令见 `HANDOFF.md`。针对性回归与最终检查通过后按授权提交 `[未Review] fix: persist rejected terminal analysis diagnostics`，不 merge 或操作主 checkout。旧失败的回复已丢失，无法补回；本修复保证新失败可诊断，不改变模型输出成功率。

## 风险与回退

诊断 record 与原 interaction 一起遵循现有访问/保留规则，不作为新 user/assistant 指令。失败仍暂停，不尝试执行被禁止的工具。撤销 Host 诊断追加即可回退，不更改数据库 schema；老失败的已丢失回复不能补回。

## 未决问题、歧义与确认

None.
