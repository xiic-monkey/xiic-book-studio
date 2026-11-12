# Agent 体系设计说明

## 1. 组成与路由

当前内置 16 个 Agent，按运行时获取方式分为两类：

### 题材合成路径（5 个）

`story_architect`、`draft`、`review`、`revision`、`orchestrator`

这些 Agent 通过 `get_agent_for_project_stage` 获取。数据库先读取项目题材 profile，再由 `compose_stage_agent` 按以下顺序生成最终 system prompt：

1. 原始阶段 Agent 的身份和任务；
2. 题材专属 Agent 身份；
3. 题材 profile 允许的 Skill 白名单；
4. 仅合成路径使用的共享事实与 Canon 边界协议。

共享协议放在 system prompt 的末尾，作为合成路径的统一边界；实际题材 Skill 内容、项目资料和阶段数据由运行时 user prompt 追加。

### 裸 Agent 路径（11 个）

`chapter_memory`、`continuity_ledger`、`continuity_check`、`story_index`、`adoption`、`canon_align`、`context_search_plan`、`context_search_rerank`、`artifact_revision`、`continuity_review`、`chapter_split_plan`

这些 Agent 直接通过 `get_agent` 获取，不自动继承 `COMPOSED_AGENT_PROTOCOL`。新增副 Agent 时必须明确登记其入口、输出契约和验证位置，不应默认复制完整题材协议。

路由清单位于 `src-tauri/src/genre_agent.rs`，内置 Agent 总表位于 `src-tauri/src/prompt_templates.rs`；测试会验证两者数量、唯一性和全集一致。

## 2. 契约分层边界规则

一条硬规则：**`.md` 模板 = 人格 + 行为准则 + 输出契约；Rust 运行时 user prompt 只注入数据与任务上下文；解析器负责最终校验。**

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

## 4. 题材合成与 Skill

`detect_genre_skill` 使用题材字符串标记进行路由。当前支持通用连载、都市异能、悬疑和男频修仙/玄幻升级流；未命中窄题材的“科幻”“奇幻”“历史”等输入落到通用连载。组合题材按检测顺序处理，测试锁定悬疑优先于都市标记、升级标记优先于都市异能标记的现状。

Skill 分两层，按职责切分、互不复述：

- `skills/craft/continuity_and_agency.md`：跨题材通用规则（角色已知信息断点、物件状态连续、主角主动选择、探索章控密度等）。规则陈述在“状态类别”层面（持有、存放、完好度、知情、约束、时限、约定），示例只允许使用所有题材的最小交集词汇（门、锁、伤势、钱物、约定、期限、信息等）；信物、封印、阵法、丹药这类具象题材道具不得出现在通用层。
- `skills/genres/*.md`：题材专属写法（题材词汇、章节模式、severity 约定等），在这里可以且应该使用具象题材词汇，例如仙侠 skill 用“灵物、令牌、战力层级”把通用规则实例化。与 craft 语义重复的纯复述条目不得回填到题材 skill；只有携带题材特有语义的条目（如能力越界的战力层级 severity 映射）才放这里。

Skill 只能提供方法和题材写法，不能创造本书事实。`compose_stage_agent` 会将 stage Agent 的 `allowed_skill_keys` 与题材 profile 白名单求交集，未获允许的题材 Skill 不会被注入。`render_stage_scoped_skill` 按 `## Always` / `## Stage: <stage>` 切片注入，守卫测试保证五个模板覆盖全部六个阶段，段落缺失会直接让测试失败而不是静默丢规则。

## 5. Story Architect 工具例外

Story Architect 在 `setting`、`outline`、`characters` 阶段即使旧配置没有显式勾选知识卡工具，也会隐式获得创建、更新和提议知识卡工具，以避免回退到 Markdown 资料流程。Preview 只允许可预览的只读工具，因此这些写入工具在 preview 中不会出现。

资料卡的细分规则和已有卡片上下文由运行时追加的“结构化资料卡工作方式”提供；静态 `story_architect.md` 只保留身份和高层 Canon 原则。

## 6. 新增 Agent 检查清单

- 在 `BUILTIN_AGENT_KEYS` 中登记默认 prompt，并在 `prompts/v2/` 提供 .md 模板。
- 在 `prompt_templates.rs` 的契约锚点测试中登记该 Agent 的契约片段。
- 明确选择题材合成路径或裸取路径，同时更新 5/11 路由清单。
- 写清 .md 模板、运行时 user prompt、解析器/校验器各自负责的内容；输出契约只放 .md。
- 为机器输出增加解析失败、空结果、非法字段和跨项目边界测试。
- 不通过 prompt 宣称完成数据库写入、人工确认或事实确认；这些状态必须由 Rust 和数据库记录决定。
