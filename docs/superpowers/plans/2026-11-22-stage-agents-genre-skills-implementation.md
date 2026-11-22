# 阶段 Agent 与题材 Skill 解耦 Implementation Plan

## 实施状态（2026-11-22）

- 任务 1–4 已完成。任务 5 已完成集成验证、浏览器走查、文档同步和回退演练。各任务下的步骤保留实施时的检查方法。
- 已验证 Rust 单元测试 `241 passed, 3 ignored`、前端 Vitest `69 passed`、TypeScript/Vite 生产构建、Dev REST 临时库回读和 Chrome 页面走查。
- 已用本地模型请求记录器验证悬疑/仙侠/禁用仙侠的实际试读请求；未调用付费模型，未宣称质量 A/B 改善。
- 旧版二进制回退只在数据库副本上验证：正文与历史运行可读，新项目能补绑定；已有失配旧绑定需要在备份副本中移除后由旧版重建。详见 [`agent-design.md`](../../agent-design.md#旧版回读)。

**Goal:** 从 Agent 身份、章节任务卡和设置界面中去掉重复的题材规则，同工种共用 Agent，题材只由运行时选中的 Skill 注入。

**Architecture:** 阶段 Agent 负责流程与交付，共享工作协议继续约束事实和候选产物；`Project.genre` 直接决定题材 Skill。写作流程与审校共享阶段规则快照，预览和跨章审校缓存对规则改动敏感；工作区只返回选中 Skill 的展示信息。

**Tech Stack:** Rust、Tauri、SQLite/rusqlite、ts-rs、React、TypeScript、Vitest。

**Spec:** [阶段 Agent 与题材 Skill 解耦设计](../specs/2026-11-22-stage-agents-genre-skills-design.md)

## 全局约束

- 不改 SQLite schema 版本；保留 `project_genre_agents` 表和存量记录用于旧版本回读。
- 不改 Agent ID、Skill key、工具配置、定制标记、启用状态和历史数据。
- 题材映射顺序、阶段 JSON/工具契约、正文采用门禁、编排依赖和取消机制不变。
- 不增加运行时依赖，不批量编辑用户自定义 Prompt 或 Skill，不手改 `src/generated/v2-contracts.ts`。
- 只在不同职责或输入输出确有需要时新增 Agent；本计划不新增。
- 用隔离临时库或测试副本验证，不在实际小说库上做测试。运行命令均从仓库根目录执行。每个任务先验证测试能复现旧行为，再验证通过；阶段检查点不自动提交。

## 优先防护的边界

| 风险 | 应有行为 | 对应任务 |
|---|---|---|
| 关闭 Skill 仍载入旧题材写法 | system prompt、章节任务卡和规则区块均无应用内旧要求；项目事实和 Craft 不消失 | 1、2 |
| 编辑发生在预览准备的异步工具期间 | 文本与指纹使用同一快照，旧预览不能通过新设置的校验 | 3 |
| 存量自定义正文未用标准小节，或者旧项目还绑了另一个题材 | 无可渲染规则则不注入，不覆盖原文；旧绑定不影响路由 | 2、4 |
| 超长候选稿、只含 Stage 的自定义 Skill | 候选稿中的 `Skill` 字样不改变预算优先级；裁掉唯一规则时不留下假注入区块 | 2 |
| 修改规则后，同样两章命中旧跨章审校 | 当前实际规则变化时重跑；旧报告仍可查看 | 3 |

## 文件分工

| 位置 | 职责 |
|---|---|
| `src-tauri/src/genre_agent.rs` → `src-tauri/src/stage_agent.rs`、`lib.rs`、`db.rs` | 阶段身份、共享工作协议；移除旧题材 Agent 的运行时选择 |
| `src-tauri/src/genre_skill.rs`、`src-tauri/skills/genres/*.md` | 路由、选段、规则快照；题材写法唯一来源 |
| `src-tauri/src/workflow.rs`、`agent_run_service.rs`、`story_architecture.rs` | 各调用链、任务卡、预算、预览与跨章审校 |
| `src-tauri/src/models.rs`、`application.rs`、`bin/generate_contracts.rs` | 工作区与详情 DTO、生成契约 |
| `src/types.ts`、`src/generated/v2-contracts.ts`、`src/components/SettingsView.tsx`、`src/features/workbench/BookStudioWorkspace.tsx` | 设置页与 API 类型 |
| `docs/agent-design.md`、`docs/PROJECT_FUNCTIONS.md` | 实施通过后同步现行行为；保留历史报告原数据 |

---

## 任务 1：固定阶段 Agent 与共享协议 · 已完成

**文件：** `src-tauri/src/genre_agent.rs`、`src-tauri/src/db.rs`、`src-tauri/src/agent_run_service.rs`、`src-tauri/src/story_architecture.rs`。

**输出接口：** `stage_agent::apply_stage_protocol(agent: Agent) -> Agent`（任务 4 移入 `stage_agent.rs`）；`AppState::get_execution_agent(&self, stage: &str) -> AppResult<Agent>`；`get_agent` 和 `get_agent_for_stage` 仍返回未拼接的存储配置。

- [ ] 在 `db.rs` 添加 `execution_agent_is_independent_of_project_genre`：同工种在悬疑、仙侠项目中使用相同 `id`、`stage`、`role`、`system_prompt`、模型与工具；设定、大纲、角色仍映射至同一 `story_architect`。共享协议只出现一次，自定义 Agent Prompt 原样保留，辅助原始 Agent 不被加上新协议。先对当前项目组合路径运行，预期题材身份不同导致失败。
- [ ] 去掉执行路径上的四个题材身份及 Skill 声明；将共享协议改为与题材无关的标题和入口。`get_execution_agent` 先走 `get_agent_for_stage` 的既有映射，再给 `story_architect`、`draft`、`review`、`revision`、`orchestrator` 附加协议；其它 Agent 保持原样。不得改变事实优先级、阶段边界、`editable_system_prompt` 或 `editable_role`。从 `workflow.rs`、`agent_run_service.rs`、`story_architecture.rs` 切换调用；`ProjectDetail.agents` 取未拼接的配置。旧 profile API/表读写可临时留到任务 4，但不再影响一轮运行的 Agent。
- [ ] 添加 `orchestrator_request_has_genre_label_without_skill_rules`：记录模拟模型的实际主 Agent 请求，含当前项目题材标签、不含内置题材规则；用既有编排测试保证任务依赖与取消不变。删除 `render_orchestrator_genre_skill` 与该调用位；创作基准一致性审校保留架构 Agent 协议，但不再有题材提示或虚假 Skill 声明。需要验证基准审校传入的快照和输出解析未变。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib execution_agent_is_independent_of_project_genre` 和 `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_run_service::tests`；各目标通过，没有新增 Agent 注册项。

## 任务 2：统一 Skill 注入，移除题材硬编码 · 已完成

**文件：** `src-tauri/src/genre_skill.rs`、`src-tauri/src/db.rs`、`src-tauri/src/workflow.rs`、`src-tauri/src/agent_run_service.rs`、`src-tauri/skills/genres/{mystery,urban_supernatural,xianxia_power_fantasy}.md`。

**输出接口：** `genre_skill::load_stage_skills(state: &AppState, project: &Project, stage: &Stage, allowed_skill_keys: &[String]) -> AppResult<StageSkillContext>`。`StageSkillContext` 持有 `selected_genre_key: &'static str`、`genre_rules: String`、`craft_rules: String`，用 `append_to(&self, prompt: &mut String)` 依次追加渲染文本。不往类型中放 Agent Prompt 或数据库连接。

- [x] 用 `genre_skill::tests` 和工作流回归测试验证题材映射、组合优先级、未知题材、缺行兜底及启停行为。覆盖 `Always`、阶段段落、空内容和 Craft 许可，断言实际渲染出的规则。
- [ ] 实现 `load_stage_skills`：按 `project.genre` 选择 key，复用 `effective_genre_skill_content` 的缺行兜底；`render_stage_scoped_skill` 在两段均空时返回空串。Craft 仅在许可含 `continuity_and_agency`、数据库记录类型为 `craft`、启用且有可渲染内容时加载，不从题材 profile 取白名单。不保留独立的 Skill 可用性声明扫描。把 `build_prompt_internal` 的两个旧渲染调用收敛为一次；`continuity_review_method_context` 改用同一入口与 `Stage::Review`。模拟请求覆盖六阶段、直接架构工具路径、修订反馈入口和跨章审校。主 Agent、创作基准审校、提案提取不调用该入口。
- [ ] 添加 `chapter_prompt_disabling_genre_skill_has_no_legacy_payoff`：悬疑、都市异能、仙侠各给一个带题材的项目和章节，关闭题材 Skill 后断言候选/任务卡依然在、旧 `本章进度增量` 行和三个题材专属硬编码文案均不在，重新启用后取自当前 Skill 的唯一标记恢复。实现时去掉 `ChapterTaskContract.payoff`、`xianxia_progress_requirement` 及其渲染行，保留通用 `chapter_mode` 和 `required_change`。将缺少的增量标准合并进内置 `Stage: draft`，不复制规则、不添加 `payoff` 字段；同时对照架构工具契约移除 Skill 中越权的要求（例如设定阶段不写主角第一章，章节计划不新增硬字段）。已有自定义 Skill 不改写。`draft_prompt_uses_genre_specific_skill_guidance` 等现有测试改查 Skill 规则，不再依赖旧 Rust 文案。
- [ ] 添加 `budget_classifies_only_real_skill_headings`：带 `Skill` 字样的长候选稿仍按候选稿优先级压缩；复制同名标题但没有来源行不被当作 Skill；真实渲染区块优先保留；只含 Stage 的区块若需要剥离阶段细则，整段移除并记为 dropped。定义 `is_skill_section(&str) -> bool`，匹配 `# 题材 Skill`、`# 写作 Craft Skill` 标题和下一行 `Skill 来源：应用内写作资料库 / ` 前缀；不用段正文 `contains("Skill")`。判断优先级和各裁剪分支复用该函数；保留 `MAX_WORKFLOW_PROMPT_CHARS = 24_000`、任务尾部及裁剪诊断。仅改已有裁剪入口，不给跨章审校新增预算，不新增前端运行事件。
- [ ] 分别运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib genre_skill::tests`、`cargo test --manifest-path src-tauri/Cargo.toml --lib workflow::tests`，检查路由、阶段切片、事实边界和预算测试通过；要求模拟运行实际收到的 Prompt 中不出现旧身份。

## 任务 3：预览与跨章审校缓存按规则失效 · 已完成

**文件：** `src-tauri/src/agent_run_service.rs`、`src-tauri/src/workflow.rs`。

**输入接口：** 任务 2 的 `StageSkillContext`。

**输出接口：** `workflow::build_prompt_for_agent_with_skills` 沿用 `build_prompt_for_agent` 的参数并追加 `skills: &StageSkillContext`，返回 `AppResult<String>`；`context_fingerprint` 同样追加 `skills: &StageSkillContext`；`continuity_cache_key(project_id: i64, chapter_inputs: &[(i64, i64, String)], method_context: &str) -> AppResult<String>`。

- [x] 添加 `prepared_context_rejects_changed_effective_skill`：同一请求预览后，编辑当前阶段规则、启停选中题材 Skill、修改已允许 Craft，分别使旧 `prepared_context_id` 校验失败。修改未选中题材、修改只适用于其它阶段的规则时保持可用。复用现有指令与来源验证，并断言旧上下文不会被用于启动新 run。
- [ ] 添加 `preview_keeps_skill_snapshot_across_async_tool_preparation`：在模拟工具回调或等价的确定性同步点改规则，断言预览文本仍为旧标记，且返回的指纹不能通过当前配置的验证。把快照加载、基础 Prompt 与指纹计算放到 `prepare_tools(...).await` 之前，保存时不重读；校验显式预览 ID 时加载当前快照并比较。指纹序列化增加 `selected_genre_key`、`genre_rules`、`craft_rules` 与构建协议版本，沿用已有 SHA-256；不哈希未渲染的整个 Skill 文件。标准 `build_prompt_for_agent` 自行加载一次后调用带快照的版本，避免普通入口重复传参。不带 ID 的启动使用本次自动准备的快照，不因任务启动后修改配置而重启。保留正式执行的现有账本/工具补充与最终预算，不能因此重新加载或替换已准备的 Skill 文本；不把预览保证成最终调用的逐字副本。
- [x] 跨章审校缓存回归：相同上下文复用结果，修改或禁用适用规则、改动章节版本/正文、更新已采用的设定或资料卡后重新审校。键 `review=v5|project={project_id}|snapshot=<sha256>` 哈希 `serde_json` 编码的章节输入和实际发送的项目、已采用资料、审校规则。仍先查缓存再跑账本，不改原文证据校验，不清库、不改基准审校指纹。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib prepared_context_` 和 `cargo test --manifest-path src-tauri/Cargo.toml --lib continuity_cache_`；确认缓存、重复运行、过期与旧报告查看路径行为都符合预期。

## 任务 4：切换工作区契约与前端，停用旧绑定 · 已完成

**文件：** `src-tauri/src/{models,application,db,lib}.rs`、`src-tauri/src/bin/generate_contracts.rs`、`src-tauri/src/genre_agent.rs` → `src-tauri/src/stage_agent.rs`、`src/types.ts`、`src/generated/v2-contracts.ts`、`src/components/SettingsView.tsx`、`src/features/workbench/BookStudioWorkspace.tsx`。

**输出接口：** `ProjectGenreSkill { skill_key: String, name: String, enabled: bool }`；`AppState::get_project_genre_skill(&self, project: &Project) -> AppResult<ProjectGenreSkill>`；`ProjectWorkspace.genre_skill` 与 `ProjectDetail.genre_skill`。

- [ ] 添加 `project_genre_skill_ignores_legacy_binding` 与 `genre_change_only_changes_selected_skill`：在临时库插入错误旧 key、缺绑定和已禁用的自定义 Skill；断言只按项目题材返回 key，保留库名称与开关，读取不改表，修改题材不换 Agent，更新其它资料不换 Skill。重开库后保持自定义规则和开关；无记录返回内置名称和 `enabled: true`。替换原来依赖「保持/更新题材 Agent 绑定」的测试；保留独立 `urban_mystery` 历史 Skill 停用迁移测试。
- [ ] 添加 `ProjectGenreSkill`（沿用 `Serialize`、`Deserialize`、`TS`），分别填充 `db.rs::get_detail` 和 `application.rs::get_project_workspace`，复用已经取得的 `Project`。`enabled` 直接取 Skill 库开关，规则为空或运行时裁剪不改这个值。移除建项目、更新项目、启动 backfill 和解析时针对 `project_genre_agents` 的读写，保留建表 SQL 和数据库版本。删除 `GenreAgentProfile`、旧查找与声明辅助函数；把剩余的阶段协议及其注册测试移至 `stage_agent.rs`。调整 Tauri/Dev REST 共用返回类型，路径和命令名不变；先检查有无仓库外调用方需要协调升级。
- [ ] 更新 `generate_contracts.rs` 后运行 `npm run contracts:generate`，再改 `src/types.ts` 中手写 `ProjectDetail` 的字段与类型引用。`BookStudioWorkspace` 把 `detail?.genre_skill` 传给 `SettingsView`；设置 → Agent 删专属身份卡；设置 → Skill 仅有当前项目时显示「项目题材 Skill」与名称、启用/未启用。最近的 `writingSkills` 同 key 记录优先于 DTO 名称/开关，缺记录时显示 DTO 值。复用现有保存回调和列表更新，不增加第二个开关或 API。
- [ ] 添加前端用例，在现有 Vitest 工具链下验证无项目、库状态比工作区 DTO 新、缺记录兜底三种状态；可以用 `react-dom/server` 静态渲染有状态行的纯组件，不新增 DOM 或浏览器依赖。手动走一次切换项目题材、打开设置、关闭 Skill 并查看状态和下一次预览；专属 Agent 卡不再出现、阶段配置仍可保存。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib project_genre_skill_`、`npm test`、`npm run build`。复查工作区类型与 Dev REST JSON；仅历史文档和保留表定义/兼容测试可以再出现旧术语或旧表名。

## 任务 5：集成与回退演练已完成，模型质量对照待测

**文件：** `docs/agent-design.md`、`docs/PROJECT_FUNCTIONS.md`；如执行了模型对照，在 `docs/test-reports/` 新增本轮报告，不修改旧 F7 结果。

- [x] 运行全套 `cargo test --manifest-path src-tauri/Cargo.toml`、`npm test` 和 `npm run build`。模拟模型请求确认悬疑和仙侠共用试读 Agent，请求携带各自的题材规则，关闭题材 Skill 后保留项目上下文和证据边界。定向测试覆盖跨章审校、显式预览和主 Agent 委托。
- [x] 用旧版 `8cf240e` 和当前 `dev_api` 验证了临时库副本。新版读取旧正文和运行记录，按当前题材路由。旧版自动为新书补绑定。已有合法 key 的失配绑定需要先在备份副本中移除该行，旧版重启后才能按新题材补齐。演练用关闭调试断言的旧构建回读含工作区已有 v12 迁移的库。发布和回退都将前端与后端作为同一版本处理。
- [ ] 若具备可用模型和批准使用的样本，为悬疑和仙侠各挑一个固定章节，分别对比新版、旧版的写作与试读，每组至少 3 次。固定模型、解码参数、人工指令和同样的已采用事实/候选稿；试读用固定候选稿而不是本次不同的新稿。记录实际发送 Prompt、是否有预算裁剪、原文引文合法性、硬问题召回、误报和章节任务兑现。测试题材规则的区别时，另用同一个新版二进制做启用/关闭对照；不要把两种对照混为一谈。不可比较或无凭据时标记未测/未定，不以感觉作结论。
- [x] 按[设计验收](../specs/2026-11-22-stage-agents-genre-skills-design.md#5-验收与质量验证)复核运行契约，并同步现行文档中的阶段职责、题材路由、技能开关、设置页和 Dev REST 字段。
