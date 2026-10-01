# 已完成子任务清理

2026-10-01 按用户明确要求执行。清理前逐项核对：任务 completed、源码无未提交/未跟踪内容、分支 HEAD 已在 main；先用 worktree-tasks 关闭登记 Codex 与配套终端，再删除工作树和已合并分支。未关闭父窗格、共享服务或字符动画 Terminal。

已清理当前 aTerminal 下 8 项：`0930-space-bunny-audit`、`0930-pure-wait-mcp`、`0930-ui-settings-audit`、`0930-android-settings-ui`、`1001-interactive-workflow`、`1001-interactive-tools-audit`、`1001-recent-terminal-cwd`、`1001-terminal-focus-fence`。

旧项目名 AITerminal 下修复了 3 个错误 Git 链接以便安全检查。旧数据库证明 `0922-web-admin` 与 `0922-ios-workspace` completed，源码干净且合并；关闭登记窗格并清理工作树/分支。旧 iOS 最终 xcresult、截图与日志已选择性保存到忽略的私有目录 `.local/ios-parity/legacy-evidence/0922-ios-workspace`，未复制凭据或 Git 元数据。

`0922-android-workspace` 的任务记录是 interrupted，保留工作树与分支；不能将 merged 或进程退出代替任务完成证据。两个项目的 SQLite 任务历史均保留。

新的 iOS 同步任务：`20261001-155711-983-ios-parity`。仅新建 `1001-ios-parity-ui` 与 `1001-ios-parity-checks` 工作树；未复用旧任务目录。完整清理审计和关闭结果位于忽略的 `.local/ios-parity/`。
# iOS 同步完成后的清理

本轮实现与独立审查均已完成，四个登记窗格（两个 Codex、两个辅助 Terminal）由 worktree-tasks 的 `subtask close` 按各自 GUID 关闭，返回 `close.errors=[]`。两处工作树 `git status --short` 均为空；最后提交分别为 `6dcec6b` 和 `2f1d382`，已通过 `git merge-base --is-ancestor <branch> main` 明确验证。

已移除 `/Volumes/Code/public-worktree/aTerminal/1001-ios-parity-ui` / `worktree/1001-ios-parity-ui` 与 `/Volumes/Code/public-worktree/aTerminal/1001-ios-parity-checks` / `worktree/1001-ios-parity-checks`。最终源码、审查、测试和永久截图均在 main，任务 SQLite 历史保留。加上同步前的 10 处，共清理 12 个已完成 Worktree 及已合并分支。

当前 Git Worktree 只保留主 checkout 和未完成的 `/Volumes/Code/public-worktree/AITerminal/0922-android-workspace`（`interrupted`），没有将其误当完成删除。原动画 Session、共享 Desktop/server/Android 未停止。关闭记录和分支合入审计保存在忽略目录 `.local/ios-parity/final-close-*.json` 与 `final-branch-audit.json`。
