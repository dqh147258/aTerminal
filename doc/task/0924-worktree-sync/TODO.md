# 同步工作树分支到 main TODO

- Status: Completed
- Updated: 2026-09-24

## Checklist

- [x] 提交三个工作树并检查状态
- [x] 提交主工作区集成快照并记录 tree
- [x] 合并三个分支并处理重复文件
- [x] 验证祖先关系、文件树和所有工作区状态

## Verification evidence

- Android `0caa6d0`、iOS `cd46e4c`、Web Admin `c2bb7af`；三个工作区 `git status --short` 均为空，暂存差异的 `git diff --cached --check` 均通过。
- 主工作区 `06d76cf` 的 tree 为 `165179fdd27ffe9e5a29b6b3cca5e6bf1f9d7736`；`git diff --cached --check` 通过。
- `e9e9f8e`、`2c9d097`、`0d3f524` 合并后 tree 均相同；三个工作树提交均为 `main` 的祖先，合并链与 `06d76cf` 无文件差异。
