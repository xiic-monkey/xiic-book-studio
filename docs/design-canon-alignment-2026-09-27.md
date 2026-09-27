# 设计稿：章节转正后的资料对齐机制（canon alignment）

拟定日期：2026-09-27
状态：待评审
背景：canon_entries 是生成时快照——正文演进（新修订转正）后资料不更新、不删除、不标记，导致重复堆积与"卡比正文旧"的矛盾（实例：录音带外观三卡互相矛盾、废弃观测站卡与正文运营中设定冲突）。章节记忆已有"哈希失效 + 惰性重建"的成熟先例，本设计把同样的纪律延伸到资料库。

## 设计原则

1. **对齐是提议，不是覆盖**：对齐 Agent 只产出变更，全部以 `pending_human_approval` 落地，人工在资料库确认（复用现有"待确认"徽标与采用按钮）。唯一自动动作是"标记过期"（置 archived，可人工恢复）。
2. **不阻塞转正**：对齐异步执行（复用 approve 后 spawn 章节记忆的挂点），失败不影响采纳。
3. **成本有界**：确定性低参设置（对齐 continuity_ledger agent 模式）；只对齐**来源为本次转正章节**的条目；无相关条目则跳过。
4. **复用现有状态机**：不新增状态值，只用 pending_human_approval / approved / archived。

## 触发链

```
approve_stage（正文转正）
  ├─ 现有：入队索引任务 → 唤醒 worker
  ├─ 现有：spawn 章节记忆重建
  └─ 新增：spawn canon_alignment::align_chapter(project_id, chapter_id)
```

跳过条件（满足其一即 return）：
- 该章节无 status != archived 的 canon_entries（`source_chapter_id` 关联）
- 该章正文哈希与上次对齐时一致（对齐标记存 `workflow_runs`：查最近一条 stage=`canon_align` 成功运行的时间与当时的正文哈希——首版可省略哈希比对，每次转正都跑一次小调用）

## 配套修复（评审后新增的三项）

1. **生成端防重（问题 1 的根因）**：原 `save_human_canon_entry` 是裸 INSERT——同项目同类别同标题会建重复行（实例：林默×2）。改为**同身份 upsert**（project_id + category + lower(title) 查到即 UPDATE），并在迁移 v10 中清理历史重复行 + 建唯一索引 `idx_canon_entries_identity` 兜底。
2. **上下文分区（问题 3）**：`append_approved_context` 拆成两段——`# 已确认资料（已写进正文的事实）`（source_chapter_id 非空）与 `# 预埋设定（尚未写入正文）`（source_chapter_id 为空），并明确告知模型"预埋设定不得当作已发生事实引用"。
3. **写作笔记提取（问题 4）**：对齐产出新增 `writing_notes`（角色说话方式、重复母题、编号规律），落为 category=rule 的资料条目（title 加"写作笔记："前缀），进资料库与续章上下文。

## 对齐流程（新模块 `canon_alignment.rs`，模板 = continuity_ledger.rs 的 ensure 模式）

1. `insert_workflow_run(stage = "canon_align", status = "running")`——后台运行留痕，失败可见（借鉴章节记忆的教训：不再静默 eprintln）。
2. 取材料：该章节的 canon_entries（含 content 与 status）+ 本次转正正文全文。
3. 一次 `complete_json_chat`（确定性设置），提示词要求输出严格 JSON：
```json
{
  "updates":  [{ "entry_id": 1, "reason": "正文 v2 中录音带外观已改为手写贴纸", "new_content": "..." }],
  "archive_ids": [{ "entry_id": 3, "reason": "正文明确定义标签为手写贴纸，'标签被撕去'描述已失效" }],
  "additions": [{ "category": "item", "title": "...", "content": "...", "reason": "正文新增的可复用设定" }]
}
```
   提示词硬约束：`new_content` 中引用正文的句子必须逐字摘自正文；updates 只改写与正文冲突或过时的段落；不为灵感性设定（尚未写进正文的预埋）出 update。
4. **应用（与章节记忆相同的 CAS/失效纪律）**：
   - `updates`：更新 content，**status 重置为 pending_human_approval**，`source_artifact_id` 指向新正文；
   - `archive_ids`：status → `archived`（唯一自动动作）；
   - `additions`：INSERT，status = `pending_human_approval`；
   - 全部走事务，转正后再入队项目搜索任务（复用 `enqueue_project_search_job`）。
5. `update_workflow_run(success/failed)`——失败时 run 行可见（工作区运行历史里可查），首版不做自动重试。

## 重复合并（MVP：只提示，不自动合并）

同一运行产出的近重复（实例：单次运行 5 分钟建 3 张录音带卡）是编辑决策，不做自动合并。首版做**检测与提示**：

- 位置：资料库 UI 渲染时，同 category 内两两比较（title 分词 Jaccard ≥ 0.6 或一方 title 是另一方子串）；
- 命中的卡片加"疑似重复"角标，点击展开另一张对比；
- 纯前端计算（42 条级别无性能问题），不动后端。

## 不做 / 后续

- 不自动合并、不自动删除（archived 可人工恢复，删除仍走 UI 彻底删除）
- 不做跨章全局对齐（首版只对齐触发章节；全局对齐等出现真实需求再评估）
- 不改 action_proposals 体系（章节修订提案不受影响）
- 角色六字段结构化投影（character_profiles）仍按独立排期推进，本机制与其正交

## 改动清单

| 文件 | 改动 |
|---|---|
| `canon_alignment.rs`（新） | align_chapter + 提示词 + 应用逻辑（约 200 行，模板 continuity_ledger.rs） |
| `application/use_cases.rs` | approve_stage 后追加 spawn（挂点同章节记忆，:332 旁） |
| `db.rs` | list_entries_for_chapter 辅助查询；updates/archive/additions 的应用方法 |
| `v2_storage.rs` | migration v10：seed 后台 agent `canon_align`（提示词入 prompts/v2/canon_align.md），仿 migrate_v8 |
| `prompt_templates.rs` | 注册 canon_align 默认提示词 key |
| `models.rs` + 契约 | AlignReport 类型（可选，供前端展示运行结果） |
| `BookStudioWorkspace.tsx` | 疑似重复角标（纯前端启发式） |
| 测试 | 对齐应用的 CAS 用例（正文变更后旧对齐结果被拒绝）；archive 不删数据断言 |

## 验收

1. 单元：updates 重置 pending、archive 保留可查、正文哈希变化拒绝旧对齐写入。
2. 集成：两章手测——转正第 2 章后，来源为第 1 章、且与正文冲突的条目被置 archived 或给出 update 提案，资料库出现待确认项。
3. 成本：每次转正固定一次小模型调用（确定性设置）。
