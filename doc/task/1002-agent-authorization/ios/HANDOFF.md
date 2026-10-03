# 当前 iOS 阻塞与证据

2026-10-03 当前有效摘要：iOS 分工完成。稳定 presentation 产品源码 `d4f9021` 的必要检查全部通过：build-for-testing、聚焦键盘生命周期 1/1、最终本地原生 9/9、完整真实加密 UI/RPC 1/1（389.911s），以及 runner 独立文件内容/计数核验。最终详细证据见 [VERIFICATION.json](VERIFICATION.json)。不再有本任务 Xcode/Simulator/fixture测试进程；独占窗口释放给 root。服务 pid56639 由 root 拥有/停止，用户原设备和账号未修改。

稳定 `ChatPanel` 持有 sheet；Lazy 卡片只发送 scope 快照的打开意图。`AssistantModel` 拥有 `AgentInteractionState`，presentation 固定 scope/pending/context，草稿按 scope/pending 分开；手动关闭和失败保留，已成功完整 pending 读取确认 consumed/消失后清除。scope/account/connection 变化即时清 presentation，提交异步任务启动前再次校验旧 presentation。详情 sheet 同样移出虚拟行，普通参数完整入口保留。

已通过 Foundation 证据：卡片值重建不销毁编辑状态、关闭再打开保留草稿、失败不清、账号/连接旧编辑无效、同 pending ID 跨账号不共享草稿、取消未消费保留、消费与迟到编辑不恢复旧草稿。原授权/CAS/详情/撤销检查仍通过。现有7+2本地 UI 中问答用例的关闭再开保留选项、键盘激活后 editor 仍存在断言，已在聚焦与最终9项中实际通过。

最终使用 root 最新 SERVICE.json 的私有服务 final-service-ojkda0dn / pid56639，example SHA256=05bcb1bd04ed7e984a3774ce4115a8355fa55b91cb22ed0c0a563ca435e3a5a3；未重建 Rust，CLI仍为已验证生产后端45ab2d4。下面保留的是修复前失败证据和定位历史，不代表当前阻塞或需要再重跑。

修复前产品源码 `cf01e95`，RPC 测试的前置待办定位审计 `193a18b`；最新稳定修复提交由 SQLite checkpoint/阶段 message 记录。此前阶段提交为 3f7a2b0 / 39a1ab7 / c22735d / bc3cd48。源码范围仅 iOS、专属脚本/说明，worktree 不合 main。

## 修复前历史：真实失败与定位

空历史服务由 root 创建并拥有：SERVICE.json 指向 fresh-jcrw0hv_，pid76779。最新测试 `build/authorization-rpc-fresh-final-v3`，prefix `ios-auth-04dbc8bb6a5c`。失败：WorkspaceUITests.swift 问答 `UITestInput.replace` 报 `Input keyboard is unavailable`，没有提交答复或通过直接 RPC 代答。

只读实际 Host 状态保存 `build/authorization-rpc-fresh-final-v3/host-state.json`：state=waiting_for_user，error=null，can_mutate=true，ask/full=false，run=01a0fe20-51ea-770a-899d-7b85917344e6。DB 当前 pending 为 question；这不是 compression 或许可失败。

录像导出 `build/fresh-keyboard-failure.mp4`；`build/fresh-question-form-68.png` 显示 Form 和已选 Markdown，`build/fresh-question-form-70.png` 显示输入点击后 Form 消失、回到长历史，键盘未出现。当前 sheet 在 LazyVStack 的 AgentPendingCard/Button 上；键盘改变底层聊天几何后宿主卡片可能被虚拟化回收。推荐最小修复：sheet 挂在稳定 ChatPanel（或更稳定 WorkspaceScreen）上，Presentation 包含 pending 与目标 Scope 快照，草稿在 AssistantModel 按 scope/pending ID 保存；卡片只发打开意图。不要通过延超时、坐标点击或直接 resolve 绕过。

## 修复前阶段检查（已被最终证据取代）

- 生产 Foundation 控制器行为检查通过，包括 CAS/幂等重试、regrant→revoke、丢失 revoke reply 后恢复、full、readonly grant/缺字段、账号/epoch、长详情 fingerprint/ACK/deny。
- 最新 build-for-testing 成功，Xcode jobs2/parallel testing NO，仅自建 SE 079C5369-F052-45A8-A767-70B1A9FA6707。
- 本地 9 项中 8 项在 sheet 版通过；XL stop 结果原先在视区外，聚焦最终复测 `build/authorization-xl-stop-final.xcresult` 1/1通过。更早键盘/旧内联版曾 9/9，但不冒充当前整套/真实验收。
- fresh 第一轮真实 once/recovery、deny、question选项→清空→自由答复→模型DONE通过；长详情卡仍未走 jump 的残留测试断言失败（合法 waiting，未执行 long）。后续已审计所有前置断言，统一先实际 pending jump；最新失败为上面的 Form 生命周期。
- 旧服务 final-ohadryvi 因10轮历史触发 compression_tools_forbidden；实际答复已consumed且正确，root已停止旧服务并保存日志/DB。与最新 UI 失败分开。
- 旧 v5 的 native always/repeat/regrant 三行及两次 revoke、full 首次 PTY行曾观察到，但最终完整 fresh 序列/long 精确内容及full跨Run断言尚未完成。不得宣称完整加密 UI通过。

当前没有新的 Xcode/fixture任务。root拥有服务停止与窗口调度，Android 释放后通知执行必要验证；执行者保留账号凭据在原私有 fixture，不输出或提交密码，不动用户生产环境。

## 最终验收与交付

- 最新源码 build：`build/authorization-stable-final-build.log`。
- Foundation生产状态机：`python3 scripts/check-ios-authorization.py`，授权与 scoped draft/presentation全部通过。
- 聚焦 keyboard：`build/authorization-stable-sheet-focused.xcresult`，1/1。
- 最终本地 UI：`build/authorization-stable-final-local9.xcresult`，9/9，含7授权场景、草稿隔离和关闭Session历史正文检索。
- 真实用户 UI / 加密 Agent RPC：`build/authorization-stable-final-rpc/rpc.xcresult`，1/1；测试通过可见按钮发送/审批/问答/详情/权限/规则，不通过直接RPC替代UI。prefix ios-auth-20793b75d54f。
- `results.json` / `actual-markers.json`：once=1行5字节，native always=3行21字节（首次/重复/重新授予），full=2行10字节（两个Run），long=6651字节与合成stdin精确一致；deny/full-off文件不存在。点击前独立验证 marker 尚未出现，Native regrant/revoke两次均确认规则消失。实际问答 consumed/answer=iOS custom answer。
- 真实完整流程含 pending panel reopen恢复、选项→清空→自由答复+模型继续、完整已脱敏长详情与fingerprint ACK、once/always/deny/full跨Run和关闭后重新审批、规则撤销再授撤销。设备readonly/legacy UI与scope/account/epoch/draft隔离分别由本地UI和生产Foundation行为测试覆盖；后端真实grant隔离由root独立Review验收覆盖，不将本地stub宣传为另一台真实iOS设备。
- Runner finally现在成功/失败都保存仅本轮prefix合成marker与原model观测，移除Simulator测试登录JSON，不复制账号DB/密码token。旧失败目录/录像保留，不覆盖成功结论。
- 只做专用SE模拟器、私有本地确定性模型；没有物理设备或真实付费供应商验证。完整OS/目录沙箱不属于本任务。

Root下一步：安全Git整合最后runner/证据提交、最终main检查并停止root-owned私有服务，之后按既有指令关闭本子任务终端。执行者不合main、不删除worktree/branch、不关闭终端。
