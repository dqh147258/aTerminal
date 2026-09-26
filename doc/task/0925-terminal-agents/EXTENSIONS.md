# 命令、配置、MCP、Skills 与模型管理合同

- Status: Approved
- Updated: 2026-09-25
- Parent: [PLAN.md](PLAN.md)

本文件是已执行计划的一部分，记录配置与扩展合同。操作说明见 [Agent 使用说明](../../../deploy/ASSISTANT.md)，实际平台验证范围见 [HANDOFF.md](HANDOFF.md)。

参考用户提供的本地 Codex 源码，具体 commit 与符号见 [CODEX-REFERENCE.md](CODEX-REFERENCE.md)；采用其能力绑定、目录隔离和交互式选择结构，不导入完整产品依赖或特定模型自动切换行为。

## 统一命令格式

所有新管理命令使用 `aTerminal <resource> <action> [ID] [options]`。集合资源用复数：`sessions`、`agents`、`providers`、`models`、`skills`、`devices`；`auth`、`mcp`、`history`、`config`、`daemon` 是固定命名空间。通用动词统一为 `list/show/add/update/remove/enable/disable/validate`，专用行为使用 `attach/capture/clean/set-default` 等明确动词。`history retention set/show/off` 作为策略子资源保留，不额外引入同义命令。

- ID 用位置参数，选项用 kebab-case；根 `--state-dir` 为全局选项，默认指向 `~/.aTerminal`，不再增加含义相同的 `--data-dir`。支持各层 `--help`；统一 `--json` 结构化成功/错误输出，stdout 不混入日志，stderr 输出诊断，退出码分类一致。
- 用户操作的 CLI、App RPC、内置 MCP、Skill 后端共用服务合同与校验，不通过 Shell 拼接再调用另一套 CLI 来复用业务逻辑。
- `providers add [ID]`、`models add [ID]` 在交互终端默认启动分步向导，缺省 ID 在向导内输入/建议；`update ID` 也可交互修改。保留 flags/JSON 配置方式供脚本使用，`--non-interactive` 或 `--json` 不隐式询问，非 TTY 缺少字段明确报错而不是挂起等待输入。
- 保留原 `auth login/status/logout`、`devices list/revoke`、无参数交互启动、`--list/--attach/--close/--history/--agent-stop` 的行为。新 `sessions list/attach/close/history` 和 `daemon stop` 成为推荐写法，旧顶层 flags 只做同合同兼容别名；互斥混用报错。旧设备撤销属于授权撤销，继续用 revoke，不与普通配置 remove 混淆。文档、帮助和脚本展示同一套规范命令。

示例：

```sh
aTerminal sessions list --json
aTerminal sessions show SESSION_ID
aTerminal sessions capture SESSION_ID --output terminal.png
aTerminal providers add
aTerminal providers update main
aTerminal providers add main --type openai --api-key-stdin --non-interactive
aTerminal providers update main --base-url https://api.openai.com/v1
aTerminal providers show main --json
aTerminal providers validate main
aTerminal providers remove main
aTerminal models add
aTerminal models discover --provider main
aTerminal models add main-agent --provider main --model MODEL_ID --context-window 128000 --non-interactive
aTerminal models update main-agent
aTerminal models update main-agent --reasoning-level high --non-interactive
aTerminal models update main-agent --temperature 0.2
aTerminal models set-default main-agent --scope global
aTerminal models set-default main-agent --scope session-default
aTerminal models set-default main-agent --session SESSION_ID
aTerminal models list --provider main
aTerminal mcp add --file server.json
aTerminal mcp update user-docs --file server.json
aTerminal mcp validate user-docs
aTerminal mcp list --json
aTerminal skills add /path/to/my-skill
aTerminal skills show user/my-skill
aTerminal skills disable user/my-skill
aTerminal skills remove user/my-skill
aTerminal history clean --older-than 30d
aTerminal history clean --before 2026-09-01
aTerminal history clean --session SESSION_ID --keep-last 10000
aTerminal history retention set --older-than 30d
aTerminal config show --json
aTerminal config validate
aTerminal config migrate --from /path/to/old-state
```

`providers/models validate` 只做静态配置/能力合同校验，保存配置不发起模型请求。模型列表刷新可以请求供应商目录（不是生成推理），由用户显式操作；连通性与实际工具循环通过用户发送一条测试消息验证，不在后台自动产生 AI 请求。密钥从隐藏输入或 stdin 接收，不放进 argv/示例/日志。

## 统一存储根与配置服务

Desktop 所有应用自行管理的文件默认位于操作系统用户主目录下大小写精确的 `~/.aTerminal`，Windows 对应用户 profile 下的 `.aTerminal`。无法解析 HOME/profile 时明确报错，不回退临时目录。`--state-dir` 仅提供显式替代根，用于隔离实例/测试；相关配置、历史、凭据引用和日志均跟随该根。

```text
~/.aTerminal/
  config.json              # schema_version、installation_id、策略、选择项、配置修订号
  providers.json           # Provider 实例；凭据只写引用
  models.json              # 模型别名、能力、参数、默认绑定
  mcp.json                 # 用户 MCP server 配置
  skills/                  # 用户安装的 Codex 格式 Skill 包
  builtin/                 # 与程序版本绑定的内置 Skill 可读镜像/清单
  credentials/             # 凭据引用；必要的私有文件凭据后端
  data/agent.sqlite3       # 历史、UUID 原文、PNG、事件、动作账本
  runtime/                 # PID、锁、本地 IPC 元信息
  logs/                    # 有大小/数量上限且脱敏的日志
  backups/                 # 有数量上限的迁移/配置备份
```

文件存储仍按账号/设备 scope 隔离。配置文件的条目带 owner/scope，远程只允许访问当前账号授权的条目；目录属于同一个 OS 用户不代表不同账号可以跨范围读取。`skills/` 下的用户包与 repo 引用有显式来源/激活 scope，不自动对其他账号启用。

凭据延续现有 OS keyring 后端，应用目录保存引用；系统凭据库是 OS 管理的存储，不属于应用散落的配置文件。已有文件凭据后端统一移到 `credentials/` 并保持私有权限，不为统一目录把原本 keyring 的明文凭据降级落盘。备份/导出默认不包含秘密；provider show/App 只返回 configured 状态和掩码，密钥写入后不回显。目录/敏感文件在 Unix 用 0700/0600，Windows 使用当前用户 ACL；不把权限控制宣称为磁盘加密。

ConfigService 是唯一写入口：版本化 JSON schema、完整候选校验、单写入序列、临时文件 fsync/原子替换、旧版本备份和 expected_revision 冲突检查。跨文件变更用事务清单/恢复日志，在全部配置通过校验后一次发布内存配置快照；部分落盘不能成为混合有效配置。手工编辑磁盘文件需 `config validate/reload` 或受控变更检测完成同一验证流程；错误配置保留旧有效快照并报告，不覆盖回用户原文件。

每个用户 Run 固定 `config_revision + model_profile_revision + tool_catalog_revision + skill_versions`。CLI/App 修改对下一 Run 生效，不在本轮分析时换温度、模型或工具前缀；删除/停用正在使用的 Provider、MCP 或 Skill 先阻止新使用，已有 Run 的处置显式显示（普通更新等当前 Run 结束，凭据撤销立即停止相应调用）。禁止无声换供应商/模型或把原文送到另一端点。

### 旧目录迁移

现有默认路径是临时目录 `ai-terminal-<uid>`（Unix）或 `ai-terminal`（Windows），登录 vault 身份还依赖旧 state_dir 的 hash。迁移需携带旧 vault 引用，生成稳定 installation_id，不能仅改默认函数然后丢失登录态。

启动先发现旧实例：旧 Desktop 正运行时继续连接它或给出明确迁移提示，不偷偷创建第二个默认实例，也不为了迁移杀死 Shell。非运行旧目录可在校验 owner/权限后备份、复制持久数据、验证、提交迁移标记；不复制 PID/锁/socket，不复制活 PTY 状态，不自动删除源。新旧根都有数据时不猜测合并；`config migrate --from` 返回冲突清单，保留两边数据。新的默认根不读取工作目录中的同名配置，也不在每次启动重新导入环境变量。

旧 AI 环境配置提供显式一次性导入路径；服务启动后以 ConfigService 为准。旧 `AI_TERMINAL_AI_*` 存在时显示迁移提示，不暗中覆盖 App/CLI 保存的 Provider/模型。

## 内置 MCP 与 Skills

选用官方 Rust MCP SDK `rmcp`，在依赖验证阶段锁定与项目/Rig兼容的正式版本；不从 SDK main 分支安装。内置 `builtin/terminal` MCP 服务使用同进程隔离会话/内存传输，执行真正的 MCP initialize/tools 合同，默认不开放网络端口。所有工具再进入已有 Terminal Broker；身份和能力由 Host 注入，不信任模型参数中的 account/session/control token。

| 层 | 首版内容 | 加载方式 |
| --- | --- | --- |
| 常用内置 MCP | Session 列表/状态、底部向上的内容锚点读屏、UUID/首尾锚点回读、字符串输入、按键；全局角色附带 Agent 状态/投递消息 | 小而稳定的 typed tool schema 常驻；Session 与全局角色各自有固定授权工具集 |
| 内置 Skills | 终端截图/视觉检查、创建/关闭/调整 Session、启动/停止 Session Agent、等待程序与完成证据解读、较复杂的历史定位 | 默认只注入名称、描述、来源；需要时读取完整 SKILL.md 与资源 |
| 固定扩展入口 | Skills 搜索/读取/受管脚本执行、用户 MCP 工具目录/调用 | 稳定 schema，按需取目标能力的参数 schema 并在 Host 校验 |

上表调整的是模型暴露方式，PLAN 的终端合同仍保留。读取参数以 [TERMINAL-READING.md](TERMINAL-READING.md) 为准：tail 读最后 N 行可不带锚点，search 中间向上搜索必须带非空 start_before，stop_before 可选；不用 live 终端行号，使用原始关键行内容；Skill 也采用“200 行 → 以前段 head 继续向上 1000 行 → 旧 tail 截止”的相同流程。Skill 是步骤/资源，不会凭文字产生执行能力：低频内置 Skill 通过受管入口调用注册的 Rust 服务动作，与 CLI/MCP 共用 Broker 和 UUID 观察处理。参数校验、人工抢占、动作去重、读取后分析不会因为经 Skill 调用而绕开。内置动作映射为发布时固定的结构化 handler，不把自然语言插到 Shell 命令执行。

内置包和 MCP 实现编译进程序，`builtin/` 只是按程序版本生成的镜像；源身份、内容 hash、权限与 handler 清单由二进制决定，镜像被修改不能变成新的可信实现。保留 `builtin` 命名空间；用户配置/导入/软链接/重名不能覆盖内置身份。CLI、App 和 RPC 均拒绝对内置条目的 update/remove/disable，普通 config reset/历史清理也不删除它们；升级仅能随应用版本整体更新。只读设备/会话权限仍可限制一次调用，这不等于修改或卸载内置能力。此处“不允许修改”是产品与运行时合同，不声称能阻止机器所有者修改程序二进制。

**缓存与触发约束：** 本轮固定内置工具排序、用户 MCP 目录快照和 Skills 元数据目录。按需加载的文档/工具 schema 作为消息尾部内容，优先通过固定桥接工具执行，不在读取后分析阶段增删顶层工具定义。MCP 通知、工具目录变化、Skills 文件变更只更新待发布版本，不调用 LLM。默认不接受 MCP server 发起的 sampling 请求；任意扩展都不能绕过“仅用户消息发起根 Run”的门控。

参考 Codex MCP binding 的职责拆分，配置 Registry、ConnectionManager 与 CatalogSnapshot 分离；本轮快照绑定实际连接/handler 与目录版本，而不是执行时按工具名解析到最新 server。启动超时与调用超时分别设定，实际 deadline 取 Run 剩余预算与工具/服务器限额的最小值。选中的必要 server 失败明确阻止依赖它的调用，未使用的用户 server 故障不阻塞内置 Terminal。Skills/MCP 元数据有单独上下文预算和稳定排序，超限条目经搜索/分页发现，不能无界注入或静默删掉内置入口。

## 用户 MCP：JSON 配置

`~/.aTerminal/mcp.json` 使用版本化顶层 `mcpServers`，支持 stdio 与 Streamable HTTP；不把 MCP JSON 配置冒充 MCP 协议本身定义的通用配置标准。CLI 导入也接受常见无 schema_version 的 `{"mcpServers": {...}}`，解析后转为本项目规范，未知有行为含义的字段明确报错。

```json
{
  "schema_version": 1,
  "mcpServers": {
    "user-docs": {
      "transport": "streamable_http",
      "url": "https://mcp.example.com/mcp",
      "enabled": true,
      "headerSecretRefs": { "Authorization": "mcp-docs-authorization" }
    },
    "user-local": {
      "transport": "stdio",
      "command": "/absolute/path/to/mcp-server",
      "args": [],
      "env": {},
      "enabled": true
    }
  }
}
```

stdio 使用独立 command/args/cwd，无 Shell 拼接；只传该 server 所需的环境变量，支持 envSecretRefs，不继承全部模型密钥。stdout 仅作为 MCP 消息通道，stderr 有界脱敏。HTTP 支持静态 header/token 凭据引用；需要额外 OAuth 流程的服务器首版明确报告该认证未接入，不声称兼容所有 MCP 登录方式。服务目录分页、初始化/调用超时、取消、输出大小、断线和协议版本错误均有明确状态；失败不自动安装程序、不无限重连、不自动重发可能已产生副作用的调用。

工具身份为 `(source, server_id, tool_name, catalog_revision)`，不会因同名覆盖内置工具。工具 annotations 是提示，不构成安全授权；未知副作用按可能写入处理。用户扩展只在被选定且用户启用后启动/连接，缺失命令或凭据返回真实错误。安装/配置和执行均发生在 Desktop，App 只管理文件和状态。

## 用户 Skills：兼容 Codex 的包方式

遵循已核查的 [Codex Build skills](https://learn.chatgpt.com/docs/build-skills)：目录包含 `SKILL.md`，YAML frontmatter 必须有 name/description，可包含 `scripts/`、`references/`、`assets/`、`agents/openai.yaml`。支持 `$skill-name` 显式调用、根据 description 隐式匹配、相对资源读取和渐进加载；遵守 `policy.allow_implicit_invocation=false`，保留 interface 元数据与 MCP 依赖声明。name 冲突时展示 source/路径并以完整 ID 选择，不合并、不静默覆盖。

`skills add` 默认将选定目录包复制安装到 `~/.aTerminal/skills/`，支持标准 Codex 包，无须修改原 SKILL.md。另可配置只读来源：`~/.agents/skills`、Session 已确认 cwd 到 repo root 的 `.agents/skills`；旧 `~/.codex/skills` 作为显式配置来源。保留 Codex 对包内相对路径、symlink skill 目录的使用方式，限制递归、循环链接和文件大小；导入时解析依赖并校验目标，不能借包路径覆盖 `builtin/` 或配置。不能可靠取得 Session 当前 cwd 时不拿初始 cwd 冒充当前项目来源。

用户 Skill 的新增/更新/启用/停用/删除由 CLI 管理；App 可查看目录、上传/编辑用户 SKILL.md 和关联资源包、启停/删除。上传是有界、分块、路径校验的安装事务，不允许 zip 路径穿越；不替用户执行 skill 安装脚本。用户普通编辑与内置只读视图明确分开，变更在下一用户 Run 生效。

`scripts/` 由 Desktop 受管脚本入口以显式解释器/argv、cwd 和受限环境执行，绑定 Run、记录动作 ID、限制时间/输出并支持取消；缺少 Python/Node 等依赖时明确报错，不自动安装。这是兼容脚本执行，不等于 OS 沙箱；任意用户脚本/MCP 程序本身具有 Desktop 用户进程权限，不能宣称 Broker 对它们提供机器级隔离。内置 Skill 不使用第三方脚本替代终端 Broker；用户脚本不继承 Host 管理凭据或模型密钥。

`agents/openai.yaml` 的依赖用于检查/提示，不自动下载 MCP 程序、绑定密钥或调用外部服务。首版承诺本地 Skill 包格式、资源解析、发现、显式/隐式调用与脚本方式的兼容，不承诺完整复制 Codex 的商店、插件分发、系统专有工具或其授权语义；引用不可用宿主工具时明确显示缺失依赖。

## Provider 与模型配置

区分三层：Provider 类型/协议 → 用户创建的 Provider 实例（endpoint、认证等）→ Model Profile（供应商 model ID、参数、能力及预算）。同一个 Provider 可配置多个端点/账号和多个模型。模型 ID 是用户输入/目录选择的字符串，不硬编码某个“最新”名称。

### 交互式添加与编辑

Provider 向导依次完成：选择协议（OpenAI Responses/Chat Completions、Anthropic Messages、Gemini、Azure OpenAI、Ollama 等）→ 选择可选供应商预设或自定义端点 → 输入名称与 endpoint/region/deployment 等该协议所需字段 → 隐藏输入密钥或选择现有凭据 → 检查配置摘要 → 提交。只展示当前协议需要的字段；预设是填写辅助，协议始终显式可见。支持上一步、取消、字段校验与修正，不向用户连续询问可从已选配置推导的信息。

模型向导依次完成：选择已配置 Provider → 由 Desktop 拉取并展示模型目录（可搜索/分页/刷新）→ 选定 model ID 或手工输入 → 配置模型名称、有效上下文/输出预算、思考强度及可选高级参数 → 选择是否作为全局/Session 默认 → 检查摘要 → 提交。默认值只使用 Provider 返回或已验证适配器知识；未知字段显示未知。Model Profile 与此次选择的默认绑定在同一事务提交。

每个向导在最后提交前仅保留草稿；Ctrl-C/取消不留下半条配置或孤立凭据。Provider 创建完成后可进入模型向导，此时两个步骤有明确各自的提交边界：取消模型添加不回滚已经成功建立的 Provider。编辑已保存条目时复用原凭据引用，不展示旧密钥；非交互 flags 使用相同 schema/校验，不另写配置逻辑。

### Provider 模型目录

实现独立 `ModelCatalog` 适配器，由 Desktop 按所选 Provider 协议和认证拉取实际模型信息，不要求所有服务都有相同的 `/models` 接口。`models list` 列本地 Model Profiles；`models discover --provider ID` 获取供应商目录，两者含义固定。App 的“从 Provider 获取模型”调用同一服务，手机不直接持有密钥访问供应商。

目录支持搜索、分页和有界缓存，缓存保存在 ~/.aTerminal 并标注获取时间、Provider 配置版本与是否过期。导入供应商提供的 ID/展示名/上下文/输出上限/能力等实际字段；目录没有提供的信息不能推断为支持。认证失败、网络错误、未开放模型列表、空目录分别显示真实原因；目录不可用时允许在向导里重试、使用标记为缓存的数据或手工输入 ID，不伪造在线模型列表。Azure 的 deployment 不能从基础模型名称自动猜出，缺少列举权限时允许手工填 deployment。

目录缓存身份参考 Codex models_identity：至少绑定账号 scope、Provider 协议、规范 endpoint/独立目录 URL、影响路由的参数以及凭据身份/版本；不能仅用显示名称或 model ID 缓存。凭据替换/账号改变时不能复用旧目录，仅保留不可逆身份摘要/版本而不落盘密钥。目录请求有响应字节和超时上限；不把授权 header 随跨源重定向转发给未知端点。CLI/App 均由 Desktop 返回 supported reasoning choices、默认值和描述；缓存数据缺少这些字段时保持未知，不按名称补造强度列表。

进入用户主动启动的模型添加步骤可以拉取目录；配置页面打开不自动发起生成请求，目录获取只作元数据请求。目录刷新不会自动修改已有模型参数/默认选择或影响在途 Run。过期缓存只作选型参考，不能覆盖用户设置。

首版支持矩阵：

| 协议适配 | 范围 |
| --- | --- |
| OpenAI | Responses 与 Chat Completions 显式选择，不在请求失败后猜测切协议 |
| Anthropic | 原生 Messages、流式、工具调用 |
| Google Gemini | 原生 generateContent/流式/工具调用 |
| Azure OpenAI | endpoint、deployment、API version 和 API key 配置，明确区分 deployment 与 model ID |
| OpenAI-compatible | 自定义 base URL；提供 DeepSeek、OpenRouter、Moonshot/Kimi、阿里云 DashScope/Qwen、智谱、MiniMax、火山方舟等配置预设；协议/工具差异逐项验证 |
| 本地服务 | Ollama 原生或明确选择其兼容端点，LM Studio 使用 OpenAI-compatible；不要求远程密钥，不附带安装/启动模型服务 |

通过 Rig 实现协议适配，项目配置不暴露其内部 Rust 类型。预设只是 endpoint/协议默认值，不自动保证每个模型支持 tools、vision 或 caching。AWS Bedrock/Vertex 等额外云身份链不在首版默认支持范围；不把可填自定义 URL 宣称为支持其签名认证。

Model Profile 保存 provider_id、model/deployment ID、context_window、输出上限、temperature/top_p（可空，模型不支持时显式拒绝而不是偷偷替换）、结构化 reasoning 配置、缓存设置、tool/vision/streaming 能力和费用/轮次限制。窗口未知时要求配置保守的有效预算并标明来源，不凭模型名称猜测。默认选择支持 global、session-default、单 Session override；读取后分析沿用当前 Run 完全相同的 Profile 和配置版本。不能工具调用的模型可用于显式只读聊天，不能静默降级后继续执行终端动作。

### 思考强度

CLI 向导和 App 设置都提供显式“思考强度”选项。配置采用 `reasoning.mode=provider_default|disabled|adaptive|level|budget` 及该模式所需值，模式互斥；只展示当前协议/模型经确认支持的项，不假定所有模型都有 low/medium/high，不把思考强度等价为 temperature。level 原样保存供应商支持的枚举；budget 模型提供 token 预算输入和有效范围；无法确认能力时允许保持供应商默认，不把推测映射为已支持。

适配器按具体模型合同映射到正确请求字段，并校验思考模式与 temperature/top_p/输出上限之间的限制。选择 level/budget 时由用户明确确认兼容参数，不静默忽略设置或切模型。CLI 非交互参数提供 `--reasoning-mode`、`--reasoning-level`、`--reasoning-budget` 的互斥/依赖校验；示例中的 high 仅对支持该枚举的模型有效。provider_default 表示不主动覆盖供应商设置，不能把它显示成已确认的某个强度。

支持修改 Model Profile 的默认强度，也支持全局 Agent/某 Session 绑定的强度覆盖；通过共用有效配置解析器计算最终值并展示来源。App 切换模型后重新加载该模型的可用强度，旧强度不兼容时要求重新选择，不能沿用无效值。模型与强度在一次保存中原子更新。任务已在运行时显示“下一轮生效”，本轮主线、读取分析、必要压缩共用原有有效模型/强度快照，保证请求配置一致。

配置时做类型/范围与已知能力校验；用户显式刷新 Provider 模型目录，无目录接口时允许手工添加模型。不同协议的系统消息、工具调用 ID、流式片段、错误与 usage 映射由适配器负责；切换模型时从项目规范历史重建合法上下文，provider 专属签名/推理块不跨供应商伪造，保留的可见事实/工具证据可继续使用。切换导致缓存重新建立属于明确行为，不后台自动 fallback。

## 移动端配置与权限

Android/iOS 增加“当前 Desktop 的 AI 设置”：Provider 分步配置、密钥设置、从 Provider 拉取并选择模型、模型参数编辑、独立可见的思考强度设置、全局/Session 默认与覆盖、MCP JSON 编辑/导入/启停/状态、用户 Skills 管理；内置条目展示只读标识。模型与思考强度都是实际可修改并可回读确认的表单项，不能仅显示配置 JSON 或把该功能留给 CLI。选中的设备名/账号始终可见，配置保存在 Desktop，不成为手机独立运行时。

App 与 CLI 共用 ConfigService 的 schema、校验错误和 expected_revision；通过已有加密通道新增单独配置 RPC。授权以当前账号 + 目标 Desktop + config_manage capability 检查，只读配对不能修改；不解禁原被禁止的远程 Account/Shutdown 操作，也不把配置 RPC 暴露给模型工具。MCP 命令/Skill 脚本安装可带来本地执行能力，必须是用户在配置界面提交的操作，不能由日志/模型输出自动创建配置。

密钥字段 write-only，经端到端加密传至 Desktop 凭据服务；手机不写聊天历史/普通偏好/日志，不回读 Desktop 明文密钥，Server 不接收明文。正常配置编辑不调用 LLM；测试模型通过用户显式发送测试消息发起。Desktop 离线时显示不可提交，允许保存无密钥的本地草稿，不静默排队覆盖日后配置。并发编辑冲突提示刷新/重试，不 last-write-wins。
