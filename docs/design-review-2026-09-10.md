# xiic-book-studio 设计评审

日期：2026-09-10
范围：Rust 34,377 行（37 个 .rs）+ TypeScript 8,671 行（24 个文件）
方法：静态审查 + 关键论断逐条回读源码验证

---

## 0. 结论摘要

项目工程质量在**基础设施层**（连接池 PRAGMA、外键 ON DELETE 行为、AES-GCM 密钥加密、AI 长任务取消/事件流、前后端契约代码生成）做得相当扎实，属于中上水平。

真正的问题集中在**领域建模层**：

1. 核心卖点（质量门禁）在架构上是"咨询式"的，可以完全绕过 —— 这是设计意图落空，不是 bug。
2. 三层架构退化为"薄壳 → 贫血转发 → db.rs 承担一切"。
3. 领域概念没有类型化（stage 用裸字符串），导致同一规则 11 处复制且集合不一致。
4. 契约生成机制被手写的 types.ts 绕开，形同虚设。

下面按严重度排列，每条均附已验证的 `file:line` 证据。

---

## 1. 【严重】质量门禁不参与审批决策，核心模型可被绕过

**证据**

- `gate.rs:13 analyze_chapter_gate` 计算质量分（`gate.rs:11 MIN_ACCEPTABLE_QUALITY = 72`）与连续性硬阻断，返回 `ChapterGateReport`。
- `db.rs:3025 approve_stage` 是唯一的审批入口，其全部校验仅为：
  - `db.rs:3033` `artifact.project_id != project_id || artifact.stage != stage`
  - `db.rs:3036` 章节归属校验
  - **全文不出现 `gate::` 调用**。
- 全仓 `gate::` 调用点只有 5 处（`lib.rs:33/129`、`dev_server.rs:409`、`commands.rs:422`、`application/use_cases.rs:390`），全部是"返回报告给前端"这条链，没有任何一处把报告结果作为审批的前置条件。
- `approvals` 表上有唯一索引 `idx_approvals_artifact_once`（`db.rs:4747`），保证一个产物只能审批一次 —— 唯一性约束做了，但门槛约束没做。

**影响**

项目的产品定义是"AI 生成候选产物 → 人工确认 → 正式章节正文"。但质量门禁与连续性阻断在架构上只是**给人类看的建议**，UI 上用户点"通过"即生效，无需门禁报告，甚至无需先发起门禁检查。72 分阈值、`GateBlocker`、连续性 `verdict` 全部失去强制力。

这不是实现疏漏，而是"检查"与"决策"两条路径从未在架构上汇合。

**建议方向**

在 `approve_stage` 内显式区分两种模式，二选一：

- **硬门禁**：审批前必须存在通过状态的门禁报告（可按 `source_text_hash` 失效），否则拒绝并附最新报告。
- **软门禁（记录型）**：允许通过，但把门禁结论写入 approvals 表（新增 `gate_verdict` / `gate_score` 列），支持事后审计与统计。

若选软门禁，需在 UI 上明确提示"未通过检查"，否则功能等同不存在。

---

## 2. 【严重】分层退化：三层架构实际是两层半

**证据**

- `commands.rs` 共 **73** 个 `#[tauri::command]`，无一直接写 SQL，全部转发给 `ApplicationGateway` —— 命令层合规。
- `application/use_cases.rs` 是贫血转发层：`create_project`(:11) → `state.create_project`、`update_chapter`(:62) → `state.update_chapter`、`analyze_chapter_gate`(:390) → `gate::analyze_chapter_gate`。
- 真正的业务规则在 `db.rs` 的 `impl AppState` 里：
  - `db.rs:3025 approve_stage` —— 审批状态机 + 缓存失效 + 索引入队，共约 100 行
  - `db.rs:2331 clear_chapter_history` —— 选择性删除策略
  - `db.rs:1618 ensure_chapter` —— 归属校验（被 gate/workflow/continuity 多处复用）
- 规模佐证：`db.rs` 6473 行、`workflow.rs` 5440 行，两者占 Rust 总量 35%。

**影响**

领域逻辑与 `rusqlite::Connection` 强耦合，无法脱离数据库单测 —— 这也直接解释了为什么 34k 行 Rust 只有模块内 `#[cfg(test)]` 单测、`src-tauri/tests/` 零集成测试：最有价值的逻辑测不了。

**建议方向**

不必大重构。先把"不依赖 Connection 的纯规则"抽出来：阶段合法性、门禁判定、正文计数口径。这些抽成纯函数后可立即单测，且不需要动调用方。

---

## 3. 【严重】领域概念未类型化，同一规则 11 处复制且集合不一致

**证据**

`stage` 全程是 `&str`，`models.rs` 1541 行中没有 `enum Stage`。于是"哪些阶段可编辑"这条规则被复制了 11 次：

| 位置 | 允许的阶段 |
|---|---|
| `gate.rs:23` | draft, revision |
| `continuity_ledger.rs:237` | draft, revision |
| `workflow.rs:753 / 1547 / 3173 / 3912` | draft, revision |
| `v2_storage.rs:1197` | draft, revision |
| `tool_runtime.rs:790` | draft, revision |
| `application/use_cases.rs:280` | draft, revision |
| `db.rs:3036 / 3076` | draft, revision |
| **`workflow.rs:271`** | **draft, revision, review** |
| **`agent_run_service.rs:917 / 1091`** | **draft, revision, review** |

同一概念存在两套真相。类似地，`db.rs:1808 / 1913 / 2114` 三处复制同一段 SQL 片段；`db.rs:3057` 与 `3114` 重复同一 approvals 查询；`workflow.rs:1066` 与 `1215` 重复阶段列表、`1123` 与 `1327` 重复状态映射。

**影响**

改一处漏一处是必然的。`review` 阶段在部分路径可编辑、部分路径不可编辑，行为取决于用户点了哪个按钮 —— 这类 bug 极难复现和定位。

**建议方向**

建立 `enum Stage`（`FromStr`/`Display` 实现），配套 `fn is_editable(&self) -> bool` 与 `fn next(&self) -> Option<Stage>`，替换全部 11 处。这是投入产出比最高的一条 —— 半天工作量，消灭一整类 bug。

---

## 4. 【高】契约生成机制被绕过

**证据**

- `src/generated/v2-contracts.ts` 121 行（标注 @generated，由 `bin/generate_contracts.rs` 产出）。
- `src/types.ts` **745 行**手写类型，其中 `Stage`(:25)、`Chapter`(:182)、`Artifact`(:200) 等与生成类型同名重复定义；`DerivedIndexJob` 在 `types.ts:431` 与 `v2-contracts.ts:46` 各一份。
- `api.ts:276` 用生成别名 `V2DerivedIndexJob`，其余用 `./types` 的 `DerivedIndexJob` —— 同一后端实体两种 TS 形状。
- 命令名：`api.ts` 中仅 18 处走 `V2_COMMANDS` 常量，其余约 60 个命令名硬编码字符串。
- `npm run contracts:check`（`package.json:13`）只校验生成文件与 Rust 一致，**不校验手写的 types.ts**。

**影响**

Rust 侧改字段名或命令名时，TS 编译不报错，运行时静默失败。契约生成投入的收益被抵消。

**建议方向**

`types.ts` 改为纯 re-export，禁止重复定义；加一条 lint/CI 规则断言 `types.ts` 无 `interface`/`type` 声明。命令名全量收进 `V2_COMMANDS`。

---

## 5. 【高】前端 God Component，react-query 未发挥作用

**证据**（`src/features/workbench/BookStudioWorkspace.tsx`）

- **4585 行**，**77 个 `useState`**，22 个 `useEffect`。
- `useQuery` 仅 **2** 个，`useMutation` **0** 个 —— 所有写操作是 handler 内 `await api.x()` + 手动 `setQueryData`（:992）。
- 唯一的 `invalidateQueries` 在 :1292，写成 `refetchType: "none"` —— 标记失效但不重取，等于无效。
- 现有 hooks 是假拆分：`useProjectWorkspace.ts` 16 行、`useArtifact.ts` 18 行，都只是 `useQuery` 薄壳。
- 无 toast / 错误上报，`console.error` 计数为 0。

**影响**

4585 行单组件承载项目列表、章节树、AI 事件流、产物对比采纳、知识卡、伏笔、设定、连续性库 —— 任何改动都可能回归。缓存手动维护多组件间易不一致。

**建议方向**

按"AI 运行 / 产物对比采纳 / 知识库"三个容器组件拆，每个自带 hook 与 `useMutation`。先把 `refetchType: "none"` 删掉。

---

## 6. 【中】嵌入模型在增量路径每章重新加载（热路径反而更贵）

**证据**

- `story_search.rs:493 replace_source_with_runtime` 内 `EmbeddingRuntime::load(state)` —— 增量索引是热路径，每章调用一次。
- `story_search.rs:86 rebuild_story_search_index` 只 load 一次并复用 —— 全量重建反而是冷路径且已优化。
- `EmbeddingRuntime::load`(:1359) 每次读取 `pytorch_model.bin` + `tokenizer.json`，`verify_model_package`(:1450) 对大模型文件做 SHA256 校验。

**影响**

N 章 = N 次模型加载 + N 次全文件 SHA256。初始索引/批量重索引的绝对瓶颈，M5 上每次加载还有可观的 RSS 抖动。

**建议方向**

runtime 缓存到 `AppState`（`OnceCell` 或按 model 版本的 `Arc`），仅在模型版本变化时重建。这是本项目性价比最高的一处性能优化。

---

## 7. 【中】主库无版本化迁移

**证据**

- `db.rs:213-240` 起的建表全部 `CREATE TABLE IF NOT EXISTS`，**无 `user_version`、无迁移记录表**（`user_version` 仅在测试 `db.rs:6363` 出现）。
- 反观 `v2_storage.rs:17-65` 有完整的 `v2_schema_migrations`（已到 v8），`apply_migration` 正确 COMMIT/ROLLBACK(:164/168)。

**影响**

承载 82 处外键的核心库只能追加建表，无法安全 `ALTER TABLE`。后续任何字段调整都要靠"建新表 + 搬迁"或裸 ALTER，schema 漂移风险高。两套 schema 治理方式并存也增加认知负担。

**建议方向**

主库接入与 v2 相同的版本化迁移 runner。

---

## 8. 【中】向量检索无 project 分区

**证据**

- `story_search.rs:1215` 向量表仅 `embedding float[512]`，无 `project_id`。
- `vector_candidates`(:1010-1014) `SELECT rowid ... WHERE embedding MATCH ?1 ORDER BY distance LIMIT 240` 全项目扫描，再 :1021 按 id 回表过滤项目。
- 删除文档需手写 `DELETE embeddings`（:758/826/860/887 四处），易遗漏成孤儿向量。

**影响**

多项目时 top-k 被其他项目占坑，本项目召回下降；跨项目扫描无谓开销。

---

## 9. 【中】其他

- **错误类型过粗**：`error.rs` 仅 26 行、5 个变体，所有业务错误压成 `Validation(String)`（如 `db.rs:3034`）。前端无法按类型处理，只能匹配中文字符串。
- **生产路径 panic**：`agent_run_service.rs:1340 TcpListener::bind(...).await.unwrap()`、`1348 axum::serve(...).await.unwrap()` —— 内嵌服务器启动失败直接 panic，连带整个 agent-run 功能挂掉。
- **索引 worker 单并发**：`index_jobs.rs:39-57` 仅一个 async 循环串行处理，几千章时重索引极慢。
- **JSON 存 TEXT 列**：`v2_storage.rs:192/193/211/212/233`、`db.rs:239 enabled_tool_keys`，ID 列表无引用完整性、不可索引。
- **大文本无虚拟化**：正文用 `<pre>{content}</pre>`（:3294），无虚拟滚动依赖。
- **每次取连接都重载 sqlite-vec 扩展**：`db.rs:160`，6 连接池每次取出都执行，热点路径放大延迟。

---

## 10. 【中】测试覆盖与风险错配

- 前端仅 2 个测试文件，且都是纯函数（`diff.test.ts`、`KnowledgeSectionCard.test.ts`）。
- `src-tauri/tests/` 零集成测试；后端靠模块内 `#[cfg(test)]`（约 169 个）。

**最该测却完全没测的**：

1. 审批状态机 `approve_stage`（第 1、2 条问题的所在地）
2. 采纳流程 `applyAdoptionProposals` / `AdoptionDrawer`（核心业务）
3. `types.ts` 与 Rust 契约一致性（无脚本，第 4 条）
4. 阶段流转规则（第 3 条，11 处复制的规则最需要测试锁定）

---

## 11. 做得好的地方（不要动）

- 连接池 PRAGMA 配置正确：`db.rs:100-105` WAL + `busy_timeout 5s` + `synchronous NORMAL` + `foreign_keys ON`。
- `with_conn` 闭包是同步 `FnOnce(&Connection)`，**没有把 DB 锁跨 await 持有**；AI 调用（`story_index.rs:165`）不在事务内 —— 无长事务持锁反模式。
- 外键全部显式 `ON DELETE CASCADE/SET NULL`（`db.rs:253-711`），无默认 NO ACTION 悬空引用。
- API Key AES-256-GCM 字段级加密 + 设备密钥外置 `device.key`。
- AI 长任务有取消（`cancelAgentRun`）+ 流式事件；`setInterval` 正确清理无泄漏。
- 无 XSS（全文 0 处 `dangerouslySetInnerHTML`，AI 内容以文本节点渲染）；`api.ts` 无 `any` / `@ts-ignore`。

---

## 建议修复顺序

| 顺序 | 事项 | 理由 |
|---|---|---|
| 1 | `enum Stage` + 统一 11 处规则 | 半天工作量，消灭一整类 bug，且是第 1 条的前置 |
| 2 | 明确门禁语义（硬/软）并在 `approve_stage` 落地 | 核心卖点是否成立 |
| 3 | 嵌入 runtime 缓存 | 一处改动，索引性能数量级提升 |
| 4 | `types.ts` 收敛为 re-export + 命令名常量化 | 防止改名静默失败 |
| 5 | 补审批状态机与采纳流程的测试 | 锁住前两条的修复 |
| 6 | 前端按容器组件拆分 + `useMutation` | 长期可维护性 |
