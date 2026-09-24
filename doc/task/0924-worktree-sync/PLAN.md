# 同步工作树分支到 main

- Status: In progress
- Updated: 2026-09-24

## 目标与范围

提交 `worktree/0922-android-workspace`、`worktree/0922-ios-workspace`、`worktree/0922-web-admin` 的现有代码和文档，以及主工作区的现有集成改动；随后把三个分支合并进 `main`。保留三个工作树及分支，不改变已确定的移动端 AI 占位和 Android API 25/x86 范围。

## 当前状态与证据

`git worktree list --porcelain` 与 `git branch -vv` 显示四个分支都在 `24fe3d5`，没有分支提交；四个工作区均有未提交改动。Web Admin 工作树的 25 个改动文件与主工作区逐字节相同。Android 工作树有 34 个改动文件，其中 29 个相同、5 个不同；iOS 有 19 个，其中 13 个相同、6 个不同。差异对应父计划 `doc/task/0922-mobile-admin/PLAN.md` 已记录的 AI 占位范围及 `doc/task/0923-android-api25-x86/PLAN.md` 的平台兼容修正。主工作区还包含 Rust 共享模块、部署说明及构建脚本的集成改动。

## 方案与执行

用户本轮已明确要求提交两个层面的工作区并同步至 `main`，此处记录执行授权。

1. 在三个工作树分别提交现有改动，检查各自工作区干净。
2. 在主工作区提交当前集成改动及本计划，记录该提交的树对象。
3. 将三个工作树分支逐一合并到 `main`。重复文件保留主工作区已集成版本；Android/iOS 的 11 个差异文件保留父计划的最新目标版本。记录每个分支的合并关系，不改动功能代码。
4. 检查所有分支均成为 `main` 祖先、四个工作区干净，且最终 `main` 树与集成提交的树相同。

## 验证

提交前运行 `git diff --check`；合并后运行 `git status --short`、`git merge-base --is-ancestor` 和树对象比较。既有父任务及 API 25 验收文档已记录 Rust、Android、iOS 与 Admin 功能验证；本次仅改变 Git 提交图且要求最终文件树完全相同，因此不重复设备测试。

## 风险与回退

两个移动工作树保留早期 AI 实现，主工作区采用后续批准的占位状态；合并时若自动结果改变集成文件树，应恢复主工作区版本并再次核对。所有提交和分支保留，必要时可用合并前 `main` 提交定位原始集成状态。

## 未决问题、歧义与确认

None.
