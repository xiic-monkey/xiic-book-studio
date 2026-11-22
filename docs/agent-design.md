# Agent 体系设计说明

## 1. 组成与路由

当前内置 16 个 Agent，按运行时获取方式分为两类：

### 阶段职责与共享协议（5 个）

`story_architect`、`draft`、`review`、`revision`、`orchestrator`

执行时 `get_execution_agent` 按职责选 Agent，再由 `apply_stage_protocol` 在其 system prompt 末尾追加共享事实与采用协议。设定、阶段大纲、角色共用 `story_architect`；章节写作、试读、修订各用对应的 Agent。同一职责不因项目题材改变身份、模型、工具或输出契约。设置页和详情返回未拼接的 Agent 配置。

题材方法从 Skill 注入到相应阶段的 user prompt，system prompt 不添加题材身份，也不声明实际注入了哪些规则。`orchestrator` 只用项目的题材标签理解请求；委托后，子 Agent 自行加载对应规则。创作基准一致性审校沿用架构 Agent 的协议，但不加载 Skill。

### 裸 Agent 路径（11 个）

`chapter_memory`、`continuity_ledger`、`continuity_check`、`story_index`、`adoption`、`canon_align`、`context_search_plan`、`context_search_rerank`、`artifact_revision`、`continuity_review`、`chapter_split_plan`

这些 Agent 直接通过 `get_agent` 获取，不自动继承 `SHARED_PROTOCOL`。只有 `continuity_review` 的跨章审校入口显式加载试读规则；其它后台任务不重新加载写法。

协议清单位于 `src-tauri/src/stage_agent.rs`，内置 Agent 总表位于 `src-tauri/src/prompt_templates.rs`；测试逐一验证对应的运行方式。

## 2. 契约分层边界规则

一条硬规则：**Agent 模板定义职责、行为准则与输出契约；运行时 user prompt 提供数据、任务和适用的 Skill 方法；解析器负责最终校验。**

- 输出契约（JSON 结构、字段清单、引文长度、severity 取值、输出条数上限）只写在 `prompts/v2/*.md` 里，Rust 侧不得再复制一份。例外：`story_architect` 的五个模式契约属于运行时按模式选择的任务框架，保留在 `story_architecture.rs`；`ai.rs` 的工具规划协议是横切所有工具 Agent 的第 17 份契约，当前无独立 Agent 载体，暂留 Rust。
- user prompt 中的数据块统一用 `DATA_BLOCK_BOUNDARY` 包裹，人工指令用"人工追加指令"标题注入，两者都不构成契约。
- `prompt_templates.rs` 中的契约锚点测试逐 Agent 锁定 .md 中的关键契约片段；改动契约时必须同步更新锚点。

### 解析与校验代码

Rust 解析器和数据库边界是最终事实来源。模型遵守的格式必须在代码侧再次解析、校验、归一化和检查项目归属。Prompt 不能替代安全校验。

`adoption` 是机器可读输出的样板：其 prompt 声明 `{"items":[...]}` 对象、候选字段和空结果形式，`parse_extracted_candidates`、`normalize_data`、证据引文校验和外键校验共同决定候选是否可进入待人工确认流程；解析器仍兼容旧数组和代码围栏，不能把兼容路径当作 prompt 契约。

## 3. prompt / Skill 的存储与同步

内置 prompt 与 Skill 的唯一事实来源是仓库文件：`src-tauri/prompts/v2/*.md` 与 `src-tauri/skills/**/*.md`。

- 数据库 `agents.system_prompt` 与 `writing_skills.content` 只是可编辑副本。启动时 `lifecycle::sync_builtin_prompts_and_skills` 会把未定制的行同步到最新模板。
- `agents.prompt_customized` / `writing_skills.customized` 标记用户是否改过内容（保存时与内置模板比对自动计算；"恢复默认 Prompt" 会清零标记）。标记为 1 的行永不被启动同步覆盖。
- 历史上的内容嗅探补丁（用旧句子探测再替换）已删除，由上述标记 + 同步机制替代；仅保留 `urban_mystery` 停用迁移这类一次性数据迁移。
- 前端设置界面展示的 `editable_system_prompt` 即数据库副本；同步后与模板一致。
- `writing_skills.enabled` 对题材 Skill 与 Craft Skill 语义一致：**禁用即不注入**。仅题材 Skill 缺数据库记录时使用同 key 的内置文件；缺 Craft 记录时不注入。`v2_storage` 的 `migrate_v12` 曾一次性把内置题材 Skill 恢复为启用，之后用户手动禁用不会再被启动流程还原。
- `agents.allowed_skill_keys` 存的是**许可**：保存时会丢弃题材类与库里不存在的 key，但不会因为 Skill 被临时禁用就删除既有许可。许可与当前运行支持的辅助方法分别判断。

## 4. 题材路由与 Skill

`detect_genre_skill` 使用题材字符串标记进行路由。当前支持通用连载、都市异能、悬疑和男频修仙/玄幻升级流；未命中窄题材的“科幻”“奇幻”“历史”等输入落到通用连载。组合题材按检测顺序处理，测试锁定悬疑优先于都市标记、升级标记优先于都市异能标记的现状。

Skill 分两层，按职责切分、互不复述：

- `skills/craft/continuity_and_agency.md`：跨题材通用规则（角色已知信息断点、物件状态连续、主角主动选择、探索章控密度等）。规则陈述在“状态类别”层面（持有、存放、完好度、知情、约束、时限、约定），示例只允许使用所有题材的最小交集词汇（门、锁、伤势、钱物、约定、期限、信息等）；信物、封印、阵法、丹药这类具象题材道具不得出现在通用层。
- `skills/genres/*.md`：题材专属写法（题材词汇、章节模式、severity 约定等），在这里可以且应该使用具象题材词汇，例如仙侠 skill 用“灵物、令牌、战力层级”把通用规则实例化。与 craft 语义重复的纯复述条目不得回填到题材 skill；只有携带题材特有语义的条目（如能力越界的战力层级 severity 映射）才放这里。

`load_stage_skills` 直接从 `Project.genre` 选择一个题材 Skill，并对启用的规则取 `## Always` 和对应 `## Stage: <stage>`。两段均无内容时不注入，不因自定义文本为空或段落缺失而回退。辅助 Skill 目前只支持 `continuity_and_agency`，要求分类为 `craft`、库中启用并被执行 Agent 许可。题材和辅助规则独立。守卫测试覆盖内置模板的全部六个阶段，不要求用户自定义规则包含所有阶段。

Skill 只提供方法，不能新增本书事实、工具或输出字段。章节任务卡保留本书目标和状态变化；悬疑的证据增量、都市异能的边界代价、仙侠的增长回报都由题材 Skill 规定，不在任务卡重复。关掉库开关不会剥除题材标签或书中已有事实，也不会删除用户写进自定义 Agent Prompt 的内容。

使用 `MAX_WORKFLOW_PROMPT_CHARS = 24_000` 的入口先移除低优先级的非 Skill 段，再压缩非 Skill 段，最后才裁剪 Skill。预算同时检查标题和来源行；正文中的 `Skill` 字样不改变优先级。只含阶段规则的区块若被裁去唯一规则则整段移除，裁剪诊断写入 stderr。

预览指纹使用生成该预览的同一份题材、Craft 规则快照；显式使用旧预览时重新校验生效规则与 Agent 配置。运行沿用已准备的上下文与通过校验的 Agent 配置，账本补充和最终预算仍可改变发送内容。跨章审校保持完整正文证据，缓存键 `review=v5` 包含章节输入和实际渲染的项目、已采用资料与审校规则；规则或资料变化后不复用不匹配的报告，旧报告仍可查看。

设置页在「技能库」显示当前项目匹配的 Skill 及其启用状态；开关影响所有匹配该 Skill 的项目。工作区和详情的 Tauri/Dev REST 返回同版改用 `genre_skill: { skill_key, name, enabled }`。此次拆分不变更 SQLite schema，旧 `project_genre_agents` 表及记录保留，新运行时不再读取或维护该绑定。前端和后端应同时升级或回退。

### 旧版回读

临时数据库演练使用 `8cf240e` 的旧 `dev_api` 和当前 `dev_api`。新版读取旧正文与历史运行，按当前题材选 Skill。旧版回读时给新版新建的书补齐绑定，但保留改过题材的书原有的有效旧 key。对这些书，关闭应用并备份后，在数据库副本中仅删除确认失配的 `project_genre_agents` 行，再启动旧版。已验证旧版能按当前题材重建该行，正文与历史运行仍可读取。

当前工作区已有 v12 数据迁移。上述旧版 `dev_api` 以关闭调试断言的方式构建。旧提交的调试构建会断言数据库版本至多为 v11，回退验证需选择匹配数据库版本的构建，或用升级前备份验证旧调试构建。

**题材 Skill 负载的复测结论（2026-11-21）**：F7 复测判为 `INCONCLUSIVE`，**不是「无退化」**。严格口径（引文 ≥8 字连续重合）B 组均值 0.250 < A 组 0.333 指向判负，人工语义口径 B 组 0.500 > A 组 0.333 指向判正，两套方向相反且差异量级 0.08–0.17 条，单次召回全部落在 0–1 条区间，结论对判定口径敏感。根因不是组间噪声大，而是 ground truth 覆盖面与模型可发现的问题集错配：GT 里 9 条有 7 条在当前上下文供给下基本不可达，而模型稳定发现了 GT 之外的真问题。同一轮暴露并修复了 F10（预算裁剪整段删除 Skill 正文，导致题材 Skill 在此之前从未真正注入）；NEG-001 的结论建立在同一套裁剪行为上，需要在修复后的系统上复核。实验协议、报告和原始样本仅保留在本地。

## 4.1 供应商 thinking 开关与工具预算（2026-11-19 补）

这两条不是题材 Skill 体系的一部分，但直接决定「试读」这一环能不能跑通，因此写在这里。

- **`thinking` 开关必须真正下发到供应商。** `ai::provider_thinking_config` 过去只识别 `deepseek` 与 `minimaxi`，其余供应商一律不下发 `thinking` 参数，等于沿用供应商默认值 —— 用户/Agent 的「关闭思考」对它们完全无效。MiMO 默认开启深度思考且忽略 `budget_tokens`，实测同一条 2 万字符的试读请求：不带 `thinking` 参数超过 300 秒仍不返回（客户端 240 秒窗口内报 `network error: error decoding response body`），带 `thinking: {"type":"disabled"}` 时 8.8 秒返回。现在 `xiaomimimo` 也走显式下发，未知供应商保持原有默认行为。
- **工具预算是收尾而不是熔断。** `tool_runtime` 里 `MAX_TOOL_CALLS = 64` 过去一旦超限就直接让整次运行失败（`Agent 工具调用超过单次运行上限 64`）。有些模型（试读场景下的 MiMO）会不断变换检索词继续搜，必然撞上限。现在轮次上限 `MAX_TOOL_ROUNDS = 4`，并且「轮次用尽」与「调用预算用尽」都改为结束循环，让模型带着已收集的工具结果产出最终答复；已经收集到的上下文仍然可用，继续搜索不会让结论更可靠。
- 这两条都不引入新的运行时开关面：thinking 沿用已有的 Agent/全局开关，工具预算沿用已有的常量。

## 5. Story Architect 工具例外

Story Architect 在 `setting`、`outline`、`characters` 阶段即使旧配置没有显式勾选知识卡工具，也会隐式获得创建、更新和提议知识卡工具，以避免回退到 Markdown 资料流程。Preview 只允许可预览的只读工具，因此这些写入工具在 preview 中不会出现。

资料卡的细分规则和已有卡片上下文由运行时追加的“结构化资料卡工作方式”提供；静态 `story_architect.md` 只保留身份和高层 Canon 原则。

## 6. 试读输出契约：提交型工具（2026-11-21）

试读（review）是唯一允许多轮 tool-calling 的阶段。它的结构化结果不再靠提示词硬压「只输出 JSON 数组」，而是靠工具：

- **工具**：`submit_review_issues`（`ToolKind::Submit`），只在 review 阶段隐式下发（与 Story Architect 知识卡工具同一先例），用户不必改 Agent 的 `enabled_tool_keys`。
- **契约位置**：字段清单、条数上限（8）、severity 取值仍写在 `prompts/v2/review.md`；Rust 侧只做解析与校验（`ai::validate_review_issues`），不复制一份契约。工具 schema 同步声明全部 7 个字段，`action_evidence_quote` 为可选。
- **产物形状**：`{"summary": "<人话总结>", "issues": [...]}`。`summary` 是模型提交后用自然语言给出的整体判断；`issues` 是结构化问题列表。
- **落库优先取工具参数**：模型没调工具时退回旧路径（解析正文 JSON 数组）；正文也不是 JSON 时 `issues` 置空、正文当人话总结。同一次运行里可能提交多次（例如首次因 severity 非法被运行时退回），以**最后一次通过校验**的提交为准；有提交但全部不合法才报错。前端 `parseReviewIssues` / `parseReviewSummary` 同时兼容新对象形状与历史裸数组产物。
- **两套工具机制并存**：review 的检索类工具（`search_story`、`continuity_check` 等）仍在 `prepare_context` 阶段由 `tool_runtime::prepare_tools` 预执行，结果作为 user prompt 注入；多轮循环只下发 `submit_review_issues` 负责回传结论。前者喂上下文，后者收结果，`enabled_tool_keys` 不参与这一阶段的隐式下发。
- **边界**：多轮 tool-calling 只作用于试读；draft / revision 等仍是单轮纯文本路径。工具预算是收尾而不是熔断，沿用 `MAX_TOOL_ROUNDS = 4`。
- **超时与重试**：客户端级超时只覆盖到响应头，上游若在正文中途停住会无限等待；`ai::read_response_text_bounded` 为所有非流式响应体读取补了一层上限。工具规划调用（`plan_tool_calls_native`）额外做 3 次尝试——实测 MiMO 偶发「连上了但不返回」，重试通常即可拿到结果；供应商明确不支持原生工具调用时不重试。

## 7. 新增 Agent 检查清单

- 在 `BUILTIN_AGENT_KEYS` 中登记默认 prompt，并在 `prompts/v2/` 提供 .md 模板。
- 在 `prompt_templates.rs` 的契约锚点测试中登记该 Agent 的契约片段。
- 仅在职责、输入边界、工具或交付方式有实质差异时新增 Agent；仅换题材方法时使用 Skill。明确是否需要共享阶段协议，并更新路由清单。
- 写清 .md 模板、运行时 user prompt、解析器/校验器各自负责的内容；输出契约只放 .md。
- 为机器输出增加解析失败、空结果、非法字段和跨项目边界测试。
- 不通过 prompt 宣称完成数据库写入、人工确认或事实确认；这些状态必须由 Rust 和数据库记录决定。
