# Agent 体系设计说明

## 1. 组成与路由

当前内置 15 个 Agent，按运行时获取方式分为两类：

### 题材合成路径（5 个）

`story_architect`、`draft`、`review`、`revision`、`orchestrator`

这些 Agent 通过 `get_agent_for_project_stage` 获取。数据库先读取项目题材 profile，再由 `compose_stage_agent` 按以下顺序生成最终 Agent：

1. 共享事实与 Canon 边界协议；
2. 题材专属 Agent 身份；
3. 题材 profile 允许的 Skill 白名单；
4. 原始阶段 Agent 的身份和任务。

### 裸 Agent 路径（10 个）

`chapter_memory`、`continuity_ledger`、`continuity_check`、`story_index`、`adoption`、`context_search_plan`、`context_search_rerank`、`artifact_revision`、`continuity_review`、`chapter_split_plan`

这些 Agent 直接通过 `get_agent` 获取，不自动继承 `BASE_AGENT_PROTOCOL`。它们的业务契约主要由运行时 user prompt、解析器和校验逻辑提供。新增副 Agent 时必须明确登记其入口、输出契约和验证位置，不应默认复制完整题材协议。

路由清单位于 `src-tauri/src/genre_agent.rs`，内置 Agent 总表位于 `src-tauri/src/prompt_templates.rs`；测试会验证两者数量、唯一性和全集一致。

## 2. 三层契约职责

### 静态 Agent prompt

静态 prompt 负责身份、长期职责和少量不可误解的输出边界。它不应复制已有的阶段业务格式、数据库字段约束或运行时上下文。

### 运行时 user prompt

运行时 prompt 负责当前项目、章节、已批准资料、任务目标、阶段行为和本次人工指令。阶段行为约束以这里为主来源，避免静态模板和运行时字符串各维护一份。

### 解析与校验代码

Rust 解析器和数据库边界是最终事实来源。模型遵守的格式必须在代码侧再次解析、校验、归一化和检查项目归属。Prompt 不能替代安全校验。

`adoption` 是机器可读输出的例外重点：其 prompt 声明 JSON 数组和候选字段，`parse_extracted_candidates`、`normalize_data`、证据引文校验和外键校验共同决定候选是否可进入待人工确认流程。

## 3. 题材合成与 Skill

`detect_genre_skill` 使用题材字符串标记进行路由。当前支持通用连载、都市异能、悬疑和男频修仙/玄幻升级流；未命中窄题材的“科幻”“奇幻”“历史”等输入落到通用连载。组合题材按检测顺序处理，测试锁定悬疑优先于都市标记、升级标记优先于都市异能标记的现状。

Skill 只能提供方法和题材写法，不能创造本书事实。`compose_stage_agent` 会将 stage Agent 的 `allowed_skill_keys` 与题材 profile 白名单求交集，未获允许的题材 Skill 不会被注入。

## 4. Story Architect 工具例外

Story Architect 在 `setting`、`outline`、`characters` 阶段即使旧配置没有显式勾选知识卡工具，也会隐式获得创建、更新和提议知识卡工具，以避免回退到 Markdown 资料流程。Preview 只允许可预览的只读工具，因此这些写入工具在 preview 中不会出现。

资料卡的细分规则和已有卡片上下文由运行时追加的“结构化资料卡工作方式”提供；静态 `story_architect.md` 只保留身份和高层 Canon 原则。

## 5. 新增 Agent 检查清单

- 在 `BUILTIN_AGENT_KEYS` 中登记默认 prompt，并提供恢复默认 prompt 的测试。
- 明确选择题材合成路径或裸取路径，同时更新 5/10 路由清单。
- 写清静态 prompt、运行时 user prompt、解析器/校验器各自负责的内容。
- 为机器输出增加解析失败、空结果、非法字段和跨项目边界测试。
- 如果需要共享规则，先证明它确实跨 Agent 且不存在运行时 canonical source，再引入轻量公共片段。
- 不通过 prompt 宣称完成数据库写入、人工确认或事实确认；这些状态必须由 Rust 和数据库记录决定。

## 6. 后续治理项

`prompts.default_version` 当前只在数据库初始化时写入，没有读取点，不参与 prompt 选择。版本迁移、NEG 实验 denylist、结构化合成元数据和 `WritingSkill.category` enum 作为后续独立变更处理，避免在没有迁移和行为实验设计时扩大本轮改动。
