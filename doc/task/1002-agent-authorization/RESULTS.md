# 实现与验证结果

状态：Completed，2026-10-03。最终功能代码与移动脚本已整合 main，必要检查均通过；用户确认的范围和 High 测试强度已落实。被测源码提交 `f63ccf5970eb3fbad7288575a786669ae488eb9b`，之后只有本任务证据/文档更新。

默认 ask：内置观察和严格原生安全读取自动执行，其他动作由真实用户一次授权、精确永久授权或拒绝。当前对话的全授权开关持续至手动关闭，委托任务继承；设备/账号只读、Session 绑定、人工输入、版本、取消和预算仍有效。没有增加工作目录/OS 沙箱，旧 allow_input=true 只映射 ask。

可靠原生叶子程序通过 run_program 获得绑定程序内容、参数、stdin、实际 cwd 和版本的可撤销永久规则。原 PTY 的 run_command/input_text/send_keys 与无法固定依赖的 MCP/泛用 Skill 只提供一次或全授权，界面说明原因；避免实际 Shell 函数/别名替换和包 helper 变更绕过审批。规则使用 v3，旧宽松规则不生效。

六类能力已补齐：确切命令结果、批量委托、用户问答、历史检索、增量读取/等待与能力查询。完整内置目录为 Session 21 / Global 29，见 [TOOLS.md](TOOLS.md)。授权与问答使用持久化用户 RPC，模型没有自批/开启全授权的工具。

| 最终检查 | 实际结果 |
| --- | --- |
| main Rust lib | Desktop 59/59、Runtime 100/100，0忽略；[日志](main/evidence/rust-lib.log) |
| main CLI与加密RPC | CLI参数16/16、真实CLI3/3、独立加密18/18，0忽略，106.70s；[日志](main/evidence/cli-and-encrypted.log) |
| main严格静态检查 | 两lib及CLI/bin/example/两集成测试 clippy -D warnings；fmt、三个runner AST、diff检查通过 |
| main Android | assembleDebug/AndroidTest/lint通过；隔离包实际29/29授权/聊天回归，73.271s；[日志](main/evidence/android-main-ui.log) |
| Android真实UI/RPC | 最终415a5c3，1执行/0忽略，119.11s；一次/重放、原生永久三行、再授再撤销新nonce、cwd、Global/full隔离、问答/IME、长详情、取消和闭Session管理/确切拒绝验证通过 |
| iOS源码与构建 | 产品d4f9021、最终37fa1c3；build-for-testing、主线Foundation与scope/草稿/授权行为检查通过 |
| iOS原生UI/RPC | 本地9/9、键盘聚焦1/1、加密1/1（389.911s）；once/always/full/long精确文件、deny/full-off无文件、实际问答consumed；[证据](ios/VERIFICATION.json) |
| 源码整合 | 所有最终提交为main祖先，移动端源码/脚本与被测分支无差异；生成JNI/bindings/fixture与验证版本一致 |

独立后端审查与完整marker证据见 [review/RESULTS.md](review/RESULTS.md)；root已关闭其转交的Android R20/R21和iOS必要事项，最终源码/视觉审查见 [main/MOBILE_REVIEW.md](main/MOBILE_REVIEW.md)。Android鲜图确认回答与提交按钮连续两个轮询稳定地同现在IME上方；iOS问答窗口移至稳定父视图，失败/关闭保留草稿，账户/范围/连接变化防止旧编辑提交。

闭Session负向检查证明未写入：同账号/Desktop正常重连后查询、真实答复、设置可用；原写入被unknown-action安全屏障拒绝，观察不存在PTY也被明确拒绝，实际marker不存在。没有把模型成功口令或任意异常当执行成功。未验证关闭后原连接持续在线或Android真实detached传输；底层detach相关检查和本地UI另有证据。

不包含物理设备、付费/线上模型供应商或PowerShell运行验收；本任务不承诺同UID授权程序的OS隔离，也没有通用TUI/后台应用完成适配器。以上边界与已批准测试安排一致。

全部六个子任务已completed并关闭其Codex/辅助终端，liveness均dead；root的私有iOS服务及daemon已停止并通过准确进程/endpoint检查。工作树/分支保留完整xcresult与原失败资料，正常账号、服务和无关工作树未用于验收。早期失败/修复历史保存在 [RESULTS_HISTORY.md](RESULTS_HISTORY.md) 与各端工作树build目录，不把失败轮次计为通过。
