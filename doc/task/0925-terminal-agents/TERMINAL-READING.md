# 终端从底部向上读取与内容锚点合同

- Status: Approved
- Updated: 2026-09-26
- Parent: [PLAN.md](PLAN.md)

本合同替代原计划对模型暴露 tail/since 游标及依赖终端行号的方案。终端是可重绘画面和有界回滚缓冲，不是不可变的追加日志；固定行号不能跨清屏、重绘、resize 或 Session 重启复用。

## 读取接口

`read_terminal` 默认从终端底部向上扫描，找到边界或达到限额后停止；返回的文字仍按屏幕从上到下排列，方便理解。MCP 工具与 CLI 使用相同参数语义：

| 参数 | 语义 |
| --- | --- |
| `mode` | `tail`（默认，从底部取最后 N 行）、`search`（从中间开始向上查找）或 `screen`（当前屏幕布局） |
| `max_lines` | 默认 200，可传 1000；是本次返回最多多少有效内容行，不是向上搜索起始点 |
| `start_before` | search 必填且必须有非空内容；从匹配块上方继续向更早内容读取。tail 不接受此字段，避免把两种模式混用 |
| `stop_before` | 截止内容锚点：向上遇到匹配块即停止；截止块及更早内容不返回 |
| `view_id` | 可选，不透明的读取视图 ID；继续翻旧内容时使用前次视图，避免并发刷新改变搜索源 |
| `max_bytes` | 有界返回大小，受 Host/模型有效预算限制；不能因为行数少就返回超大文本 |

**模式校验：** tail 允许完全不带锚点，也可单独带 stop_before 以免重复读旧内容；search 必须显式传 start_before，stop_before 可选。缺少开始锚点返回 `start_anchor_required`，空字符串/全空白锚点返回 `invalid_anchor`；仅有 view_id、stop_before 或隐式记住的上次位置都不能代替 search 的开始锚点。record_id + edge 必须能解析到已分类、有效非空的搜索原始行；权限/过期/全空白失败分别报告，不静默换模式。screen 不接受搜索边界。该合同同时在 MCP schema（可表达时）和 Host 执行校验中落实。

两个边界都可传 `{"lines":["稳定日志行1", "稳定日志行2"], "tui_lines":["需要剔除的精确 TUI 行"]}`，或 `{"record_id":"UUID-A", "edge":"head"}` / `edge=tail` 引用档案里保存的关键原始行。允许一行，但多行更容易消除重复匹配。模型不必自己记住或抄写终端行号；引用形式只是准确取出已保存的内容锚点，服务端可利用其来源定位信息帮助消歧。

默认匹配完整关键行，不做正则、模糊匹配、大小写折叠或日志时间戳删除。多行锚点必须按顺序出现，仅可跨过空白和已明确标注的 TUI 行，不能跨过其他非空日志拼成匹配。首尾边界均排除匹配块，且 start 必须在 stop 的较新一侧；边界相同返回空区间，颠倒返回 `invalid_boundary_order`，不能自动交换。

示例（UUID 为示意标签）：

```json
{"mode":"tail","max_lines":200}
```

返回 `record_id=UUID-A`、`view_id=VIEW-A`、原样展示 head/tail、去 TUI 的 search_head/search_tail 锚点及摘要后，信息不足时继续：

```json
{
  "mode":"search",
  "view_id":"VIEW-A",
  "max_lines":1000,
  "start_before":{"record_id":"UUID-A","edge":"head"},
  "stop_before":{"record_id":"UUID-OLD","edge":"tail"}
}
```

含义是从 A 已过滤 TUI 的搜索首部之前继续向上找，遇到 OLD 已过滤 TUI 的搜索尾部就停，最多取 1000 个有效内容行；并非先跳过 200 行再固定取 1000 行。没有合适的旧记录时可省略 stop。读取当前最新输出则显式用 tail，省略 view_id/start，可用上次记录的 tail 作为 stop。

结果包含 `record_id`、view_id、session/epoch/revision、body、head_anchor、tail_anchor、boundary_status、end_reason、has_more/older_available、非空/物理行计数、空白折叠描述及截断/缺口信息。`end_reason` 明确区分 `stop_anchor`、`line_limit`、`byte_limit`、`source_start`、`scan_limit`，不能把“没找到截止”写成“已读到上次位置”。source 本身被截断时，source_start 仅表示可用视图的起点，不能声称是全部终端历史开头。

## 固定读取视图与可变终端

一次新的读取在 Session actor 上捕获同一 revision 的有界不可变 ReadView：普通模式包含保留的 scrollback 和当前 live grid；alternate screen 只包含该 TUI 当前有效缓冲，不能自动拼上后台 Shell 历史。复用现有 Alacritty 权威状态，不根据 ANSI 字节另写解释器，也不靠连续定时截屏推断完整日志。

ReadView 内部可以有数组位置、字节范围和版本信息，用于精确切片、分页和引用校验，但这些只在该视图内有意义，不成为模型操作终端的行号合同。保留软换行信息，默认按捕获时的可见文本行定位，不把 resize 之后的重新折行与旧行强行对齐。隐藏单元不输出原字符；已解析的文本不含控制序列；仅把终端填充的行尾空白从匹配文本中规范化，保留缩进、标点和组合字符。

同一次向上扩展优先复用前次 view_id；源视图与原记录边界相同时，使用已验证的来源位置排除整块已读区间，包括其边缘空白，保证相邻读取不跳过内容。后台的新输出、清屏或重绘不会改写该视图；响应标明它是历史时刻画面，不称为当前屏幕。`get_terminal_state`/执行前 Broker 仍依据最新状态；旧视图只供读取，不能当成新鲜写授权。

ReadView 按内存/字节/数量上限保存，在 Run 内按需 pin；无用户请求不持续制作视图。超限或过期返回 `view_expired`，明确请求重新读当前源；不能静默把旧 view_id 映射到新屏幕。捕获和扫描有独立行数/字节/时间上限，搜索工作移到不可变数据上，不让磁盘/模型阻塞 PTY actor。达到上限返回可见的 partial/scan_limit，不能越过未扫描区域。

跨视图引用旧锚点时，只在请求选择的当前源内做内容搜索，并标记 source_changed；唯一的文字匹配也只是定位结果，不证明中间没清屏、没缺失输出或是同一个程序事件。重复匹配返回有界候选片段/候选 ID，要求更长上下文或显式选候选；不能默认挑最近一次而静默跳过新内容。用户手写的纯文本锚点没有来源身份，同样遵守歧义规则。

start 找不到时返回 `start_anchor_not_found`，不擅自从底部重读。stop 在选定读取窗口内找不到时，可返回有界内容，但 `boundary_status=not_found_in_window` 并说明停止原因；后续可用本次 head 继续查。stop 重复匹配且来源无法消歧时返回 `ambiguous` 候选，不把任意一个作为去重边界。匹配块横跨本页边缘时可做有界额外比对以确认完整锚点，但返回内容仍受 max_lines/max_bytes 限制。

清屏、scrollback 擦除、TUI 覆写可能使旧内容从当前终端彻底消失，文字锚点无法恢复它。若旧 ReadView 尚在可继续查旧画面；否则通过 `read_record(UUID)` 回读此前持久保存的观察。没有被保存且已被终端丢弃的内容明确不可恢复，不拼接其他时间画面冒充连续历史。不承诺保存每一次瞬间重绘或完整 PTY 录像。

## 默认首 10 / 尾 20 行、配置与空白处理

Host 在原文存档时确定性生成 head/tail，不让 LLM 重新挑选、改写或“凭记忆补齐”：

- 从本次返回区间顶部取默认最多 **10 条**、底部取默认最多 **20 条非空原始内容行**；分别由 `terminal_reading.head_lines/tail_lines` 配置，范围 1–100。不足配置数量时保留实际全部，head/tail 重叠时在展示中去重，但两边定位元数据仍保留。
- 空白行不会充当内容锚点。连续空白在模型文本中以结构化 `blank_run(count)` 压缩展示，行内空格和缩进不改变；标记是独立元数据，不混入原文参与匹配。记录准确的首尾/中间空白数量和原始切片信息，原文档案保留原始空白，可按 UUID 无损回读。
- 默认 max_lines 按非空内容行计数，另返 physical_lines、blank_lines；独立的扫描/字节上限防止跨无限空行寻找内容。需要分析 TUI 空间布局时用 screen/截图，不用折叠后的 tail/search 文本推断坐标。
- 匹配时在同一源范围忽略空白行数量差异，非空行的内容和相邻次序必须一致；保存 blank_runs 作为诊断，不因空白数变化自动判为同一事件。全空白/空记录返回 `anchor_unavailable=blank_only`，不把空字符串或“省略空行”当截止符；中间搜索必须换用有效的非空开始锚点，不能凭内部位置绕过 start_before；也可用 tail/screen 或 UUID 回读查看空白本身。
- 行本身过长时，按字节上限返回有明确片段标记的数据；被截断片段不能伪装成完整锚点行。原始完整行可在档案中有界回读；不足各自配置行数时标记 anchor_incomplete，不能无限越过本次读取范围取额外内容“补齐”。

读取后分析仍保持同一主线历史/配置，只在末尾说明分析最后一条记录。输入中已有 Host 生成的 head/tail；分析产出是摘要、关键原句和事实。最终 `ObservationDigest = record_id + summary + key_quotes + head_anchor + tail_anchor + boundary_status`，Host 把保存的锚点附入结果；LLM 不需要再次输出这些原文行（默认最多 30 行）。关键引文用 UUID + 原句/quote_id 校验，不要求跨时刻的终端行号。

每条分析记录的 head/tail 都持久保存。卸载原文时上下文保留这些锚点与摘要，正在向上读取的链及最近读取的首尾锚点不被普通原句清理步骤删除。更早记录的锚点可随 UUID 索引一起移出活跃上下文、按需回读，但不得由 LLM 有损改写；`read_record` 支持 `part=anchors|body|summary`，只需定位时可读取首尾与搜索锚点元数据，不展开全部原文。磁盘清理按主计划过期规则处理，原文删除后不能留下可假回读的锚点身份。

## 配置入口与 TUI 过滤

```sh
aTerminal config terminal-reading
aTerminal config terminal-reading --head-lines 10 --tail-lines 20
```

Android/iOS 的「Agent 设置 → 终端读取锚点」提供同样的数值编辑。设置按当前账号/Desktop 保存，当前 Run 固定其配置，新 Run 使用新值；不会重写旧档案的原样首尾。

**显示证据与搜索锚点分开。** `head_anchor/tail_anchor` 保留原始展示内容，包括 TUI；`search_head_anchor/search_tail_anchor` 则由 Host 从同一份已保存正文里排除空白及已识别 TUI 后按配置数量提取，保留原始文字、顺序和来源位置。`record_id + edge=head|tail` 在工具中始终选择后者，不直接使用原样展示的末尾几行。

日志底部可能是持续变化的 TUI，典型如状态栏、进度/旋转指示、交互提示符、输入框、快捷键提示和边框。分析阶段必须返回 `tui_lines: ["精确完整原句", ...]`（无 TUI 时为空数组），Host 校验每行确实存在于原文中；不接受改写、模糊匹配、凭空行或整段 JSON 元数据。原文不删除。普通日志不能仅因“处在底部”被自动认作 TUI；不能确定分类时保留原文并由调用者选择已观察到的稳定日志。

原样尾部默认 20 行可能全部都是 TUI，因此搜索锚点从本条记录的完整正文中选择可用的稳定日志，不局限于原样 tail 的 20 行窗口。混排 TUI 的语义分类由模型/明确调用参数提供，不宣称能够自动准确识别所有应用；alternate screen 作为动态 TUI 画面，不提供日志搜索锚点，使用 `screen`/截图/UUID 读取。

三种输入（显式 `lines`、`record_id + edge`、`candidate_id`）都接受可选的 `tui_lines`，并合并当前固定视图已保存的 TUI 分类。匹配前同时过滤锚点与搜索源中的已知 TUI；候选片段也不再把 TUI 带回锚点。仅忽略已明确标注的完整行，不用模糊/正则猜测，也不会因为过滤消除了差异就静默选择重复匹配中的某一个。

过滤后只有 TUI、空白或片段时明确报告无可用稳定锚点；不能绕过非空开始锚点要求，也不能悄悄改为 tail。没有 TUI 分类的旧记录返回 `unclassified`，需先分析完整的有界原文记录，或使用已观察页面中的明确稳定行。分页回读只分析一部分正文时，不将未观察到的其他页面标成已分类；没有之前完整分类的记录返回 `unclassified_partial_body`，不生成伪完整搜索首尾。

## 关键验证

- tail 不带任何锚点可读最后 N 行；search 缺失/空白开始锚点必须拒绝，只有 stop/view_id 也不成立；记录锚点过期/全空白不能静默回退。
- 先读底部 200，再用 head 继续向上最多 1000，在旧 tail 处提前停止；首尾排除、相邻记录不重不漏、返回上到下排列。
- 单行/多行锚点、重复测试输出/重复提示符、相同首尾、顺序颠倒、start 缺失、stop 在窗口外、匹配块跨分页边缘及旧 UUID 已过期。
- 纯空白、超过扫描预算的大量空白、头尾空白、内容不足配置行数、head/tail 重叠、超长行与字节截断、真实日志内容恰好类似 blank_run 的文字。
- 清屏与擦除 scrollback、全屏重绘、进度条覆盖、并发输出、resize/reflow、alternate screen 切换：固定视图不变，新视图锚点缺失/歧义明确，不伪造连续历史。
- 摘要失败/多代压缩后锚点原句保持可验证；最近读取链锚点保留；旧锚点可轻量回读；不存在用空锚点匹配任意位置的退化。

- 新增回归：10/20 默认与自定义计数、20 行锚点合法性、TUI 更新前后定位、正文无损、纯 TUI/alternate screen 拒绝、固定来源位置不能绕过过滤、过滤后重复匹配仍需消歧、分类原句验证、旧记录/部分正文分类状态与配置发布恢复。
