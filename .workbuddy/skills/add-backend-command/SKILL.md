---
name: add-backend-command
description: 'This skill should be used when adding a new backend Tauri command to the xiic-book-studio desktop app (xiic-book-studio / src-tauri), e.g. when the user wants a new capability exposed from Rust to the React frontend, a missing delete/update endpoint, or any "add an API / command" request in this repo. It captures the exact wiring chain (commands to use_cases to db), two non-obvious gotchas (non-V2 commands bypass the generated contracts so no contract regen is needed; knowledge/artifact deletions must also scrub the story_search tables), and the dual library render-branch caveat on the frontend.'
agent_created: true
---

# Add Backend Command (xiic-book-studio)

## Overview

Add a new backend command to the xiic-book-studio Tauri app. The backend is
layered: `commands.rs` (Tauri entry) → `application/use_cases.rs`
(`ApplicationGateway`) → `db.rs` (SQLite, live path for knowledge cards). The
generated TypeScript contract file (`src/generated/v2-contracts.ts`) is produced
by `generate_contracts.rs` from `contracts::V2_COMMANDS` — but **most user-facing
commands are NOT in that list** and are invoked directly, so adding one does not
require touching the contract generator.

## When to use

- User asks to "add a delete/update endpoint", "expose X from the backend", or
  any new backend capability reachable from the UI.
- A feature exists for the agent runtime (e.g. `v2_storage.rs` has a
  `*_delete` op) but is not wired to a user command.
- Fixing "I can create X but cannot delete/edit it from the UI".

## Workflow

1. **Decide the input struct.** Add a `pub struct XxxRequest { ... }` in
   `src-tauri/src/models.rs` (near a similar existing request, e.g.
   `DeleteArtifactRequest`). Derive `Debug, Clone, Serialize, Deserialize`.
2. **Implement the data layer** in `src-tauri/src/db.rs` as a method on `AppState`
   (mirror a sibling method such as `delete_artifact`). Wrap writes in
   `self.with_conn(|conn| { conn.execute_batch("BEGIN IMMEDIATE")?; ... COMMIT/ROLLBACK })`.
3. **Add the use-case method** in `src-tauri/src/application/use_cases.rs`
   (`impl ApplicationGateway`) — a thin delegate to `self.state`. Models are
   imported via `models::*` so new request types are in scope automatically.
4. **Add the Tauri command** in `src-tauri/src/commands.rs`: a `#[tauri::command]
   pub fn xxx(...)` that calls `gateway.xxx(input)`. Add the request type to the
   big `use crate::models::{...}` import block.
5. **Register the command** in `src-tauri/src/lib.rs`: add the fn name to BOTH
   the `use commands::{...}` import list and the `tauri::generate_handler![...]`
   macro list.
6. **Mirror in the dev server** `src-tauri/src/dev_server.rs`: add a
   `"xxx" => { let input: XxxRequest = read_required(&payload, "input")?;
   gateway.xxx(input)?; Ok(serde_json::to_value(())?) }` arm, and add the request
   type to its imports.
7. **Frontend types** in `src/types.ts`: add `export interface XxxInput { ... }`
   (snake_case field names, matching the Rust struct).
8. **Frontend api** in `src/api.ts`: import the input type and add a method
   `xxx: (input: XxxInput) => invokeCommand<void>("xxx", { input })` next to the
   sibling call (e.g. `deleteArtifact`).
9. **Frontend UI**: add the handler + button in
   `src/features/workbench/BookStudioWorkspace.tsx`.

## Gotchas (verify every time)

- **Contracts bypass:** `save_knowledge_card` / `delete_artifact` / etc. are
  direct Tauri commands, NOT entries in `contracts::V2_COMMANDS`. Therefore
  adding such a command does **not** require running `npm run contracts:generate`
  and `npm run contracts:check` will not break. Only the v2 agent-run commands
  (listed in `contracts.rs`) flow through the generated contract.
- **Search-index cleanup:** deleting a `knowledge_cards` row must also scrub
  `story_search_documents`, `story_search_embeddings` (via the
  `story_search_documents.id` rowid subquery, guarded by
  `table_exists(conn, "story_search_embeddings")`), and `story_search_sources`
  WHERE `source_kind = 'knowledge_card' AND source_id = ?`. Mirror
  `delete_artifact_search_data_tx` in `db.rs`. Skipping this leaves orphaned
  search hits.
- **Dual library render branches:** `BookStudioWorkspace.tsx` contains TWO
  parallel library render blocks (an old branch around line ~2715 gated by
  `libraryFocus` not in [characters,items,events], and the new branch around
  line ~3266). Both render `libraryCards`; if a control (edit/delete) is added to
  one, add it to the other too, or the user will see it missing on whichever
  surface they use. Manual `KnowledgeCard`s get `managed-knowledge-card` wrappers
  with edit/approve/archive/delete buttons; `librarySourceSections` (parsed from
  the generated artifact) are NOT individually deletable.
- **Verification:** run `cd src-tauri && cargo check` (fast if deps cached) and
  `./node_modules/.bin/tsc --noEmit` from the repo root. Do NOT run a full
  `cargo build` / `npm run contracts:check` during an in-progress refactor unless
  specifically needed — they pull in unrelated uncommitted changes.

## Reference

See `references/wiring-map.md` for the exact file/function map and a concrete
worked example (adding `delete_knowledge_card`).
