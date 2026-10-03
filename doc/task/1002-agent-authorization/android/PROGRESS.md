# Android 授权与问答

状态：Completed（Android 执行者范围；整体跨端验收仍由协调者完成）。沿用协调者 PLAN.md / CONTRACT.md 与用户全面执行授权；High 覆盖。当前 run：c0ea835a-41c8-447a-a34c-fd46b0befc01。

Android 源码已实现：

- `AgentPanel` 新任务显式发送服务端确认的 `permission_mode`，默认 ask；不读旧 allow_input/full 草稿升级授权。`waiting_for_user` 可停止/追加。
- `AgentAuthorizationPanel` 读取 Desktop 模式/full/revision/can_mutate，CAS 修改后只展示服务器确认值；缺失 grant/不支持新版时保守禁用。开关、待办与规则均绑定原账号/Desktop/对话。
- 聊天持久卡支持 once/always/deny、选项及自由回答，显示目标 Session/Run/cwd/完整有界参数/永久规则范围。不可永久批准时 disabled 并解释。失败保留卡/回答，resolve/revoke 请求 ID 持久幂等。
- 长审批使用 `approval_details(pending_id,cursor)`，分段核对 pending_id/fingerprint/text/has_more/truncated，最大 1 MiB。全文取齐并显示后才能 once/always，提交 `details_ack:true` 和匹配 fingerprint；失败、错指纹或不完整分页时仍可 deny。详情不从草稿恢复。
- 历史刷新保持回答字段连接到 View，保护输入焦点/文字选择；权限弹窗关闭回调按实例核对，避免旧弹窗清空新引用。
- 全授权与规则只存 Desktop；本地仅缓存待办、回答草稿和幂等请求 ID。Global 子 Run 审批经来源 Global scope 响应。

验证安排/当前证据：

- Android 构建/AndroidTest 构建/lint 已通过，使用原 SDK、现有 JNI/bindings。生成物属于本地依赖，不进入源码提交。
- 14 个本地原生 UI 用例覆盖三种决定、永久不可用、撤销、问答丢响应/重开重试、CAS/full 等待确认、scope 失效、分页损坏、只读 grant/缺 grant/旧 Desktop、失效 pending、历史焦点、小屏大字体/IME、长详情失败及错指纹。
- 原有 AgentFocus/GlobalAssistant/MobilePrototype/ToolDetails/WorkspaceReviewRegression 共 13 用例同步新版 fixture，继续检验草稿/图片/历史/设置。此前发现旧设置按钮/返回文案与实际原生行已不符，已更新断言；限定高度需等待真实布局完成，不能以旧 viewport 尺寸判定。
- 当前完整 27 用例重跑中；之前长详情显示缓存碰撞已修正，不将失败轮次记作通过。
- 真正加密 RPC 测试：`AgentAuthorizationRpcUiTest`，读取 `files/agent-ui-fixture.json`，输出 `files/authorization-ui-results.json` 的 passed/expected_markers。覆盖实际 UI once/always/deny/问答/full/长详情/cwd/撤销/取消/只读模式。永久案例用 `/bin/echo always >> auth-review-always.log`，不用解释器。runner 由 review 执行者拥有；待 root 合入 runtime 后执行并独立核验临时 PTY marker，不以本地桩替代端到端结果。
- 模拟器是原 emulator-5586；重启后经 android-emulator-control 恢复，status 核对 boot_completed。测试包 `com.yxf.aterminal.authorizationfixture`，构建 `-PauthorizationUiFixture=true`，正常账号/应用/终端数据隔离。不调用付费模型、不测试物理设备。

日志保存在 `apps/android/authorization-build-final.log` / `authorization-regression-final.log`（忽略生成物）。最终可复核摘要、提交号和真实 RPC 限制由本说明更新及 worktree checkpoint 报告。

## 2026-10-03 后端最终 Review 接管

当前 run：c0ea835a-41c8-447a-a34c-fd46b0befc01。后端最终 Runtime/Review 主线已完成，由 Android 接管 R20/R21。原先 `/bin/echo` PTY 永久场景已过时：最终 PTY only once/full；永久真实场景改为 `run_program {program:"/usr/bin/tee",args:["-a","auth-review-always.log"],stdin:"always\n"}`，精确三行对应首次、规则自动放行、撤销后再授；第二次撤销后同参数 deny，不产生第四行。

源码修复与必要新增验收（尚未运行）：

- R20：响应 ID/撤销结果确认后清本次 operation nonce；丢 ACK 继续同 nonce。local UI 先丢 ACK 重试，再同 ID 再授/撤销新 nonce；真实 UI/RPC 两次撤销均 rules 为空、新 nonce，后续同 Native 参数再次审批且无多余 marker。
- R21：移除 `MainActivity.writeReason` 对 closed/exited/detached 的管理/任务门控。连接/账号/Desktop/scope 与真实 `can_mutate` 继续限制；PT​​Y writes 最终由 Host 检查。local Session UI 终端 unavailable 下 read_only 任务、回答、设置、停止仍可达；真实 fixture关闭后 query/ask_user/设置，full 下 PTY write 仍由服务器拒绝且无 marker。
- 本地永久审批桩改 Native 普通 program/args/stdin 形态。长详情、CAS、失效scope、旧Desktop、只读设备和输入恢复仍保留。
- runner 复用主线 Review 已确认 envelope，增加显式 `--cli` / `--example`，保存运行二进制 SHA256、核验独立 APK package。无需重新 Cargo；复用 Review 最终二进制。600秒验收窗口只启动隔离fixture，不使用用户服务。
- MobilePrototype 可选截图辅助函数重试并记录缺图，行为断言继续必需。此前14授权/其他原回归25项过，2项因可选截图null而中断，最新最终行为结果仍待。

当前大型构建窗口归 iOS，Android 静态准备/提交，不运行 Gradle/模拟器。协调者释放后按 jobs=2 / parallel=false 构建，并在独立包/原模拟器执行必要本地及真实 RPC 验收，不能把旧桩结果宣称 Native 永久已实测。

## Android 最终窗口阶段证据

`--max-workers=2 --no-parallel -PauthorizationUiFixture=true` assembleDebug / assembleDebugAndroidTest / lintDebug 全过。原模拟器5586恢复，aapt确认独立package才安装。

本地29聚焦首轮28过（16授权全部通过）；唯一旧页paging请求在loading期间被丢，已修为加载完成后继续旧页请求。新增真实路由会修改command字段的R20回归后再跑R20+完整图片/分页/设置用例2/2通过（14.581秒）。成功nonce key在RPC加version/agent_id之前捕获，避免清错key。其他27项首轮已过；后续不无差别扩大测试。

真实RPC首轮79.774秒未过，完整日志/响应/模型observations保存在`build/authorization-rpc-first`：once重放、长详情fp/ack、Native永久跨Run、deny、问答、PTYfull跨Run、Globalfull关闭/Session隔离已实链通过。cwd切入/相同Native不同cwd审批完成，cwd-back时模型fixture未识别Application compression stage、错误返回tool；Host正确报compression_tools_forbidden。已交root最小fixture修复，Android不改Rust。R20第二撤销与Closed链未到达，不能宣称已验收。runner补finally独立保存实际marker，即便失败也保留；首轮旧runner未保留临时marker文件，日志/响应/模型证据仍完整。

第二轮真实RPC79.891秒仍未完成，独立实际marker已保存在`build/authorization-rpc-final/authorization-actual-markers.json`，二进制SHA256与root指定一致。新增软件IME可见、输入/选择跨轮询、按钮可达已真实通过。cwd-back进入completed/pending空：fixture未识别压缩后的官方`Current task constraints`数组，走generic答复；已给root准确源码证据，root最小fixture解析修复中。不改Host/权限、不开新的Rust执行者、不放宽pending/行数判据。缩减的无密失败摘要见`RPC_FAILURE_2.json`，完整日志/响应/模型/截图在生成物目录。

当前源码所有修复已提交86a906d；实际IME测试/runner收尾提交f49d052。当前root短example编译窗口，不运行Gradle或Android UI。待新examplehash后第三轮同CLI、同Required完整真实RPC，只有全判据通过后报告Completed。

第三/第四轮已真实达到 R20 Native 三行、两次 UI revoke rules 为空且 UUID 不同、后续相同参数再次 pending/deny、cwd变化与规则不同指纹、取消、Global模式隔离。末端Closed失败由fixture生命周期导致：account_demo循环固定Poll已关闭唯一Session，返回session_not_found后example退出，账号/模型服务一起掉线；root已准确定位并接管最小授权fixture改动。不是Android权限拒绝，不能靠backoff或忽略异常当通过。完整第三/第四生成物目录独立保留。

root截图审阅发现早期IME断言只用GlobalVisibleRect不足以排除键盘遮挡；fixture setContentView裸column丢生产inset owner，未证明实际可达。已最小修测试：FrameLayout使用生产同OnApplyWindowInsets padding，input与submit完整screen rect均需bottom<=实际IME top，等requestRectangleOnScreen布局；question截图必须当次成功获取，内容和按钮在键盘之上。此严格补丁尚待新fixture后完整实际验收，旧轮次不作为该布局通过证据。Closed正常同账号/Desktop reconnect保留，不选择/重建已关闭PTY；原终端watch连接持续存活不宣称验证。

## 当前有效最终结果

Android 执行者范围 Completed。最小产品布局修复使历史重建后的滚动定位在已测量布局的 PreDraw 执行；IME/viewport变化优先保持当前回答+提交控件一同可见，触摸滚动revision/hold保护用户主动位置，已有文字/选择保持。最新10个相关原生焦点、缩小viewport、图片/分页/草稿回归55.754秒全部通过。

最终第七轮完整真实加密RPC119.110秒 `OK (1 test)`，0 ignored，runner独立核全部marker及精确Host拒绝均PASS。fresh问答图经实际视觉检查：选中的回答文字和“提交回答”按钮同时处于键盘上方；两个连续1.6秒真实poll期间不由测试重复滚动，输入、选择、input/button/scroll/IME bounds保持有效。输入bounds `[77,986][1003,1123]`、按钮 `[77,1139][1003,1271]`、Scroll `[0,379][1080,1271]`、IME top1507；图与实际可见控件相符。

R20：实际Native tee精确3行（首次always、规则自动执行、撤销后regrant），两次UI revoke原ruleID相同但request UUID不同、rules为空，最后同参数再次pending并deny，没有第四行。R21：临时Session真正关闭后同账号/Desktop正常重连，未select/重建已关PTY；read_only能力查询、authenticated_user问答、模式设置与full修改均真实可达。Closed写负向判据分别为原调用精确 `observe_terminal_after_uncertain_action` 与缺失终端观察精确 `session_belongs_to_another_account`，Host终态/noactivepending、UI显示精确fixture拒绝，独立closed marker不存在；这是安全拒绝，不叫写成功，也不强等成功DONE。

一次与幂等重放、长期规则/cwd变化、deny、两种问答、完整详情fingerprint/details_ack、Session/Global full启停及隔离、waiting取消、Closed模式与最终full关闭均通过。CLI SHA256 `bc255902114bb0c2c4c25aab73f68a2075762a43774689833897dc76207fdd72`，fixture `05bcb1bd04ed7e984a3774ce4115a8355fa55b91cb22ed0c0a563ca435e3a5a3`，保持生产CLI固定，只fixture修阶段处理/closed生命周期。

生成物完整保留 `build/authorization-rpc-seventh`（results、独立marker、精确Host拒绝、model observations、binary hashes、screenshots）。前六轮各自失败目录仍保留；第一轮旧runner没有临时marker副本的限制如前所述。无密最终可合入摘要 `RESULTS.json`。

软件IME设置恢复原0，fixture账号/Desktop/model服务和临时daemon结束退出0；正常用户package/账号/终端不修改，模拟器留给协调者。已在写最终说明前立即通知root释放系统窗口给iOS。物理设备、付费模型和Android真实detached transport不宣称验证；detached为本地UI+后端core证据，关闭后原watch连接持续存活不宣称验证。主线聚合/iOS与最后移动端diff Review仍由root负责，单端Completed不等于总体完成。
