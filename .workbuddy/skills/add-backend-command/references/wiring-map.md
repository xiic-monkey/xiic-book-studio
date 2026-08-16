# Wiring Map: adding a backend Tauri command

## Layers (live path for knowledge cards)

| Layer | File | Responsibility |
|-------|------|----------------|
| Tauri entry | `src-tauri/src/commands.rs` | `#[tauri::command] fn`, input struct from `models` |
| App gateway | `src-tauri/src/application/use_cases.rs` | thin delegate `self.state.xxx(...)` |
| Data | `src-tauri/src/db.rs` | `AppState` method, SQLite + search cleanup |
| Models | `src-tauri/src/models.rs` | request/response structs |
| Dev server | `src-tauri/src/dev_server.rs` | JSON-rpc-style dispatch arm |
| Registration | `src-tauri/src/lib.rs` | import + `generate_handler!` |
| Frontend types | `src/types.ts` | `XxxInput` interface (snake_case) |
| Frontend api | `src/api.ts` | `invokeCommand<void>("xxx", { input })` |
| Frontend UI | `src/features/workbench/BookStudioWorkspace.tsx` | handler + button |

## Worked example: `delete_knowledge_card`

**models.rs** (after `DeleteArtifactRequest`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteKnowledgeCardRequest {
    pub project_id: i64,
    pub card_id: i64,
}
```

**db.rs** (`AppState` impl):
```rust
pub fn delete_knowledge_card(&self, project_id: i64, card_id: i64) -> AppResult<()> {
    self.with_conn(|conn| {
        crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, conn)?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if table_exists(conn, "story_search_embeddings")? {
                conn.execute(
                    "DELETE FROM story_search_embeddings
                     WHERE rowid IN (
                         SELECT id FROM story_search_documents
                         WHERE project_id = ?1
                           AND source_kind = 'knowledge_card' AND source_id = ?2
                     )",
                    params![project_id, card_id],
                )?;
            }
            conn.execute(
                "DELETE FROM story_search_documents
                 WHERE project_id = ?1 AND source_kind = 'knowledge_card' AND source_id = ?2",
                params![project_id, card_id],
            )?;
            conn.execute(
                "DELETE FROM story_search_sources
                 WHERE project_id = ?1 AND source_kind = 'knowledge_card' AND source_id = ?2",
                params![project_id, card_id],
            )?;
            let deleted = conn.execute(
                "DELETE FROM knowledge_cards WHERE id = ?1 AND project_id = ?2",
                params![card_id, project_id],
            )?;
            if deleted == 0 {
                return Err(AppError::Validation("知识卡不存在或不属于当前项目".to_string()));
            }
            Ok(())
        })();
        match result {
            Ok(()) => { conn.execute_batch("COMMIT")?; Ok(()) }
            Err(error) => { let _ = conn.execute_batch("ROLLBACK"); Err(error) }
        }
    })
}
```

**use_cases.rs**:
```rust
pub fn delete_knowledge_card(&self, input: DeleteKnowledgeCardRequest) -> AppResult<()> {
    self.state.delete_knowledge_card(input.project_id, input.card_id)
}
```

**commands.rs**: add `DeleteKnowledgeCardRequest` to the `use crate::models::{...}`
import, then:
```rust
#[tauri::command]
pub fn delete_knowledge_card(
    gateway: State<'_, ApplicationGateway>,
    input: DeleteKnowledgeCardRequest,
) -> AppResult<()> {
    gateway.delete_knowledge_card(input)
}
```

**lib.rs**: add `delete_knowledge_card,` to the `use commands::{...}` list AND to
`tauri::generate_handler![...]`.

**dev_server.rs**: add `DeleteKnowledgeCardRequest` to imports, then:
```rust
"delete_knowledge_card" => {
    let input: DeleteKnowledgeCardRequest = read_required(&payload, "input")?;
    gateway.delete_knowledge_card(input)?;
    Ok(serde_json::to_value(())?)
}
```

**types.ts**:
```ts
export interface DeleteKnowledgeCardInput {
  project_id: number;
  card_id: number;
}
```

**api.ts**:
```ts
deleteKnowledgeCard: (input: DeleteKnowledgeCardInput) =>
  invokeCommand<void>("delete_knowledge_card", { input }),
```

**BookStudioWorkspace.tsx** — handler + button (add to BOTH library render
branches):
```tsx
async function deleteKnowledgeCard(card: KnowledgeCard) {
  if (!detail) return;
  if (!window.confirm(`确定删除资料卡"${card.title}"吗？\n该资料卡会彻底移除，不可恢复。`)) return;
  await runTask("删除资料卡", async () => {
    await api.deleteKnowledgeCard({ project_id: detail.project.id, card_id: card.id });
    if (editingKnowledgeCardId === card.id) resetKnowledgeComposer();
    await refreshDetailBestEffort(detail.project.id, "资料卡删除");
    setNotice(`已删除资料卡 ${card.title}`);
  });
}
```
Button: `<button className="icon-btn danger" onClick={() => deleteKnowledgeCard(card)} title="彻底删除资料卡"><Trash2 size={14} /></button>`
(danger style: `.icon-btn.danger { color:#d23f3f; border-color:#f0c4c4 } .icon-btn.danger:hover { background:#fdecec }`).

## Notes

- `v2_storage.rs` already contains a `knowledge_card_delete` op used only by the
  agent tool runtime (`tool_runtime.rs` → `PROPOSE_DELETE_KNOWLEDGE_CARD`). The
  user command above goes through `db.rs`, which is the live path for manual
  cards (same path as `save_knowledge_card` → `adoption::save_human_knowledge_card`).
- No change to `generate_contracts.rs` / `v2-contracts.ts` is required because
  `delete_knowledge_card` is a direct command, not in `V2_COMMANDS`.
