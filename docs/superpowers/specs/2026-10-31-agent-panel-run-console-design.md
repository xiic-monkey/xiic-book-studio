# 右侧 Agent 面板重设计：对话式 Run Console

日期：2026-10-31
基线：`main`（工作区含 2026-09 以来的 Agent 面板未提交改动）
范围：`src/features/assistant-panel/`（新建）、`src/features/workbench/BookStudioWorkspace.tsx`、`src/features/agent-runs/`、`src-tauri/src/{models,agent_run_service,v2_storage,dev_server}.rs`、`src/generated/v2-contracts.ts`（生成）、`src/styles.css`
目标读者：实现者。本文只定义契约与边界，不含实施步骤（实施计划另见 writing-plans 产物）。

---

## 1. 目标与非目标

**目标**：让右侧面板从"聊天记录 + 隐藏调试抽屉"变成**可信的 Agent 执行台**——用户在不打开任何抽屉、不猜的前提下，能回答四个问题：

1. Agent 现在在做什么（阶段、第几个工具、第几个子任务）；
2. 它做了什么、依据是什么（工具参数与结果、耗时、失败原因）；
3. 它打算改什么（写类工具的影响、待确认提议，就地批准/拒绝）；
4. 主 Agent 把请求拆成了哪几件事、每件事到哪一步（计划与依赖）。

**非目标**（本期明确不做，见 §7 分期）：

- token / 成本用量展示（`ai.rs` 目前完全不读 provider `usage`，要先做响应解析 + `workflow_runs` 迁移，属独立变更）；
- 会话（session）分组与跨重启完整回放；
- 运行中追加指令的队列（后端有单活动运行守卫，需先做队列语义）；
- 把面板从三栏工作流里拆成独立控制台页/多标签。

## 2. 现状证据（缺口即需求）

下列行号指向改动前的基线，仅用于定位缺口来源。

**A 类｜数据已有、只是没显示（纯前端）**

| # | 缺口 | 证据 |
|---|---|---|
| A1 | 主时间线工具行不可展开；只有子会话视图传了 `sections`（参数/结果） | `BookStudioWorkspace.tsx:4634`（主）对比 `:4741`（子） |
| A2 | 工具行无耗时、无序号；成功态文案为空串 | `ConversationTimelineRows.tsx:215`；`elapsed_ms` 后端已给（`models.rs:1290`） |
| A3 | 编排层过程被整条丢弃：前端过滤 `stage === "orchestrator"`，而后端正是用这些事件播报委托进度与失败 | `BookStudioWorkspace.tsx:1561` 对比 `agent_run_service.rs:738`、`:796` |
| A4 | 执行状态条是硬编码 4 步假状态，且显示条件 `!orchestratorParentRunId` 导致编排运行时永不出现 | `BookStudioWorkspace.tsx:4771` |
| A5 | 待确认提议只在抽屉里的 `AgentRunInspector` 中，时间线无审批位 | `BookStudioWorkspace.tsx:4985` |

**B 类｜需要后端补契约**

| # | 缺口 | 证据 |
|---|---|---|
| B1 | 运行中拿不到工具载荷：`tool_started`/`tool_completed` 只带 key/status/summary/elapsed，参数与结果只在 `tool_invocations` 表，要等终态 `getAgentRun` 才到手 | `tool_runtime.rs:401`、`BookStudioWorkspace.tsx:2054` |
| B2 | 无计划事件：编排决策（tasks + `depends_on`）只活在 Rust 内存里，`RunEvent.kind` 13 种中没有计划/任务类事件 | `models.rs:1279` |
| B3 | `proposal_warning` 事件已发出但前端未消费 | `agent_run_service.rs:101` |

**C 类｜交互能力**

| # | 缺口 | 证据 |
|---|---|---|
| C1 | 只能全局停止（`cancelAgentRun` 支持任意 runId，前端只传 parent） | `BookStudioWorkspace.tsx:3221` |
| C2 | 子 Agent 是替换式子会话视图（面包屑），不是 harness 的原地嵌套展开 | `BookStudioWorkspace.tsx:4687` |
| C3 | 写类工具（`replace_text`/`apply_patch` 等）显示原始 JSON，无影响摘要、无跳转产物入口 | `toolLabels.ts:21` |

## 3. 方案选择

| 方案 | 内容 | 判断 |
|---|---|---|
| 1 前端可见性修补 | 只做 A 类：展开载荷、编排事件、内联审批 | 否。运行中仍看不到计划与任务边界，A3/A4 只能显示为散落的文本行，仍不像执行台 |
| **2 Run Console（采用）** | 一次运行升格为「计划 + 阶段 + 工具卡 + 产物/提议 + 状态」的一等模型；后端补 1 类事件（`plan_updated`）+ 懒取载荷；渲染层从 God 组件抽出为 `assistant-panel` 模块 | 用最小后端改动换最大可信度；A、B1、B2、B3、C1、C2、C3 全覆盖 |
| 3 独立控制台（多标签/可分离） | 面板变成 IDE 式终端 | 否。破坏三栏写作工作流，成本与收益不匹配 |

## 4. 架构与模块边界

```
BookStudioWorkspace（数据宿主：查询、订阅、run 生命周期动作）
  └─ <RunTimeline items={...} />                          ← src/features/assistant-panel/RunTimeline.tsx
       ├─ ReasoningRow / ToolRow / ToolGroupRow            ← src/features/agent-runs/ConversationTimelineRows.tsx
       ├─ PlanRow（任务 + 依赖 + 状态）
       ├─ SubagentRow → 展开即内嵌 <RunTimeline>（原地）
       ├─ ProposalRow（就地应用/拒绝）
       ├─ NarrationRow（委托播报 / 警示）
       └─ Composer（仍在工作区）
```

**边界规则**

- `assistant-panel/timeline.ts`：纯函数，无 React、无 IO。输入 `RunEvent[] + ToolInvocation[] + ActionProposal[]`，输出 `AssistantTimelineItem[]`。`buildAssistantTimeline` / `buildChildTimeline` / `groupReadToolItems` / `readToolKindLabel` / `toolGroupCountsLabel` / `attachPendingProposals` / `backfillTimelineFromSummary` / `findInvocation` / `toolMetaLabel` 从 `BookStudioWorkspace.tsx` 迁入此文件。
- `assistant-panel/RunTimeline.tsx`：唯一的时间线渲染器，主会话与子会话共用同一实现（`variant="main" | "child"`）；只依赖传入的 `items` 与 `summaries`，不 import `api`。
- `BookStudioWorkspace.tsx`：只负责取数与派发（queries、`subscribeRunEvents`、`startOrchestratorTurn`、`cancelAgentRun`、提案应用），不再包含时间线 JSX。面板渲染所需的一切经 props 传入，面板不得自行 `import api`。
- `ConversationTimelineRows.tsx` 留在 `agent-runs/`（行原语，`AgentRunInspector` 与 dev 预览页共用），`assistant-panel` 只做编排与新增卡型。
- 面板宽度、拖拽、滚动跟随仍归工作区（已有 ref 接线），本期不动。

## 5. 交互契约

### 5.1 编排事件不再被丢弃（A3/B2）

主时间线对 `stage === "orchestrator"` 的过滤改为 **按 kind 白名单**：

| kind | 处理 |
|---|---|
| `plan_updated`（新增） | 渲染/更新计划行（`AssistantPlanRow`） |
| `thinking_start` / `thinking_delta` / `thinking_end` | 照常渲染为思考行（含"正在理解你的请求…"） |
| `started` | 不产出文本行：编排层的 `started` 是父 run 的身份标识，对外呈现为子任务行 |
| `output_delta` 且 `status ∈ {running}` | 委托播报行（"任务 X 等待前置任务"、"无法启动 X：…"），用次要样式 |
| `output_delta` 且 `status ∈ {success,failed,cancelled}` | **不渲染**：该文本已由 `insert_message` 落库为 assistant 消息，避免重复 |
| `completed` / `failed` / `cancelled` | 更新运行终态；`failed` / `cancelled` 不产出时间线行，失败与取消由工作区的 `appendAssistantMessage` 落成一条 assistant 消息（复用既有通道，避免同一信息两条通道各说一遍） |

理由：终态编排文本走消息通道、过程文本走事件通道，两条通道不产生同一行的两次呈现。

### 5.2 `plan_updated` 事件（B2）

- 发出点：`run_orchestrator_turn` 解析决策后、启动子任务循环中每次状态跃迁后（启动成功/启动失败/依赖失败跳过）各发一次。
- 载荷：`delta` = 紧凑 JSON，`status` = `"running"`。

```json
{"tasks":[{"index":0,"title":"重写第三章结尾","task_type":"revision","chapter_id":3,
           "depends_on":[],"status":"running"}]}
```

`status` 取值：`pending | running | success | failed | skipped | cancelled`。
- 不变量：`index` 与决策数组下标一致且稳定，`depends_on` 用同一 `index` 空间。
- 载荷是整体快照而非增量：前端用最新一条替换计划行，事件乱序不会让某个任务永久停在 `pending`。
- 前端以 `run_id` + 最新 `sequence` 的快照整体替换计划行状态，不做增量合并。
- 子运行 `started`/终态事件仍保留，作为计划行状态的交叉校验（事件乱序时以 `plan_updated` 为准）。
- `RunEvent.kind` 联合类型加入 `"plan_updated"`，`src/generated/v2-contracts.ts` 由 `npm run contracts:generate` 再生成，禁止手改。

### 5.3 工具载荷懒取（B1/A1）

- 展开工具行时按 `run_id` 懒调 `api.getAgentRun(run_id)`，同 run 的展开不重复请求。
- 工具事件与 invocation 的关联键：优先 `tool_invocation_id`，缺失时按 `(run_id, tool_key, 出现次序)` 匹配（沿用 `toolInvocationEvents` 的既有兜底）。
- 理由：不把 `arguments/result` 塞进广播事件（体积与协议稳定性），且终态数据以 DB 为权威。
- 工具行展开区两段：`参数`（JSON pretty）与 `结果/错误`；额外一行元信息 `#序号 · 耗时 ms · 协议`。
- 写类工具的影响摘要**不在前端推导**：后端 `tool_runtime.rs` 的 `tool_result_summary` 已经把工具结果压成一行人类可读文本（错误原文，或 `summary`/`message`/`description`，或"返回 N 条结果"），随 `tool_completed` 事件的 `delta` 下发；前端只做一件事——过滤掉"已返回结构化结果"这类零信息占位（`GENERIC_TOOL_SUMMARY`）。需要人工拍板的写入走 §5.5 的锚定提议，而不是再推一份影响摘要。
- 载荷缓存键为 `run_id`，缓存放在工作区的 `runSummariesById`（`Record<runId, AgentRunSummary>`），与"终态时顺手缓存一次"共用同一份，避免两套缓存。

### 5.4 子任务原地嵌套（C2）

- `SubagentRow` 的点击行为改为**展开/收起内嵌 `<RunTimeline>`**，展示该子 run 的思考与工具行（含懒取载荷）与耗时。
- 展开区右上角保留"单独查看"图标按钮，进入现有面包屑全宽子会话视图（该代码路径不动，作为深度阅读入口）。
- 嵌套只一层：子 run 自身再委托时，其 `started` 在子层渲染为同一形状的展开行（依赖 `parent_run_id` 链，不做无限递归容器）。

### 5.5 就地审批与错误动作（A5/B3/C1）

- `proposal_warning`：在时间线当前位置渲染警示行（正文为后端 `delta`）。
- 终态后 `AgentRunSummary.proposals` 中的 pending 提议**锚定在产生它的 run 的最后一行之后**（`attachPendingProposals`，按 `source_run_id` 归位，`order` 取宿主行 `+0.5` 保证重排后仍贴附），就地渲染类型 + 摘要 + 采用/放弃按钮，按钮走工作区已有的 `applyAgentProposal` / `rejectAgentProposal`。未挂在任何可见行上的提议（`source_run_id` 为空）不出现在时间线里，由抽屉与待确认计数兜底。`AgentRunInspector` 保留（更底层的调试视图），抽屉默认收起。
- 工具行状态为 `failed`/`rejected` 时原位展开错误，附一句"下一步"（重试该子任务 = 停止后重发；不引入后端单工具重试语义）。
- 运行中：每个 `running` 子任务行尾出现停止按钮，调 `api.cancelAgentRun(childRunId)`；全局停止按钮语义不变（停 parent，级联取消 children，已由 `wait_for_children_and_summarize` 与取消检查保证）。

### 5.6 假状态条退场（A4）

删除 `:4771` 的 4 步硬编码块（`准备 / 工具 / 生成结果 / 完成`）与它对应的 `workflowStepsCollapsed` 状态：那四步是前端编的，与真实运行无关，"工具"步骤还依赖已被废弃的 `liveToolEvents`，展示条件 `!orchestratorParentRunId` 又让它在编排运行时永不出现。

真实阶段信息改由时间线本身承载：

- **计划行**（`plan_updated` 快照）：任务清单、`depends_on`、每个任务的 `pending/running/success/failed/skipped/cancelled` 状态与进度文案（"2/3 完成 · 1 异常"）；
- **工具行**：每一步工具调用的成功/失败与耗时；
- **播报行**：编排层的委托说明与警示；
- **子任务行**：子 run 的终态（"已完成 / 失败 / 已停止 / 执行中"）；
- **子会话面包屑**：选中子任务时给出 `运行 #id · 状态 · 耗时 · N 次工具调用`。

不另做独立的 RunHeader 组件：面板头部已有身份与操作区，再叠一层状态栏会与时间线里的状态重复。

## 6. 错误处理与不变量

1. **事件乱序/缺口**：`listRunEvents` 拉到的历史与 SSE 增量按 `run_id:sequence` 去重（沿用 `mergeRunEvents`）；同一 run 内以 `sequence` 定序，跨 run（编排与子任务、持久化消息与事件）以 `created_at` 派生的 `order` 混排——两套序并存是现状，本期不改变其语义。
2. **载荷取不到**：`getAgentRun` 失败时工具行显示"参数与结果暂不可读"，收起态摘要仍可用；不得因为懒取失败而让行消失。
3. **未知 kind / 未知 tool_key**：兜底渲染为通用行（label 走 `toolLabels` 去下划线兜底），绝不抛错、绝不清空时间线。
4. **计划快照异常**（JSON 解析失败）：丢弃该事件并记一次 console 警告，计划行退回"由子任务行推导"的状态。
5. **章节切换**：现有 `chapter_id` 过滤保持（`BookStudioWorkspace.tsx` 的 `visibleAssistantTimelineEvents`）；计划行与工具行同受该过滤约束，避免跨章串台。
6. **取消竞态**：`cancellation_requested` 立即把子任务行切成"正在停止"，终态事件到达后才复位（现逻辑已如此，迁移进纯函数时不得丢掉）。

## 7. 分期与交付状态

| 期 | 内容 | 状态 |
|---|---|---|
| P0 | 模块抽出 + A1–A5 + B1（懒取）+ B3 + C1–C3 | 已交付 |
| P1 | `plan_updated` 事件 + kind 联合类型 + 契约生成 + Rust 测试 | 已交付 |
| P2 | provider `usage` 解析与 `workflow_runs` 迁移、session 分组与完整回放、运行中指令队列 | 未开始，另立设计稿 |

**P0/P1 落地清单**

- 纯逻辑层 `src/features/assistant-panel/timeline.ts`：编排 kind 白名单、`plan_updated` 快照解析、提议锚定、子 run 状态推导、读类工具分组、事件合并与排序。
- 渲染层 `src/features/assistant-panel/RunTimeline.tsx`：主时间线与子会话共用同一渲染器（`variant="main" | "child"`），行原语仍在 `src/features/agent-runs/ConversationTimelineRows.tsx`。
- 数据宿主 `src/features/workbench/BookStudioWorkspace.tsx`：只做取数与派发（`api.getAgentRun` 懒取、提议应用/放弃、子 run 停止），不再持有时间线 JSX。
- 后端 `src-tauri/src/models.rs`：`RunEvent.kind` 联合类型加入 `"plan_updated"`；`src-tauri/src/agent_run_service.rs` 在决策解析与任务状态跃迁时发出该事件；`src/generated/v2-contracts.ts` 由 `npm run contracts:generate` 重新生成。
- 删除物：假 4 步状态条及其 `workflowStepsCollapsed` 状态、`liveToolEvents` / `timelineToolStatus` / `assistantToolTimeline` / `toolInvocationEvents` 一组派生状态、两处重复 feed 渲染链、`.assistant-workflow-panel*` 与 `.assistant-workflow-todo*` 样式。

## 8. 测试策略与实测结果

- **纯函数**（`src/features/assistant-panel/timeline.test.ts`，26 个用例）：编排白名单各分支、终态 `output_delta` 去重、`plan_updated` 替换语义与畸形载荷丢弃、未知 kind 兜底、提议锚定、子 run 状态推导、`groupReadToolItems` 既有行为不回退、`buildChildTimeline` 剔除子 run 自身 `started`（避免子任务行重复出现）。
- **前端整体**（`npm run test`）：50/50 通过。
- **类型与契约**：`npx tsc --noEmit` 无错；`npm run contracts:check` 通过。
- **Rust**（`cargo test --lib`）：209 通过、2 忽略。
- **浏览器**：`cargo run --bin dev_api` + `npm run dev:web`，对真实库数据走查一次委托运行（项目 6，运行 #137，14 次 `create_knowledge_card` 调用，耗时 141,830 ms）：主时间线出现编排播报行与子任务行；子任务原地展开后出现 17 条嵌套行（思考 + 14 条工具行 + 思考）；点击工具行懒取载荷，显示 `#1 · 1 ms · native` 与"参数 / 结果"两段 JSON；"单独查看"进入面包屑 `运行 #137 · 已完成 · 141,830 ms · 14 次工具调用`，可退回主时间线。该运行早于 `plan_updated` 特性，计划行不出现、退化为子任务行（§9 的兜底路径）。控制台无错误。300px 与 360px 两档宽度下无意外横向溢出（子任务标题省略、流式预览渐隐为刻意设计）。
- **视觉迭代**：`timeline-preview.html` 覆盖计划行（1/3 完成 + 依赖与状态字形）、播报行（普通 + 警示两种）、提议行（确认采用 / 放弃）、可展开子任务行（已完成 / 执行中）、工具组行、失败工具行与写类工具的参数 JSON。
- **回归**：重选书籍、切换章节、子会话面包屑进入/退出、停止后重发，均已人工走查。

## 9. 风险

| 风险 | 缓解 |
|---|---|
| 从 5354 行工作区抽面板时丢掉滚动/订阅接线 | 只搬 JSX 与纯函数，state 与 ref 留原位；搬迁前后逐分支对照（`assistantFeedItems`、`selectedSubagentProcess` 行为不变） |
| 放开编排事件后出现重复行（编排文本 vs 持久化消息） | §5.1 的 status 白名单 + 既有 `timelineOutputs` 去重集合 |
| 懒取 `getAgentRun` 放大请求量 | 按 run 缓存 + 仅在展开时请求；子会话视图已有同类请求，复用缓存字典 |
| 新增事件 kind 破坏旧前端/旧数据 | 未知 kind 兜底渲染（§6.3），旧 `workflow_runs` 无 `plan_updated` 时计划行不出现，退化为子任务行 |
