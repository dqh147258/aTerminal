# 协调者交接

实现与定向验证已完成，详见 RESULTS.md。协调者已确认代码及验证已 review，并授权本轮以 `[未Review]` subject 提交约定源码与必要文档；本执行者只提交，主 checkout 的 Git 合并由协调者处理。主 checkout 的服务、真实 test-1001 Session、AndroidTest harness 与模拟器仍由协调者统筹。

## 建议验收

1. 在真实 Desktop Terminal 进入不同目录，等待一轮采样（约 2 秒加 OS 查询），手机打开新建会话应显示最近目录；同目录重复使用应去重，另一个 idle 终端不会改变排序。
2. 选择含空格/特殊字符目录后创建，Desktop 新会话的真实 cwd 应一致；输入为空或点“默认目录”仍使用原 Desktop 默认 cwd，不自动采用第一条最近目录。
3. 先记录目录后删除它，再从最近项选择创建：应显示 Desktop 错误、保留路径、恢复按钮；不能偷偷创建默认目录。读取历史失败仍可手填或默认创建。
4. 检查小屏、横屏、大字体、键盘打开时列表可滚动、取消/创建可达；Android 对照 0930-app-settings-redesign 的信息行/蓝色主题。iOS 复用仓库现有 WorkspaceStyle，不全局重绘旧设置主题。
5. 切换账号/设备、关闭弹窗或断开连接后，迟到列表/创建回调不得填入另一连接；重开应读取当前 Desktop/账号记录。
6. 使用旧 Desktop 验证新 UI 的空最近列表与默认/手填创建；使用旧 mobile 验证新 Desktop 的 List/Create 行为不变。必要时用隔离 fixture 复核持久化跨 Desktop 重启，不必为此干扰当前真实服务。

## 边界

- 后台采样不捕获所有极短暂 cwd；OS 身份/cwd 不可用时不记录猜测值。Windows 当前只有成功启动 cwd，沿用现有 process::cwd 不支持的事实。
- 删除后的最近目录保留供创建时返回明确失败；本轮没有管理/清空历史 UI。
- Rust、Kotlin、Swift 验证通过不等于模拟器视觉或完整链接验收。需要重新生成 UniFFI 并配套构建 native 库，再安装测试。
- AndroidTest 的 `TerminalAgentWorkflowUiTest.kt` 未修改。

## 本轮追加：真实 UI harness 与弹窗高度修正

协调者已合并 `5e92dd7` 并完成主 checkout Android 3 ABI / APK / Lint。随后明确授权新增一个独立 `RecentDirectoriesUiTest.kt`，并依据主 checkout `.local/interactive-pelican/recent-empty.png` 修正固定 80% 窗口导致的大面积空白；本轮仅修改该新测试、MainActivity 中两个测量设置和本交接文档，以新 `[未Review]` commit 同步，不 merge。

最小 UI 改动保留现有 AlertDialog、Palette 圆角及标准创建/取消按钮：窗口改为 `WRAP_CONTENT`，内层 ScrollView `isFillViewport=false`。长内容由原生 AlertDialogLayout 的可用高度约束，并保留 `SOFT_INPUT_ADJUST_RESIZE`；本机 Android SDK 33 的 `AlertDialogLayout.tryOnMeasure` 可确认其先测 title/button 再给 customPanel 剩余高度的路径。设备上的长列表/IME效果由下述 harness 和协调者视觉验证，不把静态代码检查当作设备验收。

可选 fixture 放在 App 私有 `files/recent-directory-fixture.json`，仅允许三个字段，无凭据：

```json
{
  "valid_directory": "/absolute/canonical/disposable-valid-directory",
  "invalid_directory": "/absolute/canonical/disposable-deleted-directory",
  "timeout_seconds": 180
}
```

前置条件由协调者准备：当前账号已正常保存登录并连接目标 Desktop，正在显示一个现有、attached、未退出的终端；关闭工作空间/设置/弹窗，最多 14 个已有 Session，为测试留下 2 个名额。Desktop 已记录两个 canonical 非根目录路径，随后仅删除 invalid 目录；不在手机上 canonicalize Desktop 路径。期间不要并发新建/关闭会话、切换账号/设备、改变 shell cwd。使用可正常弹出的软键盘；测试会真实请求 IME 显示和隐藏并检查按钮边界，不修改键盘系统设置。

缺少 fixture 时 Assume skip；fixture 存在但状态/目录不符合时明确失败，不登录、不补造目录、不改账号、模型、凭据、宿主文件或服务。创建/取消全部通过正常工作空间的生产按钮和目录行；只读 RPC 核对 Session 列表、MRU 和 Desktop `context.cwd`。最后恢复原现有终端的 UI 选择，再用独立短连接关闭本次创建并由 UI 返回的明确新 Session ID；不会对任意新出现的列表差集批量关闭，也不触碰种子/既有会话。若创建结果不确定且没有 UI 返回 ID，保留未知 Session 并报告失败，不猜测删除。

执行（由协调者在目标设备运行）：

```sh
adb -s emulator-5586 shell am instrument -w -r -e class com.yxf.aterminal.RecentDirectoriesUiTest com.yxf.aterminal.test/androidx.test.runner.AndroidJUnitRunner
```

覆盖 invalid 行选择 → Desktop 目录校验错误且路径保留/按钮恢复/Session 数不变；同一弹窗改选 valid → 创建成功且 SessionInfo.cwd 与 OS `context.cwd` 等于 canonical fixture；第二次同 valid 创建 → 每次恰好一个新 Session，MRU 和 UI 均仅一条且前置；选择后取消不创建。还检查窗口 wrap-content、按钮距底部无巨大空白、屏幕及 IME 可见区域完整包含标准按钮，长内容可滚动到底。MRU 短列表和长列表取决于真实 Desktop 种子数量；空列表视觉另由协调者核对。

输出均在 App 私有 files 中：`recent-directory-results.json`，以及 `recent-directory-normal.png`、`recent-directory-ime.png`、`recent-directory-invalid-selected.png`、`recent-directory-rejected.png`、`recent-directory-selected-1.png` / `-2.png`、`recent-directory-success-1.png` / `-2.png`、`recent-directory-cancel-prepared.png`；失败时尽力补 `recent-directory-failed.png`。报告包含步骤判定、Desktop cwd、创建/清理 ID、保留会话 ID、错误、布局边界和截图文件列表，不包含凭据。

本工作树验证：`:app:compileDebugAndroidTestKotlin`（同时编译生产 Kotlin）通过；`git diff --check` 通过。未执行 instrumentation、未操作模拟器/服务/宿主目录，截图与运行 report 由协调者正式执行后产生。`TerminalAgentWorkflowUiTest.kt` 未修改。

## 本轮追加：IME 下标准按钮完整触摸区域被裁切

授权与证据：协调者提供主 checkout `evidence/normal-final/results.json` 和 `recent-directory-failed.png`，真实 harness 明确失败于 `ime: create button clipped by screen/IME`；普通状态可见高度 2138、dialog 高度 1571，标准创建按钮 bounds 为 `[871,1830][1047,1979]`。IME 截图中文案仍可见，但完整触摸 rect 超出 visible frame；账号与已有 Session 已确认保留。不能把标签可见当作按钮可达。

本轮仅改 `MainActivity.createSession()` 和本交接，按协调者本条授权直接作 `[未Review]` 提交用于 Git 测试同步；不改 `RecentDirectoriesUiTest.kt`（主 checkout 的连接等待、最近 content ScrollView 与 Account persist 修正由协调者保留）。

实现：仍用 WrapContent 窗口、现有 AlertDialog title / 标准按钮及 Palette 圆角；保存原 custom ScrollView 引用，在 decor global-layout 回调读取当前 visible display frame，按布局坐标扣除标题和页脚区域。页脚测量包含两个标准按钮的完整 bounds，若超出 decor 也纳入预留，再留 8dp 可见边界间距；只把 custom ScrollView 高度设为“自然内容高度”和“剩余可见高度”的较小值。长内容保持滚动，短内容按需收缩；IME 收起或内容/宽度变化时重新计算，可恢复完整内容高度。只有目标高度不同才更新 layoutParams；dismiss 移除原 ViewTreeObserver listener 并清空闭包，避免挂留及无变化的 requestLayout 循环。没有重设固定百分比窗口，也没有把按钮移入滚动内容。

自查/编译：本工作树 `:app:compileDebugKotlin` 与 `git diff --check` 通过。核对改动仅位于 createSession 的布局/解绑代码，创建 RPC、目录校验、身份保护及测试文件均不变。本轮未操作设备、模拟器、服务，也未宣称真实 UI 回归通过。

协调者重测：使用你已修正的真实 harness，确认 IME 打开时两个按钮的完整 rect 都包含在 visible frame 内；长列表滚动到底仍有固定可达按钮；键盘收起后内容恢复；空/少项无大块底部空白。重复开关 IME/弹窗，检查高度稳定、没有持续重排。报告与截图仍由主 checkout 保存。

## 本轮追加：隐藏 IME 后解除内容高度限制

协调者真实证据位于主 checkout `evidence/ime-fixed-verified/`：普通 visible/dialog 高度 2138/1571，IME 时 1371/1349，两个标准按钮 bounds 已通过；IME 隐藏后 `imeVisible=false`，但恢复 visible height 的等待超时，截图中第二最近项仍被截断。上一版计算把浮动 dialog 自身的 display frame 和“decor 剩余空隙”带入下一次 cap，存在收缩状态反馈。该证据不能只靠检查 IME visibility 宣称恢复成功。

本轮替换高度预算来源：API 30+ 从 **Activity** 的 `windowManager.currentWindowMetrics.bounds/windowInsets` 得到 task 可用高度，减 systemBars/displayCutout 与当前可见 IME 的联合 insets；IME 不可见时不再减 IME。API 25–29 使用完整 Activity 窗口的 visible frame，避免读取受限浮动 dialog 的 frame。通过 viewport 与标准按钮的共同父节点找到原生面板组，分别以固定宽度、UNSPECIFIED 高度测量固定标题/按钮兄弟面板；只加实际 padding/margins，不把已缩小 decor 的空隙当作页脚。自然内容能放下时将 viewport 的 `layoutParams.height` 明确恢复 `WRAP_CONTENT (-2)`；长内容仍按上限滚动。原窗口 WrapContent、Palette、标准按钮、仅变化时更新和 dismiss 解绑保持。

仅修改 `MainActivity.createSession()` 与本 Handoff，不触碰主 checkout 已独立修正的 UItest。`:app:compileDebugKotlin`、`git diff --check` 已通过；未操作模拟器/服务，本轮不宣称真机恢复通过。按本条明确授权提交 `[未Review]` 用于协调者 Git 同步，不 merge。

建议协调者在 normal / IME / hideKeyboard 已确认 false 后立即 / 恢复或超时前，补以下确切读取项（仅测试报告，本轮生产代码不增加日志）：

- Activity `windowManager.currentWindowMetrics.bounds`；其 `windowInsets.getInsets(systemBars | displayCutout)`、`getInsets(ime)`、`isVisible(ime)`。
- dialog decor `rootWindowInsets.isVisible(ime)` / `getInsets(ime)`，及 dialog decor、Activity decor 两者的 `getWindowVisibleDisplayFrame` 完整 Rect。
- dialog decor `height`；nearest content ScrollView 的 `layoutParams.height`、`height`、`measuredHeight`、`scrollY`，及其 `getChildAt(0).height/measuredHeight`。
- 原生 `topPanel` / `buttonPanel` / `customPanel` 的 `height`、`measuredHeight`、`paddingTop/paddingBottom`，可用 `resources.getIdentifier(name, "id", "android")` 读取；两个标准按钮完整 screen rect。

预期隐藏后 viewport 不再停留在 IME cap；相同宽度与内容下，本次两条目录应回到原自然 dialog 高度。长列表可以仍受真实 task 可用高度约束；可见 frame 若仍与 before 不同，上述 metrics 可区分窗口报告与内容尺寸，不应仅凭截图标签或 IME false 判断成功。
