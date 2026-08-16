use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;

use crate::{
    db::AppState,
    error::{AppError, AppResult},
    models::{
        ActionProposal, ActiveAgentRun, ArtifactFilters, ArtifactSummary, ContextSegment,
        PreparedContext, ProposalApplyResult, ProposalStatus, ProviderCapabilities, RunEvent,
        ToolInvocation, ToolProtocol, WorkflowRun, WorkflowRunSummary,
    },
};

const V2_SCHEMA_VERSION: i64 = 4;

pub(crate) fn migrate(state: &AppState) -> AppResult<()> {
    state.with_conn(|conn| {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS v2_schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );",
        )?;
        let mut current: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM v2_schema_migrations",
            [],
            |row| row.get(0),
        )?;
        if current < 1 {
            apply_migration(conn, 1, migrate_v1)?;
            current = 1;
        }
        if current < 2 {
            apply_migration(conn, 2, migrate_v2)?;
            current = 2;
        }
        if current < 3 {
            apply_migration(conn, 3, migrate_v3)?;
            current = 3;
        }
        if current < 4 {
            apply_migration(conn, 4, migrate_v4)?;
        }
        debug_assert!(V2_SCHEMA_VERSION >= current);
        Ok(())
    })
}

fn apply_migration(
    conn: &Connection,
    version: i64,
    migration: fn(&Connection) -> AppResult<()>,
) -> AppResult<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        migration(conn)?;
        conn.execute(
            "INSERT INTO v2_schema_migrations(version, applied_at) VALUES (?1, ?2)",
            params![version, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn migrate_v1(conn: &Connection) -> AppResult<()> {
    if !column_exists(conn, "ai_providers", "tool_protocol")? {
        conn.execute_batch(
            "ALTER TABLE ai_providers ADD COLUMN tool_protocol TEXT NOT NULL DEFAULT 'auto';
             ALTER TABLE ai_providers ADD COLUMN detected_tool_protocol TEXT;
             ALTER TABLE ai_providers ADD COLUMN tool_capability_error TEXT;
             ALTER TABLE ai_providers ADD COLUMN tool_capability_updated_at TEXT;",
        )?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prepared_contexts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            chapter_id INTEGER,
            stage TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            system_prompt TEXT NOT NULL,
            prompt TEXT NOT NULL,
            segments_json TEXT NOT NULL DEFAULT '[]',
            tool_invocation_ids_json TEXT NOT NULL DEFAULT '[]',
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_prepared_contexts_lookup
            ON prepared_contexts(project_id, stage, expires_at DESC, id DESC);

        CREATE TABLE IF NOT EXISTS tool_invocations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER,
            prepared_context_id INTEGER,
            project_id INTEGER NOT NULL,
            chapter_id INTEGER,
            stage TEXT NOT NULL,
            tool_key TEXT NOT NULL,
            protocol TEXT NOT NULL,
            arguments_json TEXT NOT NULL,
            result_json TEXT NOT NULL DEFAULT '{}',
            status TEXT NOT NULL,
            error TEXT,
            elapsed_ms INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES workflow_runs(id) ON DELETE SET NULL,
            FOREIGN KEY(prepared_context_id) REFERENCES prepared_contexts(id) ON DELETE SET NULL,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_tool_invocations_run ON tool_invocations(run_id, id);
        CREATE INDEX IF NOT EXISTS idx_tool_invocations_context
            ON tool_invocations(prepared_context_id, id);

        CREATE TABLE IF NOT EXISTS action_proposals (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            chapter_id INTEGER,
            source_run_id INTEGER,
            proposal_type TEXT NOT NULL,
            summary TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            expected_version TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            decision_note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            decided_at TEXT,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
            FOREIGN KEY(source_run_id) REFERENCES workflow_runs(id) ON DELETE SET NULL
        );
        CREATE INDEX IF NOT EXISTS idx_action_proposals_project
            ON action_proposals(project_id, status, id DESC);

        CREATE TABLE IF NOT EXISTS agent_run_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER NOT NULL,
            project_id INTEGER NOT NULL,
            chapter_id INTEGER,
            sequence INTEGER NOT NULL,
            kind TEXT NOT NULL,
            delta TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            error TEXT,
            created_at TEXT NOT NULL,
            UNIQUE(run_id, sequence),
            FOREIGN KEY(run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE,
            FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_agent_run_events_run ON agent_run_events(run_id, sequence);",
    )?;
    Ok(())
}

fn migrate_v2(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE agent_run_contexts (
            run_id INTEGER PRIMARY KEY,
            prepared_context_id INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE,
            FOREIGN KEY(prepared_context_id) REFERENCES prepared_contexts(id) ON DELETE RESTRICT
        );
        CREATE INDEX idx_agent_run_contexts_prepared
            ON agent_run_contexts(prepared_context_id, run_id);",
    )?;
    Ok(())
}

fn migrate_v3(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "ALTER TABLE agent_run_events ADD COLUMN stage TEXT NOT NULL DEFAULT '';
         UPDATE agent_run_events
         SET stage = COALESCE((SELECT stage FROM workflow_runs WHERE id = run_id), '')
         WHERE stage = '';",
    )?;
    Ok(())
}

fn migrate_v4(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_run_artifacts (
            run_id INTEGER PRIMARY KEY,
            artifact_id INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE,
            FOREIGN KEY(artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_run_artifacts_artifact
            ON agent_run_artifacts(artifact_id);",
    )?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> AppResult<bool> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{}') WHERE name = ?1)",
        table.replace('\'', "''")
    );
    Ok(conn.query_row(&sql, [column], |row| row.get(0))?)
}

impl AppState {
    pub fn get_active_agent_run(&self, project_id: i64) -> AppResult<Option<ActiveAgentRun>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, project_id, chapter_id, stage, output, status, error, elapsed_ms, created_at
                 FROM workflow_runs
                 WHERE project_id = ?1 AND status IN ('streaming', 'running', 'cancellation_requested')
                 ORDER BY id DESC LIMIT 1",
                [project_id],
                |row| {
                    Ok(ActiveAgentRun {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        chapter_id: row.get(2)?,
                        stage: row.get(3)?,
                        output: row.get(4)?,
                        status: row.get(5)?,
                        error: row.get(6)?,
                        elapsed_ms: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn list_artifact_summaries(
        &self,
        filters: ArtifactFilters,
    ) -> AppResult<Vec<ArtifactSummary>> {
        self.with_conn(|conn| {
            let mut sql = String::from(
                "SELECT id, project_id, chapter_id, stage, title, version, status,
                        parent_artifact_id, created_at, content
                 FROM artifacts WHERE project_id = ?",
            );
            let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(filters.project_id)];
            if let Some(stage) = filters.stage {
                sql.push_str(" AND stage = ?");
                values.push(Box::new(stage));
            }
            if let Some(chapter_id) = filters.chapter_id {
                sql.push_str(" AND chapter_id = ?");
                values.push(Box::new(chapter_id));
            }
            sql.push_str(" ORDER BY created_at DESC, id DESC");
            let params = rusqlite::params_from_iter(values.iter().map(|value| &**value));
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params, |row| {
                let content: String = row.get(9)?;
                Ok(ArtifactSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    chapter_id: row.get(2)?,
                    stage: row.get(3)?,
                    title: row.get(4)?,
                    version: row.get(5)?,
                    status: row.get(6)?,
                    parent_artifact_id: row.get(7)?,
                    created_at: row.get(8)?,
                    char_count: content.chars().filter(|ch| !ch.is_whitespace()).count() as i64,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn formal_char_count(&self, project_id: i64) -> AppResult<i64> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT a.content
                 FROM chapters c
                 JOIN artifacts a ON a.id = c.current_artifact_id
                 WHERE c.project_id = ?1
                   AND a.project_id = ?1
                   AND (a.status = 'approved' OR EXISTS (
                     SELECT 1 FROM approvals p
                     WHERE p.project_id = ?1 AND p.artifact_id = a.id
                   ))",
            )?;
            let mut rows = stmt.query(params![project_id])?;
            let mut total = 0_i64;
            while let Some(row) = rows.next()? {
                let content: String = row.get(0)?;
                total += content.chars().filter(|ch| !ch.is_whitespace()).count() as i64;
            }
            Ok(total)
        })
    }

    pub fn list_workflow_run_summaries(
        &self,
        project_id: i64,
    ) -> AppResult<Vec<WorkflowRunSummary>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, chapter_id, stage, status, error, elapsed_ms,
                        length(output), created_at
                 FROM workflow_runs
                 WHERE project_id = ?1
                 ORDER BY created_at DESC, id DESC
                 LIMIT 100",
            )?;
            let rows = stmt.query_map([project_id], |row| {
                let output_chars: i64 = row.get(7)?;
                Ok(WorkflowRunSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    chapter_id: row.get(2)?,
                    stage: row.get(3)?,
                    status: row.get(4)?,
                    error: row.get(5)?,
                    elapsed_ms: row.get(6)?,
                    output_chars: output_chars.max(0) as usize,
                    created_at: row.get(8)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn get_workflow_run_v2(&self, run_id: i64) -> AppResult<WorkflowRun> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, project_id, chapter_id, stage, input, output, status, error,
                        elapsed_ms, created_at
                 FROM workflow_runs WHERE id = ?1",
                [run_id],
                |row| {
                    Ok(WorkflowRun {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        chapter_id: row.get(2)?,
                        stage: row.get(3)?,
                        input: row.get(4)?,
                        output: row.get(5)?,
                        status: row.get(6)?,
                        error: row.get(7)?,
                        elapsed_ms: row.get(8)?,
                        created_at: row.get(9)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("Agent 运行不存在".to_string()))
        })
    }

    pub fn link_run_artifact(&self, run_id: i64, artifact_id: i64) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO agent_run_artifacts(run_id, artifact_id, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(run_id) DO UPDATE SET artifact_id = excluded.artifact_id",
                params![run_id, artifact_id, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn artifact_id_for_run(&self, run_id: i64) -> AppResult<Option<i64>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT artifact_id FROM agent_run_artifacts WHERE run_id = ?1",
                    [run_id],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    pub fn run_cancellation_requested(&self, run_id: i64) -> AppResult<bool> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM workflow_runs
                     WHERE id = ?1 AND status = 'cancellation_requested'",
                    [run_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    pub fn insert_prepared_context(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
        stage: &str,
        fingerprint: &str,
        system_prompt: &str,
        prompt: &str,
        segments: &[ContextSegment],
        tool_invocation_ids: &[i64],
    ) -> AppResult<PreparedContext> {
        let created_at = Utc::now();
        let expires_at = created_at + Duration::minutes(15);
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO prepared_contexts
                    (project_id, chapter_id, stage, fingerprint, system_prompt, prompt,
                     segments_json, tool_invocation_ids_json, expires_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    project_id,
                    chapter_id,
                    stage,
                    fingerprint,
                    system_prompt,
                    prompt,
                    serde_json::to_string(segments)?,
                    serde_json::to_string(tool_invocation_ids)?,
                    expires_at.to_rfc3339(),
                    created_at.to_rfc3339(),
                ],
            )?;
            let prepared_context_id = tx.last_insert_rowid();
            for invocation_id in tool_invocation_ids {
                let changed = tx.execute(
                    "UPDATE tool_invocations
                     SET prepared_context_id = ?1
                     WHERE id = ?2 AND run_id IS NULL AND project_id = ?3",
                    params![prepared_context_id, invocation_id, project_id],
                )?;
                if changed != 1 {
                    return Err(AppError::Validation(
                        "工具审计记录与准备上下文不匹配".to_string(),
                    ));
                }
            }
            let prepared = query_prepared_context(&tx, prepared_context_id)?;
            tx.commit()?;
            Ok(prepared)
        })
    }

    pub fn get_prepared_context(&self, id: i64) -> AppResult<PreparedContext> {
        self.with_conn(|conn| query_prepared_context(conn, id))
    }

    pub fn purge_expired_prepared_contexts(&self) -> AppResult<usize> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM prepared_contexts WHERE expires_at <= ?1",
                [Utc::now().to_rfc3339()],
            )?)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_tool_invocation(
        &self,
        run_id: Option<i64>,
        prepared_context_id: Option<i64>,
        project_id: i64,
        chapter_id: Option<i64>,
        stage: &str,
        tool_key: &str,
        protocol: &str,
        arguments: &Value,
        result: &Value,
        status: &str,
        error: Option<&str>,
        elapsed_ms: i64,
    ) -> AppResult<ToolInvocation> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tool_invocations
                    (run_id, prepared_context_id, project_id, chapter_id, stage, tool_key,
                     protocol, arguments_json, result_json, status, error, elapsed_ms, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    run_id,
                    prepared_context_id,
                    project_id,
                    chapter_id,
                    stage,
                    tool_key,
                    protocol,
                    serde_json::to_string(arguments)?,
                    serde_json::to_string(result)?,
                    status,
                    error,
                    elapsed_ms,
                    Utc::now().to_rfc3339(),
                ],
            )?;
            query_tool_invocation(conn, conn.last_insert_rowid())
        })
    }

    pub fn list_tool_invocations_for_run(&self, run_id: i64) -> AppResult<Vec<ToolInvocation>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "{} WHERE run_id = ?1 ORDER BY id",
                TOOL_INVOCATION_SELECT
            ))?;
            let rows = stmt.query_map([run_id], map_tool_invocation)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn list_tool_invocations_for_context(
        &self,
        prepared_context_id: i64,
    ) -> AppResult<Vec<ToolInvocation>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "{} WHERE prepared_context_id = ?1 ORDER BY id",
                TOOL_INVOCATION_SELECT
            ))?;
            let rows = stmt.query_map([prepared_context_id], map_tool_invocation)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn link_run_prepared_context(
        &self,
        run_id: i64,
        prepared_context_id: i64,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO agent_run_contexts(run_id, prepared_context_id, created_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(run_id) DO UPDATE SET
                    prepared_context_id = excluded.prepared_context_id,
                    created_at = excluded.created_at",
                params![run_id, prepared_context_id, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn prepared_context_id_for_run(&self, run_id: i64) -> AppResult<Option<i64>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT prepared_context_id FROM agent_run_contexts WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_action_proposal(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
        source_run_id: Option<i64>,
        proposal_type: &str,
        summary: &str,
        payload: &Value,
        expected_version: Option<&str>,
    ) -> AppResult<ActionProposal> {
        self.get_project(project_id)?;
        if let Some(chapter_id) = chapter_id {
            self.ensure_chapter(project_id, Some(chapter_id))?
                .ok_or_else(|| AppError::Validation("提案章节不属于当前项目".to_string()))?;
        }
        validate_proposal_payload(proposal_type, payload)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO action_proposals
                    (project_id, chapter_id, source_run_id, proposal_type, summary, payload_json,
                     expected_version, status, decision_note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', '', ?8)",
                params![
                    project_id,
                    chapter_id,
                    source_run_id,
                    proposal_type,
                    summary.trim(),
                    serde_json::to_string(payload)?,
                    expected_version,
                    Utc::now().to_rfc3339(),
                ],
            )?;
            query_action_proposal(conn, conn.last_insert_rowid())
        })
    }

    pub fn list_action_proposals(
        &self,
        project_id: i64,
        status: Option<&str>,
    ) -> AppResult<Vec<ActionProposal>> {
        self.with_conn(|conn| {
            let sql = if status.is_some() {
                format!(
                    "{} WHERE project_id = ?1 AND status = ?2 ORDER BY id DESC",
                    ACTION_PROPOSAL_SELECT
                )
            } else {
                format!(
                    "{} WHERE project_id = ?1 ORDER BY id DESC",
                    ACTION_PROPOSAL_SELECT
                )
            };
            let mut stmt = conn.prepare(&sql)?;
            let rows = if let Some(status) = status {
                stmt.query_map(params![project_id, status], map_action_proposal)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            } else {
                stmt.query_map([project_id], map_action_proposal)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            Ok(rows)
        })
    }

    pub fn reject_action_proposal(
        &self,
        project_id: i64,
        proposal_id: i64,
        note: &str,
    ) -> AppResult<ActionProposal> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE action_proposals
                 SET status = 'rejected', decision_note = ?1, decided_at = ?2
                 WHERE id = ?3 AND project_id = ?4 AND status = 'pending'",
                params![
                    note.trim(),
                    Utc::now().to_rfc3339(),
                    proposal_id,
                    project_id
                ],
            )?;
            if changed == 0 {
                return Err(AppError::Validation(
                    "提案不存在、已处理或不属于当前项目".to_string(),
                ));
            }
            query_action_proposal(conn, proposal_id)
        })
    }

    pub fn apply_action_proposal(
        &self,
        project_id: i64,
        proposal_id: i64,
        note: &str,
    ) -> AppResult<ProposalApplyResult> {
        self.with_conn(|conn| {
            let needs_vector_runtime = conn
                .query_row(
                    "SELECT proposal_type FROM action_proposals WHERE id = ?1",
                    [proposal_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .is_some_and(|proposal_type| {
                    matches!(
                        proposal_type.as_str(),
                        "knowledge_card_update" | "knowledge_card_delete"
                    )
                });
            if needs_vector_runtime {
                crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, conn)?;
            }
            let tx = conn.unchecked_transaction()?;
            let proposal = query_action_proposal(&tx, proposal_id)?;
            if proposal.project_id != project_id || proposal.status != ProposalStatus::Pending {
                return Err(AppError::Validation(
                    "提案不存在、已处理或不属于当前项目".to_string(),
                ));
            }
            if proposal_is_stale(&tx, &proposal)? {
                tx.execute(
                    "UPDATE action_proposals
                     SET status = 'expired', decision_note = ?1, decided_at = ?2
                     WHERE id = ?3 AND status = 'pending'",
                    params![
                        "目标资源已在提案生成后变化",
                        Utc::now().to_rfc3339(),
                        proposal_id,
                    ],
                )?;
                tx.commit()?;
                return Err(AppError::Validation(
                    "目标资源已在提案生成后变化，提案已过期".to_string(),
                ));
            }
            let (entity_kind, entity_id) = apply_proposal_tx(&tx, &proposal)?;
            tx.execute(
                "UPDATE action_proposals
                 SET status = 'applied', decision_note = ?1, decided_at = ?2
                 WHERE id = ?3 AND status = 'pending'",
                params![note.trim(), Utc::now().to_rfc3339(), proposal_id],
            )?;
            let proposal = query_action_proposal(&tx, proposal_id)?;
            tx.commit()?;
            Ok(ProposalApplyResult {
                proposal,
                entity_kind,
                entity_id,
            })
        })
    }

    pub fn insert_run_event(
        &self,
        run_id: i64,
        project_id: i64,
        chapter_id: Option<i64>,
        kind: &str,
        delta: &str,
        status: &str,
        error: Option<&str>,
    ) -> AppResult<RunEvent> {
        let event = self.with_conn(|conn| {
            let sequence: i64 = conn.query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM agent_run_events WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO agent_run_events
                    (run_id, project_id, chapter_id, stage, sequence, kind, delta, status, error, created_at)
                 VALUES (?1, ?2, ?3, COALESCE((SELECT stage FROM workflow_runs WHERE id = ?1), ''),
                         ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    run_id,
                    project_id,
                    chapter_id,
                    sequence,
                    kind,
                    delta,
                    status,
                    error,
                    Utc::now().to_rfc3339(),
                ],
            )?;
            query_run_event(conn, conn.last_insert_rowid())
        })?;
        let _ = self.run_event_tx.send(event.clone());
        Ok(event)
    }

    pub fn list_run_events(&self, run_id: i64, after_sequence: i64) -> AppResult<Vec<RunEvent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "{} WHERE run_id = ?1 AND sequence > ?2 ORDER BY sequence",
                RUN_EVENT_SELECT
            ))?;
            let rows = stmt.query_map(params![run_id, after_sequence], map_run_event)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn provider_capabilities(&self, base_url: &str) -> AppResult<ProviderCapabilities> {
        self.with_conn(|conn| {
            let capabilities = conn
                .query_row(
                    "SELECT base_url, tool_protocol, detected_tool_protocol,
                        tool_capability_error, tool_capability_updated_at
                 FROM ai_providers WHERE base_url = ?1",
                    [base_url],
                    |row| {
                        let configured: String = row.get(1)?;
                        let detected: Option<String> = row.get(2)?;
                        Ok(ProviderCapabilities {
                            provider_base_url: row.get(0)?,
                            configured_protocol: ToolProtocol::parse(&configured),
                            detected_protocol: detected.as_deref().map(ToolProtocol::parse),
                            last_error: row.get(3)?,
                            updated_at: row.get(4)?,
                        })
                    },
                )
                .optional()?;
            Ok(capabilities.unwrap_or_else(|| ProviderCapabilities {
                provider_base_url: base_url.to_string(),
                configured_protocol: ToolProtocol::Auto,
                detected_protocol: None,
                last_error: None,
                updated_at: None,
            }))
        })
    }

    pub fn record_provider_tool_protocol(
        &self,
        base_url: &str,
        protocol: Option<ToolProtocol>,
        error: Option<&str>,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE ai_providers
                 SET detected_tool_protocol = ?1, tool_capability_error = ?2,
                     tool_capability_updated_at = ?3
                 WHERE base_url = ?4",
                params![
                    protocol.as_ref().map(ToolProtocol::as_str),
                    error,
                    Utc::now().to_rfc3339(),
                    base_url,
                ],
            )?;
            Ok(())
        })
    }
}

fn proposal_is_stale(tx: &Transaction<'_>, proposal: &ActionProposal) -> AppResult<bool> {
    let Some(expected) = proposal.expected_version.as_deref() else {
        return Ok(false);
    };
    let current = match proposal.proposal_type.as_str() {
        "rename_chapter" => {
            let chapter_id = json_i64(&proposal.payload, "chapter_id")?;
            tx.query_row(
                "SELECT updated_at FROM chapters WHERE id = ?1 AND project_id = ?2",
                params![chapter_id, proposal.project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        }
        "knowledge_card_update" | "knowledge_card_delete" => {
            let card_id = json_i64(&proposal.payload, "card_id")?;
            tx.query_row(
                "SELECT updated_at FROM knowledge_cards WHERE id = ?1 AND project_id = ?2",
                params![card_id, proposal.project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        }
        _ => return Ok(false),
    };
    Ok(current.as_deref() != Some(expected))
}

fn validate_proposal_payload(proposal_type: &str, payload: &Value) -> AppResult<()> {
    let required = match proposal_type {
        "create_chapter" => &["title"][..],
        "rename_chapter" => &["chapter_id", "title"][..],
        "artifact_candidate" => &["stage", "title", "content"][..],
        "knowledge_card" => &["category", "title", "content"][..],
        "knowledge_card_update" => &["card_id", "category", "title", "content"][..],
        "knowledge_card_delete" => &["card_id"][..],
        "foreshadowing" => &["title", "content"][..],
        _ => {
            return Err(AppError::Validation(format!(
                "不支持的写入提案类型：{proposal_type}"
            )))
        }
    };
    let object = payload
        .as_object()
        .ok_or_else(|| AppError::Validation("提案 payload 必须是对象".to_string()))?;
    for key in required {
        if !object.contains_key(*key) {
            return Err(AppError::Validation(format!("提案缺少字段：{key}")));
        }
    }
    Ok(())
}

fn apply_proposal_tx(tx: &Transaction<'_>, proposal: &ActionProposal) -> AppResult<(String, i64)> {
    let now = Utc::now().to_rfc3339();
    match proposal.proposal_type.as_str() {
        "create_chapter" => {
            let title = json_string(&proposal.payload, "title")?;
            let next_no: i64 = tx.query_row(
                "SELECT COALESCE(MAX(chapter_no), 0) + 1 FROM chapters WHERE project_id = ?1",
                [proposal.project_id],
                |row| row.get(0),
            )?;
            tx.execute(
                "INSERT INTO chapters(project_id, chapter_no, title, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'planning', ?4, ?4)",
                params![proposal.project_id, next_no, title.trim(), now],
            )?;
            Ok(("chapter".to_string(), tx.last_insert_rowid()))
        }
        "rename_chapter" => {
            let chapter_id = json_i64(&proposal.payload, "chapter_id")?;
            let title = json_string(&proposal.payload, "title")?;
            if let Some(expected) = proposal.expected_version.as_deref() {
                let current: String = tx.query_row(
                    "SELECT updated_at FROM chapters WHERE id = ?1 AND project_id = ?2",
                    params![chapter_id, proposal.project_id],
                    |row| row.get(0),
                )?;
                if current != expected {
                    return Err(AppError::Validation(
                        "章节已在提案生成后被修改，请重新生成提案".to_string(),
                    ));
                }
            }
            let changed = tx.execute(
                "UPDATE chapters SET title = ?1, updated_at = ?2
                 WHERE id = ?3 AND project_id = ?4",
                params![title.trim(), now, chapter_id, proposal.project_id],
            )?;
            if changed == 0 {
                return Err(AppError::Validation("章节不存在".to_string()));
            }
            Ok(("chapter".to_string(), chapter_id))
        }
        "artifact_candidate" => {
            let stage = json_string(&proposal.payload, "stage")?;
            if !matches!(stage, "setting" | "outline" | "characters") {
                return Err(AppError::Validation("候选资料阶段不合法".to_string()));
            }
            let title = json_string(&proposal.payload, "title")?;
            let content = json_string(&proposal.payload, "content")?;
            let version: i64 = tx.query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM artifacts
                 WHERE project_id = ?1 AND chapter_id IS NULL AND stage = ?2",
                params![proposal.project_id, stage],
                |row| row.get(0),
            )?;
            tx.execute(
                "INSERT INTO artifacts
                    (project_id, chapter_id, stage, title, content, version, status, parent_artifact_id, created_at)
                 VALUES (?1, NULL, ?2, ?3, ?4, ?5, 'pending_human_approval', NULL, ?6)",
                params![proposal.project_id, stage, title.trim(), content, version, now],
            )?;
            Ok(("artifact".to_string(), tx.last_insert_rowid()))
        }
        "knowledge_card" => {
            let category = json_string(&proposal.payload, "category")?;
            let title = json_string(&proposal.payload, "title")?;
            let content = json_string(&proposal.payload, "content")?;
            let source_chapter_id =
                existing_chapter_id(tx, proposal.project_id, proposal.chapter_id)?;
            tx.execute(
                "INSERT INTO knowledge_cards
                    (project_id, category, title, content, status, source_artifact_id,
                     source_chapter_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'pending_human_approval', NULL, ?5, ?6, ?6)",
                params![
                    proposal.project_id,
                    category.trim(),
                    title.trim(),
                    content,
                    source_chapter_id,
                    now,
                ],
            )?;
            Ok(("knowledge_card".to_string(), tx.last_insert_rowid()))
        }
        "knowledge_card_update" => {
            let card_id = json_i64(&proposal.payload, "card_id")?;
            let category = json_string(&proposal.payload, "category")?;
            let title = json_string(&proposal.payload, "title")?;
            let content = json_string(&proposal.payload, "content")?;
            let status = tx
                .query_row(
                    "SELECT status FROM knowledge_cards WHERE id = ?1 AND project_id = ?2",
                    params![card_id, proposal.project_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| AppError::Validation("知识卡不存在或不属于当前项目".to_string()))?;
            let changed = tx.execute(
                "UPDATE knowledge_cards
                 SET category = ?1, title = ?2, content = ?3, updated_at = ?4
                 WHERE id = ?5 AND project_id = ?6",
                params![
                    category.trim(),
                    title.trim(),
                    content,
                    now,
                    card_id,
                    proposal.project_id
                ],
            )?;
            if changed == 0 {
                return Err(AppError::Validation(
                    "知识卡不存在或不属于当前项目".to_string(),
                ));
            }
            crate::story_search::delete_source(tx, proposal.project_id, "knowledge_card", card_id)?;
            tx.execute(
                "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                params![now, proposal.project_id],
            )?;
            if status == "approved" {
                crate::db::mark_story_bible_changed_tx(tx, proposal.project_id, &now)?;
            }
            Ok(("knowledge_card".to_string(), card_id))
        }
        "knowledge_card_delete" => {
            let card_id = json_i64(&proposal.payload, "card_id")?;
            let status = tx
                .query_row(
                    "SELECT status FROM knowledge_cards WHERE id = ?1 AND project_id = ?2",
                    params![card_id, proposal.project_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| AppError::Validation("知识卡不存在或不属于当前项目".to_string()))?;
            crate::story_search::delete_source(tx, proposal.project_id, "knowledge_card", card_id)?;
            let changed = tx.execute(
                "DELETE FROM knowledge_cards WHERE id = ?1 AND project_id = ?2",
                params![card_id, proposal.project_id],
            )?;
            if changed == 0 {
                return Err(AppError::Validation(
                    "知识卡不存在或不属于当前项目".to_string(),
                ));
            }
            tx.execute(
                "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                params![now, proposal.project_id],
            )?;
            if status == "approved" {
                crate::db::mark_story_bible_changed_tx(tx, proposal.project_id, &now)?;
            }
            Ok(("knowledge_card".to_string(), card_id))
        }
        "foreshadowing" => {
            let title = json_string(&proposal.payload, "title")?;
            let content = json_string(&proposal.payload, "content")?;
            let payoff = proposal
                .payload
                .get("planned_payoff_note")
                .and_then(Value::as_str)
                .unwrap_or("");
            let planted_chapter_id =
                existing_chapter_id(tx, proposal.project_id, proposal.chapter_id)?;
            tx.execute(
                "INSERT INTO foreshadowings
                    (project_id, title, content, status, planted_chapter_id,
                     planned_payoff_chapter_id, planned_payoff_note, source_artifact_id,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'pending_human_approval', ?4, NULL, ?5, NULL, ?6, ?6)",
                params![
                    proposal.project_id,
                    title.trim(),
                    content,
                    planted_chapter_id,
                    payoff.trim(),
                    now,
                ],
            )?;
            Ok(("foreshadowing".to_string(), tx.last_insert_rowid()))
        }
        _ => Err(AppError::Validation("不支持的写入提案类型".to_string())),
    }
}

fn json_string<'a>(payload: &'a Value, key: &str) -> AppResult<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Validation(format!("提案字段 {key} 不能为空")))
}

fn json_i64(payload: &Value, key: &str) -> AppResult<i64> {
    payload
        .get(key)
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| AppError::Validation(format!("提案字段 {key} 不合法")))
}

/// Resolves a chapter id to `Some(id)` only if that chapter still exists in the
/// project. Returns `None` otherwise so callers can fall back to a NULL FK column
/// instead of failing the whole write with `FOREIGN KEY constraint failed`
/// (the referenced chapter may have been deleted after the proposal was created).
fn existing_chapter_id(
    tx: &Transaction<'_>,
    project_id: i64,
    chapter_id: Option<i64>,
) -> AppResult<Option<i64>> {
    let Some(chapter_id) = chapter_id else {
        return Ok(None);
    };
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM chapters WHERE id = ?1 AND project_id = ?2)",
        params![chapter_id, project_id],
        |row| row.get(0),
    )?;
    Ok(if exists { Some(chapter_id) } else { None })
}

fn query_prepared_context(conn: &Connection, id: i64) -> AppResult<PreparedContext> {
    conn.query_row(
        "SELECT id, project_id, chapter_id, stage, fingerprint, system_prompt, prompt,
                segments_json, tool_invocation_ids_json, expires_at, created_at
         FROM prepared_contexts WHERE id = ?1",
        [id],
        |row| {
            let prompt: String = row.get(6)?;
            let segments_json: String = row.get(7)?;
            let tool_ids_json: String = row.get(8)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                prompt,
                segments_json,
                tool_ids_json,
                row.get(9)?,
                row.get(10)?,
            ))
        },
    )
    .optional()?
    .map(
        |(
            id,
            project_id,
            chapter_id,
            stage,
            fingerprint,
            system_prompt,
            prompt,
            segments_json,
            tool_ids_json,
            expires_at,
            created_at,
        ): (
            i64,
            i64,
            Option<i64>,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        )| {
            let total_chars = prompt.chars().count();
            Ok::<PreparedContext, AppError>(PreparedContext {
                id,
                project_id,
                chapter_id,
                stage,
                fingerprint,
                system_prompt,
                prompt,
                segments: serde_json::from_str(&segments_json)?,
                tool_invocation_ids: serde_json::from_str(&tool_ids_json)?,
                total_chars,
                estimated_tokens: total_chars.div_ceil(4),
                expires_at,
                created_at,
            })
        },
    )
    .transpose()?
    .ok_or_else(|| AppError::Validation("准备上下文不存在".to_string()))
}

const TOOL_INVOCATION_SELECT: &str = "SELECT id, run_id, prepared_context_id, project_id,
    chapter_id, stage, tool_key, protocol, arguments_json, result_json, status, error,
    elapsed_ms, created_at FROM tool_invocations";

fn query_tool_invocation(conn: &Connection, id: i64) -> AppResult<ToolInvocation> {
    conn.query_row(
        &format!("{} WHERE id = ?1", TOOL_INVOCATION_SELECT),
        [id],
        map_tool_invocation,
    )
    .map_err(AppError::from)
}

fn map_tool_invocation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolInvocation> {
    let arguments: String = row.get(8)?;
    let result: String = row.get(9)?;
    Ok(ToolInvocation {
        id: row.get(0)?,
        run_id: row.get(1)?,
        prepared_context_id: row.get(2)?,
        project_id: row.get(3)?,
        chapter_id: row.get(4)?,
        stage: row.get(5)?,
        tool_key: row.get(6)?,
        protocol: row.get(7)?,
        arguments: serde_json::from_str(&arguments).map_err(json_sql_error)?,
        result: serde_json::from_str(&result).map_err(json_sql_error)?,
        status: row.get(10)?,
        error: row.get(11)?,
        elapsed_ms: row.get(12)?,
        created_at: row.get(13)?,
    })
}

const ACTION_PROPOSAL_SELECT: &str = "SELECT id, project_id, chapter_id, source_run_id,
    proposal_type, summary, payload_json, expected_version, status, decision_note, created_at,
    decided_at FROM action_proposals";

fn query_action_proposal(conn: &Connection, id: i64) -> AppResult<ActionProposal> {
    conn.query_row(
        &format!("{} WHERE id = ?1", ACTION_PROPOSAL_SELECT),
        [id],
        map_action_proposal,
    )
    .optional()?
    .ok_or_else(|| AppError::Validation("提案不存在".to_string()))
}

fn map_action_proposal(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActionProposal> {
    let payload: String = row.get(6)?;
    Ok(ActionProposal {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_id: row.get(2)?,
        source_run_id: row.get(3)?,
        proposal_type: row.get(4)?,
        summary: row.get(5)?,
        payload: serde_json::from_str(&payload).map_err(json_sql_error)?,
        expected_version: row.get(7)?,
        status: ProposalStatus::parse(&row.get::<_, String>(8)?),
        decision_note: row.get(9)?,
        created_at: row.get(10)?,
        decided_at: row.get(11)?,
    })
}

const RUN_EVENT_SELECT: &str = "SELECT id, run_id, project_id, chapter_id, stage, sequence, kind,
    delta, status, error, created_at FROM agent_run_events";

fn query_run_event(conn: &Connection, id: i64) -> AppResult<RunEvent> {
    conn.query_row(
        &format!("{} WHERE id = ?1", RUN_EVENT_SELECT),
        [id],
        map_run_event,
    )
    .map_err(AppError::from)
}

fn map_run_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunEvent> {
    Ok(RunEvent {
        run_id: row.get(1)?,
        project_id: row.get(2)?,
        chapter_id: row.get(3)?,
        stage: row.get(4)?,
        sequence: row.get(5)?,
        kind: row.get(6)?,
        delta: row.get(7)?,
        status: row.get(8)?,
        error: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewChapter, NewProject, SaveKnowledgeCard};

    #[test]
    fn proposal_does_not_mutate_until_applied() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "提案测试".to_string(),
                genre: "测试".to_string(),
                target_words: 10_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter_count_before = state.list_chapters(project.id).unwrap().len();
        let proposal = state
            .create_action_proposal(
                project.id,
                None,
                None,
                "create_chapter",
                "创建第一章",
                &serde_json::json!({"title": "第一章"}),
                None,
            )
            .unwrap();
        assert_eq!(
            state.list_chapters(project.id).unwrap().len(),
            chapter_count_before
        );

        let applied = state
            .apply_action_proposal(project.id, proposal.id, "确认")
            .unwrap();
        assert_eq!(applied.entity_kind, "chapter");
        assert_eq!(
            state.list_chapters(project.id).unwrap().len(),
            chapter_count_before + 1
        );
    }

    #[test]
    fn rename_proposal_detects_version_conflict() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "冲突测试".to_string(),
                genre: "测试".to_string(),
                target_words: 10_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("旧标题".to_string()),
            })
            .unwrap();
        let proposal = state
            .create_action_proposal(
                project.id,
                Some(chapter.id),
                None,
                "rename_chapter",
                "重命名",
                &serde_json::json!({"chapter_id": chapter.id, "title": "新标题"}),
                Some("stale-version"),
            )
            .unwrap();
        assert!(state
            .apply_action_proposal(project.id, proposal.id, "确认")
            .is_err());
        assert_eq!(
            state
                .list_action_proposals(project.id, None)
                .unwrap()
                .into_iter()
                .find(|item| item.id == proposal.id)
                .unwrap()
                .status,
            ProposalStatus::Expired
        );
    }

    #[test]
    fn knowledge_card_proposal_detects_version_conflict() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "资料卡冲突测试".to_string(),
                genre: "测试".to_string(),
                target_words: 10_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let card = state
            .save_knowledge_card(SaveKnowledgeCard {
                id: None,
                project_id: project.id,
                category: "人物".to_string(),
                title: "旧称".to_string(),
                content: "旧内容".to_string(),
                status: "approved".to_string(),
                source_artifact_id: None,
                source_chapter_id: None,
            })
            .unwrap();
        let proposal = state
            .create_action_proposal(
                project.id,
                None,
                None,
                "knowledge_card_update",
                "更新资料卡",
                &serde_json::json!({
                    "card_id": card.id,
                    "category": "人物",
                    "title": "新称",
                    "content": "提案内容"
                }),
                Some(&card.updated_at),
            )
            .unwrap();
        state
            .save_knowledge_card(SaveKnowledgeCard {
                id: Some(card.id),
                project_id: project.id,
                category: "人物".to_string(),
                title: "人工修改".to_string(),
                content: "人工内容".to_string(),
                status: "approved".to_string(),
                source_artifact_id: None,
                source_chapter_id: None,
            })
            .unwrap();

        assert!(state
            .apply_action_proposal(project.id, proposal.id, "确认")
            .is_err());
        assert_eq!(
            state
                .get_knowledge_card(project.id, card.id)
                .unwrap()
                .content,
            "人工内容"
        );
        assert_eq!(
            state
                .list_action_proposals(project.id, None)
                .unwrap()
                .into_iter()
                .find(|item| item.id == proposal.id)
                .unwrap()
                .status,
            ProposalStatus::Expired
        );
    }

    #[test]
    fn deleting_a_knowledge_card_proposal_removes_search_documents() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "资料卡删除测试".to_string(),
                genre: "测试".to_string(),
                target_words: 10_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let card = state
            .save_knowledge_card(SaveKnowledgeCard {
                id: None,
                project_id: project.id,
                category: "world".to_string(),
                title: "待删除资料".to_string(),
                content: "过期检索内容".to_string(),
                status: "approved".to_string(),
                source_artifact_id: None,
                source_chapter_id: None,
            })
            .unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO story_search_documents
                        (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage,
                         title, content, search_text, chunk_no, chunk_start, chunk_end,
                         visibility_cutoff_chapter_no, source_text_hash, normalization_version,
                         updated_at)
                     VALUES (?1, 'knowledge_card', ?2, NULL, NULL, NULL, ?3, ?4, ?4,
                             0, 0, 6, NULL, 'test-hash', 'test-v1', ?5)",
                    params![
                        project.id,
                        card.id,
                        card.title,
                        card.content,
                        card.updated_at,
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        let proposal = state
            .create_action_proposal(
                project.id,
                None,
                None,
                "knowledge_card_delete",
                "删除资料卡",
                &serde_json::json!({"card_id": card.id}),
                Some(&card.updated_at),
            )
            .unwrap();

        state
            .apply_action_proposal(project.id, proposal.id, "确认")
            .unwrap();
        assert!(state.get_knowledge_card(project.id, card.id).is_err());
        state
            .with_conn(|conn| {
                let remaining: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM story_search_documents
                     WHERE project_id = ?1 AND source_kind = 'knowledge_card' AND source_id = ?2",
                    params![project.id, card.id],
                    |row| row.get(0),
                )?;
                assert_eq!(remaining, 0);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn prepared_context_owns_preview_tool_audit_records() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "审计测试".to_string(),
                genre: "测试".to_string(),
                target_words: 10_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let invocation = state
            .insert_tool_invocation(
                None,
                None,
                project.id,
                None,
                "setting",
                "reference_materials",
                "structured",
                &serde_json::json!({}),
                &serde_json::json!({"status":"success"}),
                "success",
                None,
                3,
            )
            .unwrap();
        let prepared = state
            .insert_prepared_context(
                project.id,
                None,
                "setting",
                "fingerprint",
                "system",
                "prompt",
                &[],
                &[invocation.id],
            )
            .unwrap();
        let records = state
            .list_tool_invocations_for_context(prepared.id)
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].prepared_context_id, Some(prepared.id));
    }
}
