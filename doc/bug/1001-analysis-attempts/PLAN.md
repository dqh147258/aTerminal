# 持久化失败终端观察分析诊断

- Status: Completed
- Updated: 2026-10-01

## 目标与范围

原诊断阶段已保存失败 attempt、subtype、原文 UUID 与有界回复，并以 `6aae60f` 提交。本阶段基于 U+200A 转抄失真，支持有界可见原文完整行引用，精确展开后继续严格校验，兼容旧 text 格式。分析屏障、两次尝试、取消和恢复不重放保持现有合同；不修改 Desktop/Android、不操作真实服务。最终验证后按新增授权仅提交本 worktree 的必要文件，主 checkout Git 同步由协调者执行。

## 当前状态与证据

0930 真实验收在 read_terminal record `01a0f2df-f413-70e0-867b-8686aec86d17` 分析暂停（`doc/task/0930-space-bunny-wait/evidence/initial-attempt/android-workflow.json:18`）。Host `analyze` 仅在局部 entries 中保留 rejected reply/具体 error，成功时才 checkpoint；两次失败后只 `observation_analysis_pending`，故普通历史与 projection 无失败原因。原始终端 observation 仍已归档。

## 方案与执行

2026-10-01 协调者传达用户扩展阻塞工具的授权，并明确要求实施本最小修复、补回归、记录计划后直接执行。本轮授权覆盖以下步骤，无新待确认决定。

同日协调者确认诊断 diff 已 review，授权针对性回归后提交 `host.rs` 及本目录必要文档，subject 明确使用 `[未Review]`。本轮真实 Android 初次与恢复都停在空 Codex TUI observation `01a0f544-a806-7375-af7c-dc7f2bb255cf`，新诊断代码尚未运行；已核对主 checkout 的 initial-attempt/recovery-attempt JSON，并将实际空 TUI 行加入恢复回归，未操作运行服务。

后续原文行引用修正授权（2026-10-01）：主 checkout `doc/task/1001-interactive-pelican/evidence/diagnostics-recovery/rejected-analysis.json` 已定位 `invalid_analysis_quote` 与 `unverified_analysis_evidence`：模型把 Codex 更新行 emoji 后的 U+200A 改为普通空格，重试仍复用无效 fact。协调者再次明确恢复 running、授权直接实施确定性修复，只修改 agent-runtime 必要文件及本计划目录；本阶段先报告 diff review，不 commit/merge，不操作真实环境。focus 通知 preempt 由其他子任务负责。

最终提交授权（同日）：协调者已初步 review analysis/host/model diff，确认精确展开、可见限制、legacy 与错误引用拒绝符合授权；明确要求最终测试/fmt/clippy 后将新 `analysis.rs`、`lib.rs`、`host.rs`、`model.rs` 和必要文档提交，subject 为 `[未Review]`，无需再请求许可。本阶段 commit 限制被此后续授权替代；不 merge，不操作真实服务。

1. 在 Host 两个拒绝分支保存 `associated_text` record，关联原 interaction；元数据为 run/observation UUID、attempt（单轮 1–2）、error subtype、截断标志和必要违规工具名。正文只保存 UTF-8 安全截断的回复文本（最多 8192 字节），不序列化 request、provider 配置、凭据、完整工具参数或 reasoning。
2. 用现有 unit_update/history records 暴露每次失败的 subtype 与原始回复 UUID；无需数据库迁移、新工具或改变 projection/恢复流程。
3. 补两次不同失败原因重启仍可回读、原始文本上限/Unicode 边界、分析违规工具不执行、恢复不重放写入的关键回归。
4. 在分析前由 Host 从当前 observation 的可见正文与 immutable 原文构造有界完整行表（最多 256 行、16 KiB），编号为原文中 1-based 行号。部分页排除首尾残行，fragment 或无法确定原文位置时不提供不可验证行；binary 不提供行引用。
5. key_quotes/facts.evidence 保留旧 text 格式，并新增 `{record_id,line_start,line_end}`（inclusive）引用；tui_lines 保留旧字符串，并接受同一引用对象。Host 只展开行表内的完整原文，再走原有严格校验；拒绝错误 UUID、越界/未展示行、反向/非整数范围及 text/ref 混用，不做 Unicode/空白归一化。更新分析及重试指令优先引用；保留失败诊断、两次尝试、取消和不重放语义。
6. 覆盖真实 hair-space/Unicode TUI、跨行引用、错误/歧义引用、partial 首尾与可见范围、binary 禁用、有界表、旧 text/string 兼容，并运行 runtime 测试/fmt/clippy 后报告未提交 diff。

诊断阶段完成：Host 两个拒绝分支持久化有界回复及具体 error；原 analysis_pending、两次重试与恢复流程未更改，诊断只附着既有 interaction。该阶段已提交 `6aae60f`，无数据库迁移。

原文引用阶段完成：新增 `analysis.rs` 提供最多 256 行/16 KiB 的原文表和严格引用展开；`host.rs:1361` 在分析前提供表，`:1436` 展开后仍调用既有 Store 严格校验，`:1498` 处理真实 record/page/binary 可见边界；`model.rs:82` 优先指导引用。旧 quote/fact text 和 TUI 字符串不归一化，错误 Unicode 变体仍拒绝；新引用拒绝错 UUID、非整数/反向/缺失/越界范围和 text 混用。诊断保存保留。

## 验证

运行 agent-runtime 关键回归与完整 crate 测试、fmt --check、crate clippy。检查 diff 仅 agent-runtime 与本计划文档。真实服务/手机验证由协调者执行；本 executor 不访问真实服务或 Downloads。

最终引用阶段验证全部通过：`cargo +stable test --locked -p ai-terminal-agent-runtime`（44 unit + 5 model_boundary + doc-tests）；`cargo +stable clippy --locked -p ai-terminal-agent-runtime --all-targets -- -D warnings`；`cargo +stable fmt --all -- --check`；`git diff --check`。8 个新增关键回归覆盖真实 hair-space/TUI、旧 text 精确兼容、引用拒绝、表大小/行数、partial 首尾与重叠歧义、Host 实际 binary/partial 限制、分析屏障与 durable 拒绝记录。既有 cancel/不重放/provider 边界回归仍通过。按最终授权提交 `[未Review] fix: resolve analysis evidence from exact visible lines`，不 merge 或操作主 checkout；真实模型重测由协调者执行。

## 风险与回退

诊断和原文表继承现有 Scope/record 访问规则，不作为新 user/assistant 指令。表内引用展开后仍严格验证，失败仍暂停。binary 禁用行引用；partial 残行与未展示行不能引用。表达到上限时优先保留最近完整行，省略的行不可引用；legacy text 仍须逐字准确。当前修改不改变数据库 schema；回退本引用提交不会撤销此前 `6aae60f` 的诊断能力。真实模型行为须协调者验证，老失败的已丢失回复不能补回。

## 未决问题、歧义与确认

None.
