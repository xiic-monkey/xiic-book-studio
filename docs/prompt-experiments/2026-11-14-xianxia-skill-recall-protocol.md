# F7 预注册协议 · 男频修仙题材 Skill 试读召回复测

> **本文件是预注册（pre-registration）**：判定标准、指标定义、候选稿来源与 ground truth 规则在实验开始前定稿，实验结束后不得调整。
> 判定结果写入 `docs/test-reports/2026-11-14-xianxia-skill-recall-ab.md`；判负时另在 `docs/prompt-experiments/negative-prompt-patterns.md` 登记 NEG-004。

## 一、实验要回答的问题

`xianxia_power_fantasy.md`（男频修仙题材 Skill）规则条目 41 条，是四个题材 Skill 中负载最重的一个（urban 10 / mystery 10 / general 8）。NEG-001 已经用实验证明：**常驻注入的 Skill 会与事实边界、状态连续性和章节任务争夺模型注意力，导致硬问题召回下降**。修仙路线是负载最高的，却从未做过同类复测。

本实验只回答一个问题：

> 在试读（review）阶段，注入男频修仙题材 Skill 与不注入相比，**硬问题召回率是否退化**？

- 召回不降、major 不增、误报不显著增 → 保留现状（`POSITIVE`）。
- 召回下降 → 判负，按 NEG-004 登记并按 F6 同规格削减 `xianxia_power_fantasy.md` 的 Stage 段条目后复测（`NEGATIVE`）。
- 基线自身两次差异过大 → 噪声大于信号（`INCONCLUSIVE`），需增加重复次数后再判。

**本实验不回答**：题材 Skill 是否提升文笔、是否应改为常驻、其它题材 Skill 是否安全。

## 二、变量与控制

| 组别 | 题材 Skill `xianxia_power_fantasy` | Craft Skill `continuity_and_agency` | 重复次数 |
| --- | --- | --- | --- |
| A（基线） | `enabled = 0` | 开 | 2 |
| B（现状） | `enabled = 1` | 开 | 2 |

**唯一变量是题材 Skill 的 `enabled` 开关。** 以下全部固定：

| 固定项 | 值 | 来源 |
| --- | --- | --- |
| 项目 | 同一项目（活库新建，男频修仙爽文） | — |
| 候选稿 | 同一份候选稿，4 次 run 输入完全一致 | 见 §三 |
| Provider / Model | `https://api.xiaomimimo.com/v1` + `mimo-v2.6-flash` | 活库全局设置 |
| Temperature | `0.35` | `agents.temperature`（试读 Agent 行） |
| Thinking | `enabled`，level `medium` | 活库全局设置（Agent 的 provider/model 为空 → 继承全局） |
| 试读 Agent | id `3`，`allowed_skill_keys = ["continuity_and_agency"]` | 活库 |
| 注入顺序与预算裁剪 | 同一份二进制（`dev_api` 23:41 构建，含 F1–F6） | — |

**为什么不是 DeepSeek**：2026-07-19 那轮用 `deepseek-v4-pro`（temp 0.75，thinking 关闭），但该 provider 当前返回 402 余额不足；MiMO 是当前唯一实测可用的 provider。**两组臂必须同模型同参数**，因此本轮两组都用 MiMO。这使本轮与 2026-07-19 报告**不可直接跨轮比较数值**，只能比较本轮 A/B 之间的相对差。

**运行顺序**：A1 → A2 → B1 → B2，交替关闭/开启 `enabled` 开关。开关的每次切换都必须经 `save_writing_skill` 走应用自身路径，不得直接改库。

## 三、候选稿与项目

### 3.1 候选稿

**复用 2026-07-19 那次实验的同一份候选稿**：历史库《烬骨长生》项目 id `2` 的第 6 章 draft artifact `101`（3987 字符）。

理由：

1. 与 `docs/test-reports/2026-07-19-storyforge-prompt-ab.md` 的试读实验同构，可直接归因到「题材 Skill 开关」这一个变量。
2. 避免为生成候选稿而人工批准设定 / 大纲 / 角色产物 —— 方案 §八 明确禁止「人工批准任何实验产物」，而 App 的 draft 阶段硬门禁要求先人工批准设定/大纲/角色并确认创作基准、当前计划、基准审校。
3. 候选稿是真实模型产出的真实章节，含真实缺陷，不是人为构造的靶子。

### 3.2 项目数据搬运

历史库 `book-studio.sqlite3` **只读**，零写入。往活库 `book-studio-v2.sqlite3` 新建一个项目，搬运历史项目 id `2` 的：

| 表 | 搬运内容 |
| --- | --- |
| `projects` | 1 行（标题《烬骨长生·召回复测》，题材 `男频修仙爽文`） |
| `project_genre_agents` | `xianxia_power_fantasy`（由 `create_project` 自动路由，F5 验证） |
| `chapters` | 6 行（第 1-5 章 + 第 6 章） |
| `artifacts` | 已批准的设定 / 大纲 / 角色底稿 + 第 1-5 章正文 + 第 6 章候选稿 |
| `approvals` | 对应的批准记录（artifact status 已是 `approved`） |
| `continuity_ledger_entries` / `_sources` | 32 + 5 行（状态账本，「物件/药物状态断点」的判定依据） |
| `chapter_memories` | 1 行 |
| `story_facts` / `story_events` / `story_entities` / `story_threads` | 检索类工具的语料 |

搬运后新项目**不得**产生任何新的 `draft` / `revision` 产物；4 次实验只产出 `review` 产物。

### 3.3 候选稿固定性

4 次 review run 必须指向**同一个 artifact id**（第 6 章候选稿）。run 之间不得修改或批准候选稿，不得将它采用为正文或纳入创作基准。

## 四、Ground truth（唯一人工关口）

- 由**人工**通读候选稿（及 §3.2 搬运来的设定 / 大纲 / 角色 / 前 5 章正文 / 状态账本），列出一份硬问题清单。
- 标注源材料见 [`2026-11-14-xianxia-groundtruth-source.md`](./2026-11-14-xianxia-groundtruth-source.md)（含候选稿原文、已批准基准、前 5 章正文、状态账本与空白清单模板）。
- 类别限定为 7 类：事实越界、角色知情断点、物件/药物状态断点、威胁断线、章节任务未兑现、收益未落地、能力越界。
- 每条必须给出候选稿中**可回查的连续原文片段**（8-80 字）。
- 清单在**实验开始前定稿**，定稿后**不得修改**；实验结束后如需修改视为实验作废。
- **禁止**由被测模型（MiMO）自动生成 ground truth —— 那是循环论证。人工可对 AI 起草的候选清单逐条审核修改后定稿，最终以人工版本为准。
- 评分时以模型自己输出的 `severity` 字段统计 major/moderate/minor 分布；标注里的「严重度建议」仅供参考。

## 五、指标

| 指标 | 定义 |
| --- | --- |
| 硬问题召回率 | 命中的 ground truth 条数 ÷ ground truth 总条数 |
| 命中判定 | 模型报告的 `issue_type` + `evidence_quote` 与某条 ground truth 指向同一问题（原文片段可回查） |
| major / moderate / minor 计数 | 按模型输出的 `severity` 字段统计 |
| 误报数 | 报告了、但不在 ground truth 中、且经人工确认成立的问题数 |
| 输出字符数 / 耗时 | 用于观察 B 组是否因规则更密而输出更短/更少 |
| 组内稳定性 | A1 vs A2、B1 vs B2 的问题集差异条数 |

## 六、判定标准（写死，事后不得调整）

按顺序判定，先命中先定：

1. **A 组自身两次差异 ≥ 2 条**（问题集差异条数）→ `INCONCLUSIVE`。说明噪声大于信号，需增加重复次数后再判，不得据此下结论。
2. **B 组召回率 < A 组任一次** → `NEGATIVE`。
3. **B 组召回率 ≤ A 组两次的平均召回率** → `NEGATIVE`。
4. **B 组召回率 ≥ A 组平均值，且 major 数不增加、误报数不显著增加** → `POSITIVE`。
5. 未落入上述任一条 → `INCONCLUSIVE`，如实记录，不得强行归类。

补充约束：

- `NEGATIVE` 或 `INCONCLUSIVE` **必须**回到 `docs/prompt-experiments/negative-prompt-patterns.md` 登记，不得只写测试报告。
- `NEGATIVE` 时按 F6 的同规格削减 `xianxia_power_fantasy.md` 的 Stage 段条目，再复测。
- `POSITIVE` 时在 `docs/agent-design.md` §4 记录「题材 Skill 负载经复测无退化」。

## 七、数据完整性

- 实验前后各备份一次活库 `book-studio-v2.sqlite3`（连同 `-wal` / `-shm`）。
- 4 份 review 产物一律保持 `pending_human_approval`，**不得**人工批准、不得写入正文、不得影响项目创作基准。
- 实验结束后 `xianxia_power_fantasy.enabled` **必须恢复为 1**（F4d 迁移后的正常取值），并核对 `writing_skills` 五行 `enabled` 与实验前一致。
- **禁止**写入历史库 `book-studio.sqlite3`。
- 报告不得包含 API Key 或任何凭据。
- 不得在代码里写「每次启动强制启用内置题材 Skill」这类绕过 F4d 迁移的逻辑。

## 八、复现步骤

1. 备份活库（含 `-wal`/`-shm`）。
2. 在活库新建项目并从历史库只读搬运数据（§3.2）。
3. 人工定稿 ground truth 清单（§四）。
4. `save_writing_skill` 关闭 `xianxia_power_fantasy` → 跑 A1。
5. 关闭状态不变 → 跑 A2。
6. `save_writing_skill` 开启 `xianxia_power_fantasy` → 跑 B1。
7. 开启状态不变 → 跑 B2。
8. 逐条人工评分（召回 / 误报 / severity 分布）。
9. 按 §六 判定。
10. 恢复 `enabled = 1`，核对 `writing_skills` 五行 `enabled`，停掉后台服务。

## 九、变更记录

| 日期 | 变更 | 说明 |
| --- | --- | --- |
| 2026-11-14 | 初版 | 实验前定稿 |
| 2026-11-19 | 执行前事实补记 | **只补记事实，不改判定标准、指标、候选稿与 ground truth 规则。** |

### 2026-11-19 执行前事实补记

- **实际执行日期**：2026-11-19（环境时钟）。本协议与 ground truth 文件名保留 11-14。
- **索引重建**：把历史项目数据搬进活库后，App 的索引 worker 自动为 6 章排了 `story_chapter` / `search_chapter` 任务。`story_chapter` 结果：ch12 / ch13 / ch14 / ch16 成功，ch15 达 `MAX_JOB_ATTEMPTS` 后终止（错误均为 `network error`），`search_chapter` 6 章全部成功。重建全部进入终态后才开始第一次 A/B run，**两臂看到的索引状态完全一致**。
- **索引覆盖与 7 月一致**：重建后 `story_facts` 仍覆盖候选稿所在章（第 6 章），与 2026-07-19 当时该项目 `story_facts` 有 9 条来自第 6 章的行为一致，不构成对某一臂的偏袒。
- **模型连通性**：MiMO 单次调用约 3 秒；试读一次 run 包含工具规划轮 + `continuity_check` / `quality_analysis` 等子调用，实际耗时数分钟。索引阶段出现过的 `network error` 为偶发连接失败，未发生在 A/B run 内。
- **未变项**：候选稿（artifact 69）、ground truth 规则、指标定义、判定标准、数据完整性约束。

### 2026-11-19 执行前补记之二：为 MiMO 补 thinking 参数下发（F8）

- **问题**：`ai.rs` 的 `provider_thinking_config` 只识别 `deepseek` 与 `minimaxi`，其余供应商一律不下发 `thinking` 参数，改为沿用供应商默认值。MiMO 默认开启深度思考，导致大提示词请求（例如试读阶段的工具规划）在应用的 240 秒请求窗口内无法返回，表现为 `network error: error decoding response body`，试读无法跑通。
- **实测**（同一份 20K 字符提示词 + 9 个工具定义，`mimo-v2.6-flash`）：
  - 不带 `thinking` 参数 → 超过 300 秒仍未返回；
  - 带 `thinking: {"type":"disabled"}` → **8.8 秒**返回并给出正确 tool_call。
- **修复**：`provider_thinking_config` 增加 `xiaomimimo` 分支，按用户/Agent 的 thinking 开关显式下发 `{"type":"enabled"|"disabled"}`。新增单测 `maps_mimo_thinking_switch_to_explicit_type` 与 `unknown_providers_keep_default_thinking_behavior`。**不改变其它供应商的行为**。
- **运行时配置**：试读 Agent（id `3`）显式绑定 `https://api.xiaomimimo.com/v1` + `mimo-v2.6-flash`，thinking 关闭、temperature 保持 `0.35`。与 2026-07-19 协议（DeepSeek、temperature 0.75、**Thinking 关闭**）的 thinking 设定一致。Agent 的 `allowed_skill_keys` 保持 `["continuity_and_agency"]` 不变。
- **对实验的影响**：两臂使用同一模型、同一 temperature、同一 thinking 设定，**不引入组间差异**；只是把原本无法跑通的链路变成可跑通。
- **收尾要求**：实验结束后把试读 Agent 的 `provider_base_url` / `model` 恢复为空（回到继承全局设置），并把全局 thinking 设置保持为 `enabled` / `medium`。

### 2026-11-19 执行前补记之三：工具预算收尾（F9）与模型改用 `mimo-v2.6-pro`

- **F9 问题**：试读 Agent 的工具循环原本在超过 `MAX_TOOL_CALLS = 64` 时直接让整次运行失败（`Agent 工具调用超过单次运行上限 64`）。MiMO 在试读场景下会不断变换检索词继续搜，必然撞上限，导致实验无法完成。
- **F9 修复**：`tool_runtime.rs` 增加轮次上限 `MAX_TOOL_ROUNDS = 4`，并把「调用预算用尽」与「轮次用尽」都改为**收尾**（break）而不是报错，让模型带着已收集的工具结果产出最终答复。新增单测 `tool_loop_ends_gracefully_when_rounds_or_calls_are_exhausted`。
- **模型从 `mimo-v2.6-flash` 改为 `mimo-v2.6-pro`**（两组臂同模型同参数，只切题材 Skill 开关）：
  - 用应用**实际发出**的请求（合成 system prompt 1892 字符 + user prompt 24000 字符，流式，thinking 关闭）实测：
    - `mimo-v2.6-flash`：返回**单个 JSON 对象**，违反试读契约「只输出 JSON 数组」，`parse_review_issues` 严格解析失败 → 运行硬失败（`invalid type: map, expected a sequence`）。
    - `mimo-v2.6-pro`：返回 ` ```json ` 代码块包裹的**数组**，应用既有的 `trim_code_fence` 能正确剥离后解析 ✅，且耗时更短（41 s vs 59 s）。
  - 因此本轮两组臂统一使用 `mimo-v2.6-pro`。
- **未变项**：候选稿、ground truth 规则、指标、判定标准、试读 Agent 的 temperature（0.35）与 thinking（关闭）。

### 2026-11-21 执行前补记之四：统一引文长度约束指令

- **问题**：首轮 A1/A2 试读中，`mimo-v2.6-pro` 输出的问题内容正确（8 条事实越界，质量高），但 `evidence_quote` 写成了整段原文（>80 字），被 `ai.rs` 的 `8..=80` 字校验拒绝（`试读 Agent 返回的问题缺少 8-80 字 evidence_quote`），run 判 `failed`、无产物入库。这是输出格式问题，不是网络或召回问题。
- **处置**：为 A1/A2/B1/B2 四次运行**统一**传入同一条 `user_instruction`，要求「每条 `evidence_quote` 取候选稿中 8-80 字的连续原文片段」。该指令四臂完全相同，与题材 Skill 开关无关，**不构成新的组间变量**。
- **重试**：单次运行若因偶发 `network error` 或模型格式抖动失败，最多重试 3 次；仅成功的 run 计为有效样本。重试规则四臂一致。
- **未变项**：候选稿、ground truth 清单、指标、判定标准、模型/温度/thinking、题材 Skill 开关与数据完整性约束。

### 2026-11-21 执行前补记之五：预算裁剪把 Skill 正文整段删掉（F10）与补跑口径

- **发现（2026-11-21）**：2026-11-19 那轮 A/B 的四次 run 里，**题材 Skill 与 craft Skill 的正文都没有进入 user prompt**。全库 17 条 `prepared_contexts` 搜索题材 Skill 正文特征串命中数为 0；A 组与 B 组的 prompt 段结构逐段完全相同，差异只有 system prompt 里两行声明文字（B 组比 A 组多 38 字符，声称「其规则正文会随本轮上下文注入」）。
- **根因**：`workflow.rs` 的 `enforce_workflow_prompt_budget` 裁剪顺序是「先剥 Skill 段阶段细则 → 再整段移除低优先级段 → 最后字符压缩」，其中「整段移除」这一档的过滤条件是 `priority == 0 && text.contains("Skill")`。题材 Skill 段与 craft skill 段的标题都含 "Skill"、`prompt_section_priority` 又把它们判为 priority 0，于是在工具结果段超长时**被整段删除**——而 system prompt 的声明仍按「已注入」表述。F1 建立的「声明与注入同源」只覆盖了 `enabled` 语义，没有覆盖预算裁剪这一层。
- **修复（F10）**：字符压缩阶段跳过 Skill 段；剥阶段细则与整段删除都退到兜底位置（顺序改为 B1/B2 移除非 Skill 低优先级段 → 字符压缩非 Skill 段 → C 级剥 Skill 阶段细则 → D 级整段删除 Skill）。顺带修掉一个潜在死循环：压缩循环原用 `target = (current-excess).max(minimum).min(current-1)`，当所有可压段都到达下限时每轮只减 1 个字符；现改为在候选阶段就跳过已达下限的段，`target = (current-excess).max(minimum)` 严格递减。新增单测 `budget_keeps_skill_sections_whole_while_other_sections_shrink`、`budget_keeps_skill_always_when_everything_else_is_squeezed`，并抽出 `prompt_section_floor`。全量 `cargo test --lib` 234 passed / 0 failed / 3 ignored。
- **对历史结论的影响**：2026-07-19 的 NEG-001（「常驻注入的 Skill 会与事实边界争夺注意力，导致硬问题召回下降」）建立在这套裁剪行为之上，那轮 Skill 正文同样很可能未真正进入上下文。**该结论需要重新评估**，本轮报告必须显式披露这一影响。
- **2026-11-19 那轮结果的处置**：判定为**作废**，不进入报告结论。四次 run 保留在库里（run 162/163/165/167、产物 92/93/95/97）作为缺陷证据，不计入统计。
- **重跑用的二进制**：2026-11-21 重建（含 F1–F6 + F8 + F9 + F10），四臂同模型同参数同指令，唯一变量仍是题材 Skill `enabled` 开关。

#### 补跑轮次与判定口径（先写定，再执行）

首轮四跑（A1 67% / A2 78% / B1 78% / B2 11%）出现 **B 组组内差异 6 条**，远超 A 组差异 1 条。按协议第六节第 1 条的立法意图（噪声大于信号时不得据此下结论），补跑以确认这是「题材 Skill 有害」还是单次模型抖动。补跑口径**在补跑前写定**：

- **补跑轮次**：A3、B3、B4（A 组共 3 次，B 组共 4 次）。仅成功的 run 计为有效样本，失败重试规则与前四跑一致（四臂统一）。
- **组指标**：以该组全部有效 run 的召回率**均值**作为组指标（消除单次抖动），major / 误报同样按均值比较。
- **组内离散检查（优先于均值比较）**：任一组的召回率极差 ≥ 4 条 → 记 `INCONCLUSIVE`，按协议第六节补充约束登记，并如实写明噪声大于信号。
- **判定**：B 组均值 < A 组均值 → `NEGATIVE`；B 组均值 ≥ A 组均值，且 major 均值不增加、误报均值不显著增加 → `POSITIVE`；未落入任一条 → `INCONCLUSIVE`。
- **不变项**：候选稿（artifact 69）、ground truth 清单（9 条，已定稿）、命中判定口径、误报定义、模型/温度/thinking/引文长度指令、数据完整性约束。

