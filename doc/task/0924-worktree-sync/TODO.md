# 同步工作树分支到 main TODO

- Status: In progress
- Updated: 2026-09-24

## Checklist

- [x] 提交三个工作树并检查状态
- [ ] 提交主工作区集成快照并记录 tree
- [ ] 合并三个分支并处理重复文件
- [ ] 验证祖先关系、文件树和所有工作区状态

## Verification evidence

- Android `0caa6d0`、iOS `cd46e4c`、Web Admin `c2bb7af`；三个工作区 `git status --short` 均为空，暂存差异的 `git diff --cached --check` 均通过。
