# ModelScope 真实供应商自动化验收

- Date: 2026-09-26
- Status: Passed
- Endpoint: `https://api-inference.modelscope.cn/v1`
- Protocol: OpenAI Chat Completions
- Model: `Qwen/Qwen3.8-27B`
- Reasoning: `provider_default`；未声明/测试 level、budget 或禁用思考参数。

用户明确提供凭据并授权配置和真实自动化测试。Provider 已存入当前 Desktop 配置服务，模型别名为 `modelscope-qwen`，全局和 Session 默认均绑定此配置（配置修订 4）。工具与流式能力来自本次实测；视觉能力和供应商最大上下文仍保持未知。应用设置采用保守的 65,536 上下文预算、4,096 最大输出 token。

默认 Desktop 在检查到 **0 个终端会话、0 个活动 Agent** 后重新载入修复代码，并回读确认 `available=true`。没有终止用户 Shell。真实终端写入仅发生在测试创建的临时 HOME/PTY 内。

## 已通过的真实链路

测试 `modelscope_live_terminal_agent` 创建随机 SOURCE 标记，然后要求模型读取、分析、执行一次合成标记写入、再次读取确认并返回最初 SOURCE 标记。模型从未在用户指令中获得 SOURCE 的值，必须实际读取终端。

- 模型目录认证成功，目录包含指定模型 ID；应用自身 Rust 目录适配器验证通过。
- 真实 SSE 流式工具调用通过，透明测试转发器逐块传递数据，没有替换或模拟供应商回答。
- Terminal MCP → 不可变读屏 → 原文 UUID → 同配置尾部分析 → PTY 输入 → 再读/再分析 → 最终回复完成。
- 文件内容精确为一行预期 RESULT 标记，证明终端写入没有缺失或重复。
- 首次读取的 SOURCE 标记出现在原文回读和最终回复中。
- 同一 request ID 重试返回 duplicate，HTTP 请求数和持久模型调用数均保持不变。
- 关闭 Terminal Session 后，已存档 UUID 原文仍可回读。
- 实际供应商请求体断言旧消息前缀及 model、temperature、max_tokens、tools、tool_choice 保持一致。

成功运行用时约 **71 秒**，共 **7 次真实 HTTP/模型调用**。其中一次分析响应提出了 `read_record` 工具调用；Host 正确拒绝该调用并返回 `analysis_stage_tools_forbidden`，模型随后返回了合格 JSON，分析期间没有执行该工具。完整链路仍只执行了一次终端写入。

供应商返回的成功运行 usage（不含此前定位失败的请求）：

| 指标 | 数值 |
| --- | ---: |
| input_tokens | 32,801 |
| output_tokens | 3,958 |
| total_tokens | 36,759 |
| cached_input_tokens | 0 |

这证明实际请求/usage 合同可用，不证明缓存命中收益。SSE 有 `reasoning_content`，但本次 usage 没有提供可用的独立思考 token 分解；不能将标准化的零值解释为模型没有思考。

## 发现并修复的问题

初始真实运行在读取后停于 `observation_analysis_pending`。捕获的实际响应显示：模型将 `observed_status`、`blank_runs` 等 JSON 元数据写入 `facts[].evidence.text`，而证据校验只接受终端 `body` 的连续原句。重复请求的通用“分析无效”反馈没有指出这个区别。

修复没有放宽证据校验：

1. 分析尾部指令明确要求引文/事实证据仅取自 `body`，不引用元数据、JSON 包装或拼接省略号；状态和锚点由 Host 附入。
2. 重试反馈返回具体校验错误，要求删除无法提供原文证据的事实。
3. 新增确定性回归，验证元数据伪装成原文证据仍被拒绝，而精确正文引文可通过并保留 Host 状态。

## 复跑

真实测试默认 `#[ignore]`，普通 `cargo test` 不会调用 ModelScope。用户提供的测试凭据保存在本机被 Git 忽略的 `.local/modelscope-api.key`；当前 Desktop 凭据另外由 ConfigService 的既有凭据后端管理。源码/本文不嵌入 API Key。

在仓库根执行：

```sh
CARGO_INCREMENTAL=0 \
MODELSCOPE_API_KEY_FILE="$PWD/.local/modelscope-api.key" \
MODELSCOPE_LIVE_REPORT="$PWD/.local/modelscope-live-report.json" \
cargo +stable test --locked --offline -p ai-terminal --test agent \
  modelscope_live_terminal_agent -- --ignored --nocapture
```

`--offline` 仅限制 Cargo 下载依赖；显式运行的此测试仍会访问 ModelScope。可通过 `MODELSCOPE_BASE_URL` 和 `MODELSCOPE_MODEL` 覆盖测试目标。报告包含合成历史、原文、usage 和实际请求/SSE，不记录 Authorization header；失败报告也会保存。

最终结果：常规工作区 **105 项通过、1 项 live 测试默认忽略**；上述 live 测试显式运行 **1 项通过**；全工作区 clippy（`-D warnings`）、fmt、差异检查通过。此次生产代码变更限于 Desktop 分析指令/反馈，未修改移动端 ABI。

边界：本次没有执行真机 UI、真实视觉输入、其他供应商、Linux/Windows 运行或真实模型全局多 Session 委托验收。后者的并发/停止与来源门控仍由现有确定性自动化覆盖。

## 首尾/TUI 策略追加复验

用户随后要求默认首 10 / 尾 20、行数可配置以及搜索锚点排除动态 TUI。更新后真实 ModelScope 测试再次通过：**6 次模型请求，约 59 秒，一次实际写入**。两份终端档案均携带 `head_lines=10`、`tail_lines=20`，模型标注了交互提示符 `sh-3.2$`；原样证据保留它，`search_head_anchor/search_tail_anchor` 均已剔除。稳定 SOURCE/RESULT 日志和已执行命令回显保留，重试未新增 HTTP 请求。

部分正文分页的分析不会将未观察到的页面标成已分类；缺少完整分类时搜索首尾明确不可用。此项另外由确定性存储回归覆盖。默认 Desktop 无会话/活动任务时已载入新实现并持久写入 10/20（revision 5），ModelScope 全局/Session 模型绑定保持。
本次复验 usage：input_tokens=27962, output_tokens=3011, total_tokens=30973, cached_input_tokens=0。数值仅指本次成功运行。
