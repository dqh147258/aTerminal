# 本次任务 Git 提交复审与修复

- Date: 2026-10-01
- Reviewed range: `cb4fbad..9d0d4e7`
- Status: Reviewed; confirmed issues fixed and regression checks passed.

## 提交范围

| 提交 | 内容 | 复审结果 |
| --- | --- | --- |
| c314d91 | 纯延时 MCP、只读、取消与 Run 剩余时限 | 未发现新增功能问题；原有回归与实现一致。 |
| c823e8c | Android/Codex 真实任务测试 | 启动断言会误接受版本查询，已在本轮收紧。 |
| 749a5b2 | 合并独立 MCP 与 Android 测试分支 | 两个父提交正确，未发现丢失变更或混入无关源文件。 |
| 4b68623 | Android 主题、全屏设置、导航与配置表单 | 新增 Windows 路径误拒绝和删除冲突后操作问题，已修复。 |
| 635851e | 同ID目录能力及迟到回调测试修复 | 原 R1/R2 已解决，本轮不重复报告旧缺陷。 |
| dbf97e3 | 真实 MCP/Skill 表单回路 | 未发现新增问题；清理仅作用本轮 UUID 配置。 |
| 9d0d4e7 | 品牌、原生主题、恢复测试及永久截图 | 主题/API25 与品牌范围正确；图片指纹、链接有效，文档未发现 OpenRouter 密钥内容。历史证据保留首次暂停与恢复边界。 |

## 已修复的确认问题

1. **P2：Windows Desktop Skill 路径被 Android 本地校验误拒绝。** 原仅接受 `/`，导致 `C:\Users\Carl\skills\sample` 和 UNC 路径无法发到 Desktop。新增平台路径预校验支持 Unix、盘符绝对路径和 UNC；空、NUL、相对及盘符相对路径仍拒绝，真实文件与包校验继续由 Desktop 执行。
2. **P2：扩展在其他客户端删除后，保留详情重试崩溃或重新创建。** revision conflict 刷新后详情仍存在，再启停会在 UI 线程 getJSONObject(id) 抛错，MCP 编辑重试还会 put 回已经删除的条目。统一存在性检查保护详情读取、启停、删除及编辑保存；保持草稿并提示返回检查，不发新修改。启停原有目标语义保持，并加回归证明其他客户端已应用目标时不会反向切换。
3. **P2：Codex 验收启动断言可能假通过。** 原 regex 接受 `command -v codex; codex --version`，还未检查 submit 或 input 结果。现在必须是 `codex exec`，有成功接受且提交的输入或单独 Enter，并有其后关联到同一 Session/interaction 的不可变 Terminal 启动/完成原文。版本探测、引号中的文字、仅键入、被拒绝输入与助手自述不作为启动证据。工具调用/结果采用完整有序 records 索引，避免 bounded updates 尾部截断影响关联。

明确保留隐藏 `read_only` 的现有权限语义，不把模型能力声明当作新的写权限授权。未改变 Rust wait 或本轮配置/凭据默认值。

## 验证

- `assembleDebug`、`assembleDebugAndroidTest`、`lintDebug --offline` 通过。
- 指定 Android 16 `emulator-5586`：`AgentSettingsUiTest` **17/17** 通过（含新增 6 项），两个不调用模型的 Codex 证据回归 **2/2** 通过。
- 同一 Codex 证据 helper 的临时 Kotlin/JUnit harness **2/2** 通过；未重新发起真实模型任务。
- `git diff --check` 通过；20 原型与 Android 截图 manifest SHA256、文档链接检查通过。

本复审为源码/合同检查和针对性验证，未宣称 Windows 真机、SAF 系统文件选择器或所有任意 Shell 语法均已实测。历史提交的 `[未Review]` 是创建时状态，本轮不改写已有 Git 历史。
