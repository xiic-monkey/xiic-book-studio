use rusqlite::{params, Connection, OptionalExtension};

use crate::{error::AppResult, workflow};

use super::AppState;

pub(super) fn initialize(state: &AppState) -> AppResult<()> {
    state.migrate()?;
    migrate_derived_schema(state)?;
    crate::index_jobs::recover_running_jobs(state)?;
    recover_stale_workflow_runs(state)?;
    workflow::rebuild_story_threads(state)
}

const DERIVED_SCHEMA_VERSION: i64 = 2;

fn migrate_derived_schema(state: &AppState) -> AppResult<()> {
    state.with_conn(|conn| {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version >= DERIVED_SCHEMA_VERSION {
            return Ok(());
        }

        ensure_search_vector_extension(state, conn)?;

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            migrate_search_documents(conn)?;
            migrate_search_sources(conn)?;
            mark_unverified_search_sources(conn)?;
            normalize_duplicate_artifact_versions(conn)?;
            conn.execute_batch(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_artifacts_version_chapter
                     ON artifacts(project_id, chapter_id, stage, version)
                     WHERE chapter_id IS NOT NULL;
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_artifacts_version_project
                     ON artifacts(project_id, stage, version)
                     WHERE chapter_id IS NULL;",
            )?;
            conn.pragma_update(None, "user_version", DERIVED_SCHEMA_VERSION)?;
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
    })
}

fn migrate_search_documents(conn: &Connection) -> AppResult<()> {
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings
             WHERE rowid NOT IN (SELECT id FROM story_search_documents)",
            [],
        )?;
    }

    conn.execute_batch(
        "DROP TRIGGER IF EXISTS story_search_documents_ai;
         DROP TRIGGER IF EXISTS story_search_documents_ad;
         DROP TRIGGER IF EXISTS story_search_documents_au;
         DROP TABLE IF EXISTS story_search_documents_fts;
         DROP INDEX IF EXISTS idx_story_search_documents_source;
         DROP INDEX IF EXISTS idx_story_search_documents_chapter;
         ALTER TABLE story_search_documents RENAME TO story_search_documents_legacy;
         CREATE TABLE story_search_documents (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             project_id INTEGER NOT NULL,
             source_kind TEXT NOT NULL,
             source_id INTEGER NOT NULL,
             chapter_id INTEGER,
             chapter_no_sort INTEGER,
             stage TEXT,
             title TEXT NOT NULL,
             content TEXT NOT NULL,
             search_text TEXT NOT NULL,
             chunk_no INTEGER NOT NULL,
             chunk_start INTEGER NOT NULL,
             chunk_end INTEGER NOT NULL,
             visibility_cutoff_chapter_no INTEGER,
             source_text_hash TEXT NOT NULL,
             normalization_version TEXT NOT NULL,
             updated_at TEXT NOT NULL,
             FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
             FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
         );",
    )?;
    conn.execute(
        "INSERT INTO story_search_documents
            (id, project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, title,
             content, search_text, chunk_no, chunk_start, chunk_end, visibility_cutoff_chapter_no,
             source_text_hash, normalization_version, updated_at)
         SELECT d.id, d.project_id, d.source_kind, d.source_id, d.chapter_id, d.chapter_no_sort, d.stage,
                d.title, d.content, d.search_text, d.chunk_no, d.chunk_start, d.chunk_end,
                d.visibility_cutoff_chapter_no, d.source_text_hash, d.normalization_version, d.updated_at
         FROM story_search_documents_legacy d
         INNER JOIN projects p ON p.id = d.project_id
         LEFT JOIN chapters c ON c.id = d.chapter_id
         WHERE d.chapter_id IS NULL OR c.project_id = d.project_id",
        [],
    )?;
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings
             WHERE rowid NOT IN (SELECT id FROM story_search_documents)",
            [],
        )?;
    }
    conn.execute_batch(
        "DROP TABLE story_search_documents_legacy;
         CREATE INDEX IF NOT EXISTS idx_story_search_documents_source
             ON story_search_documents(project_id, source_kind, source_id, chunk_no);
         CREATE INDEX IF NOT EXISTS idx_story_search_documents_chapter
             ON story_search_documents(project_id, chapter_no_sort, source_kind);
         CREATE VIRTUAL TABLE story_search_documents_fts
             USING fts5(search_text, content='story_search_documents', content_rowid='id', tokenize='trigram');
         CREATE TRIGGER story_search_documents_ai AFTER INSERT ON story_search_documents BEGIN
             INSERT INTO story_search_documents_fts(rowid, search_text) VALUES (new.id, new.search_text);
         END;
         CREATE TRIGGER story_search_documents_ad AFTER DELETE ON story_search_documents BEGIN
             INSERT INTO story_search_documents_fts(story_search_documents_fts, rowid, search_text)
             VALUES ('delete', old.id, old.search_text);
         END;
         CREATE TRIGGER story_search_documents_au AFTER UPDATE ON story_search_documents BEGIN
             INSERT INTO story_search_documents_fts(story_search_documents_fts, rowid, search_text)
             VALUES ('delete', old.id, old.search_text);
             INSERT INTO story_search_documents_fts(rowid, search_text) VALUES (new.id, new.search_text);
         END;
         INSERT INTO story_search_documents_fts(story_search_documents_fts) VALUES ('rebuild');",
    )?;
    Ok(())
}

fn migrate_search_sources(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_story_search_sources_status;
         ALTER TABLE story_search_sources RENAME TO story_search_sources_legacy;
         CREATE TABLE story_search_sources (
             project_id INTEGER NOT NULL,
             source_kind TEXT NOT NULL,
             source_id INTEGER NOT NULL,
             chapter_id INTEGER,
             chapter_no_sort INTEGER,
             stage TEXT,
             source_artifact_id INTEGER,
             source_text_hash TEXT NOT NULL,
             normalization_version TEXT NOT NULL,
             status TEXT NOT NULL,
             error TEXT,
             indexed_at TEXT NOT NULL,
             PRIMARY KEY(project_id, source_kind, source_id),
             FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
             FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
             FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
         );",
    )?;
    conn.execute(
        "INSERT INTO story_search_sources
            (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, source_artifact_id,
             source_text_hash, normalization_version, status, error, indexed_at)
         SELECT s.project_id, s.source_kind, s.source_id, s.chapter_id, s.chapter_no_sort, s.stage,
                s.source_artifact_id, s.source_text_hash, s.normalization_version, s.status, s.error, s.indexed_at
         FROM story_search_sources_legacy s
         INNER JOIN projects p ON p.id = s.project_id
         LEFT JOIN chapters c ON c.id = s.chapter_id
         LEFT JOIN artifacts a ON a.id = s.source_artifact_id
         WHERE (s.chapter_id IS NULL OR c.project_id = s.project_id)
           AND (s.source_artifact_id IS NULL OR a.project_id = s.project_id)
         ON CONFLICT(project_id, source_kind, source_id) DO UPDATE SET
             chapter_id = excluded.chapter_id,
             chapter_no_sort = excluded.chapter_no_sort,
             stage = excluded.stage,
             source_artifact_id = excluded.source_artifact_id,
             source_text_hash = excluded.source_text_hash,
             normalization_version = excluded.normalization_version,
             status = excluded.status,
             error = excluded.error,
             indexed_at = excluded.indexed_at",
        [],
    )?;
    conn.execute_batch(
        "DROP TABLE story_search_sources_legacy;
         CREATE INDEX IF NOT EXISTS idx_story_search_sources_status
             ON story_search_sources(project_id, status, chapter_no_sort);",
    )?;
    Ok(())
}

fn mark_unverified_search_sources(conn: &Connection) -> AppResult<()> {
    let vector_sql = conn
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'story_search_embeddings'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(vector_sql) = vector_sql else {
        conn.execute(
            "UPDATE story_search_sources
             SET status = 'fts_only',
                 error = '数据库迁移后无法确认向量索引；已保留全文检索'
             WHERE status = 'success'",
            [],
        )?;
        return Ok(());
    };

    let vector_sql = vector_sql.to_ascii_lowercase();
    if !vector_sql.contains("using vec0") || !vector_sql.contains("float[512]") {
        conn.execute(
            "UPDATE story_search_sources
             SET status = 'fts_only',
                 error = '数据库迁移后无法确认向量模型；已保留全文检索'
             WHERE status = 'success'",
            [],
        )?;
        return Ok(());
    }

    conn.execute(
        "UPDATE story_search_sources AS s
         SET status = 'fts_only',
             error = '数据库迁移后部分向量无法复用；已保留全文检索'
         WHERE s.status = 'success'
           AND (
               NOT EXISTS (
                   SELECT 1 FROM story_search_documents d
                   WHERE d.project_id = s.project_id
                     AND d.source_kind = s.source_kind
                     AND d.source_id = s.source_id
               )
               OR EXISTS (
                   SELECT 1
                   FROM story_search_documents d
                   LEFT JOIN story_search_embeddings e ON e.rowid = d.id
                   WHERE d.project_id = s.project_id
                     AND d.source_kind = s.source_kind
                     AND d.source_id = s.source_id
                     AND e.rowid IS NULL
               )
           )",
        [],
    )?;
    Ok(())
}

fn ensure_search_vector_extension(state: &AppState, conn: &Connection) -> AppResult<()> {
    let vector_sql = conn
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'table' AND name = 'story_search_embeddings'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if vector_sql
        .as_deref()
        .is_some_and(|sql| sql.to_ascii_lowercase().contains("using vec0"))
    {
        crate::story_search::ensure_sqlite_vec_loaded(state, conn)?;
    }
    Ok(())
}

fn normalize_duplicate_artifact_versions(conn: &Connection) -> AppResult<()> {
    let mut groups = conn.prepare(
        "SELECT project_id, chapter_id, stage
         FROM artifacts
         GROUP BY project_id, chapter_id, stage
         HAVING COUNT(*) > 1",
    )?;
    let groups = groups
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (project_id, chapter_id, stage) in groups {
        let mut ids_stmt = conn.prepare(
            "SELECT id FROM artifacts
             WHERE project_id = ?1 AND chapter_id IS ?2 AND stage = ?3
             ORDER BY version ASC, id ASC",
        )?;
        let ids = ids_stmt
            .query_map(params![project_id, chapter_id, stage], |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for id in &ids {
            conn.execute(
                "UPDATE artifacts SET version = -id WHERE id = ?1",
                params![id],
            )?;
        }
        for (index, id) in ids.iter().enumerate() {
            conn.execute(
                "UPDATE artifacts SET version = ?1 WHERE id = ?2",
                params![index as i64 + 1, id],
            )?;
        }
    }
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> AppResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn recover_stale_workflow_runs(state: &AppState) -> AppResult<()> {
    state.with_conn(|conn| {
        conn.execute(
            "UPDATE workflow_runs
             SET status = 'failed',
                 error = CASE
                     WHEN error IS NULL OR trim(error) = ''
                     THEN '应用重启前未完成的 Agent 运行已中止'
                     ELSE error
                 END
             WHERE status = 'streaming'",
            [],
        )?;
        Ok(())
    })
}
