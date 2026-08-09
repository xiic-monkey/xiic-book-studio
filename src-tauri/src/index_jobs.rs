use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::time::sleep;

use crate::{
    chapter_memory,
    db::AppState,
    error::{AppError, AppResult},
    models::{DerivedIndexJob, RebuildStorySearchIndexRequest, RetryIndexJobsRequest},
    story_index, story_search,
};

pub(crate) const STORY_CHAPTER_JOB: &str = "story_chapter";
pub(crate) const SEARCH_CHAPTER_JOB: &str = "search_chapter";
pub(crate) const SEARCH_PROJECT_JOB: &str = "search_project";

const MAX_JOB_RETRIES: i64 = 3;
const MAX_JOB_ATTEMPTS: i64 = 1 + MAX_JOB_RETRIES;
const JOB_SELECT: &str = "SELECT id, project_id, chapter_id, source_artifact_id, job_type, scope_key,
    status, attempt_count, next_attempt_at, last_error, created_at, started_at, finished_at, updated_at
    FROM derived_index_jobs";

pub(crate) fn recover_running_jobs(state: &AppState) -> AppResult<()> {
    let timestamp = now();
    state.with_conn(|conn| {
        conn.execute(
            "UPDATE derived_index_jobs
             SET status = 'pending', next_attempt_at = ?1, started_at = NULL,
                 finished_at = NULL, updated_at = ?1
             WHERE status = 'running'",
            params![timestamp],
        )?;
        Ok(())
    })
}

pub(crate) fn start_worker(state: AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            match process_next_job(&state).await {
                Ok(true) => continue,
                Ok(false) => {
                    tokio::select! {
                        _ = state.wait_for_index_work() => {}
                        _ = sleep(Duration::from_secs(5)) => {}
                    }
                }
                Err(error) => {
                    eprintln!("derived index worker error: {error}");
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });
}

pub(crate) fn enqueue_chapter_index_jobs_tx(
    conn: &Connection,
    project_id: i64,
    chapter_id: i64,
    source_artifact_id: i64,
    timestamp: &str,
) -> AppResult<()> {
    enqueue_chapter_job_tx(
        conn,
        project_id,
        chapter_id,
        source_artifact_id,
        STORY_CHAPTER_JOB,
        timestamp,
    )?;
    enqueue_chapter_job_tx(
        conn,
        project_id,
        chapter_id,
        source_artifact_id,
        SEARCH_CHAPTER_JOB,
        timestamp,
    )?;
    Ok(())
}

fn enqueue_search_chapter_job_tx(
    conn: &Connection,
    project_id: i64,
    chapter_id: i64,
    source_artifact_id: i64,
    timestamp: &str,
) -> AppResult<()> {
    enqueue_chapter_job_tx(
        conn,
        project_id,
        chapter_id,
        source_artifact_id,
        SEARCH_CHAPTER_JOB,
        timestamp,
    )
}

fn enqueue_chapter_job_tx(
    conn: &Connection,
    project_id: i64,
    chapter_id: i64,
    source_artifact_id: i64,
    job_type: &str,
    timestamp: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO derived_index_jobs
            (project_id, chapter_id, source_artifact_id, job_type, scope_key, status,
             attempt_count, next_attempt_at, last_error, created_at, started_at, finished_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0, ?6, NULL, ?6, NULL, NULL, ?6)
         ON CONFLICT(project_id, job_type, scope_key) DO UPDATE SET
            chapter_id = excluded.chapter_id,
            source_artifact_id = excluded.source_artifact_id,
            status = 'pending',
            attempt_count = 0,
            next_attempt_at = excluded.next_attempt_at,
            last_error = NULL,
            started_at = NULL,
            finished_at = NULL,
            updated_at = excluded.updated_at",
        params![
            project_id,
            chapter_id,
            source_artifact_id,
            job_type,
            format!("chapter:{chapter_id}"),
            timestamp
        ],
    )?;
    Ok(())
}

/// Queue only missing or stale search-index work for existing approved chapter
/// bodies. This does not schedule AI fact extraction and never touches user text.
pub(crate) fn enqueue_missing_search_jobs(state: &AppState) -> AppResult<usize> {
    let timestamp = now();
    state.with_conn(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut stmt = conn.prepare(
                "SELECT c.project_id, c.id, a.id, c.title, a.content,
                        s.source_artifact_id, s.source_text_hash, s.status
                 FROM chapters c
                 INNER JOIN artifacts a ON a.id = c.current_artifact_id
                 LEFT JOIN story_search_sources s
                   ON s.project_id = c.project_id
                  AND s.source_kind = 'chapter'
                  AND s.source_id = c.id
                 WHERE a.stage IN ('draft', 'revision')
                   AND EXISTS (SELECT 1 FROM approvals ap WHERE ap.artifact_id = a.id)
                 ORDER BY c.project_id, c.chapter_no",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })?;
            let rows = rows.collect::<Result<Vec<_>, _>>()?;
            let mut queued = 0;
            for (
                project_id,
                chapter_id,
                artifact_id,
                title,
                content,
                source_artifact_id,
                source_hash,
                status,
            ) in rows
            {
                let current_hash = crate::story_search::chapter_source_hash(&title, &content);
                if source_artifact_id == Some(artifact_id)
                    && source_hash.as_deref() == Some(current_hash.as_str())
                    && status.as_deref() == Some("success")
                {
                    continue;
                }
                enqueue_search_chapter_job_tx(
                    conn,
                    project_id,
                    chapter_id,
                    artifact_id,
                    &timestamp,
                )?;
                queued += 1;
            }
            Ok(queued)
        })();
        match result {
            Ok(queued) => {
                conn.execute_batch("COMMIT")?;
                Ok(queued)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    })
}

pub(crate) fn enqueue_following_chapter_index_jobs_tx(
    conn: &Connection,
    project_id: i64,
    chapter_id: i64,
    timestamp: &str,
) -> AppResult<()> {
    let chapter_no = conn
        .query_row(
            "SELECT chapter_no FROM chapters WHERE id = ?1 AND project_id = ?2",
            params![chapter_id, project_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, current_artifact_id
         FROM chapters
         WHERE project_id = ?1 AND chapter_no > ?2 AND current_artifact_id IS NOT NULL
         ORDER BY chapter_no ASC",
    )?;
    let chapters = stmt
        .query_map(params![project_id, chapter_no], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (following_chapter_id, source_artifact_id) in chapters {
        enqueue_chapter_index_jobs_tx(
            conn,
            project_id,
            following_chapter_id,
            source_artifact_id,
            timestamp,
        )?;
    }
    Ok(())
}

pub(crate) fn enqueue_project_search_job_tx(
    conn: &Connection,
    project_id: i64,
    timestamp: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO derived_index_jobs
            (project_id, chapter_id, source_artifact_id, job_type, scope_key, status,
             attempt_count, next_attempt_at, last_error, created_at, started_at, finished_at, updated_at)
         VALUES (?1, NULL, NULL, ?2, 'project', 'pending', 0, ?3, NULL, ?3, NULL, NULL, ?3)
         ON CONFLICT(project_id, job_type, scope_key) DO UPDATE SET
            status = 'pending',
            attempt_count = 0,
            next_attempt_at = excluded.next_attempt_at,
            last_error = NULL,
            started_at = NULL,
            finished_at = NULL,
            updated_at = excluded.updated_at",
        params![project_id, SEARCH_PROJECT_JOB, timestamp],
    )?;
    Ok(())
}

fn requeue_chapter_index_jobs_tx(
    conn: &Connection,
    project_id: i64,
    chapter_id: i64,
    source_artifact_id: i64,
    timestamp: &str,
) -> AppResult<()> {
    for (job_type, scope_key) in [
        (STORY_CHAPTER_JOB, format!("chapter:{chapter_id}")),
        (SEARCH_CHAPTER_JOB, format!("chapter:{chapter_id}")),
    ] {
        let status = conn
            .query_row(
                "SELECT status FROM derived_index_jobs
                 WHERE project_id = ?1 AND job_type = ?2 AND scope_key = ?3",
                params![project_id, job_type, scope_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match status.as_deref() {
            None => {
                conn.execute(
                    "INSERT INTO derived_index_jobs
                        (project_id, chapter_id, source_artifact_id, job_type, scope_key, status,
                         attempt_count, next_attempt_at, last_error, created_at, started_at, finished_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0, ?6, NULL, ?6, NULL, NULL, ?6)",
                    params![
                        project_id,
                        chapter_id,
                        source_artifact_id,
                        job_type,
                        scope_key,
                        timestamp
                    ],
                )?;
            }
            Some("pending") | Some("failed") => {
                conn.execute(
                    "UPDATE derived_index_jobs
                     SET chapter_id = ?1, source_artifact_id = ?2, status = 'pending',
                         attempt_count = 0, next_attempt_at = ?3, last_error = NULL,
                         started_at = NULL, finished_at = NULL, updated_at = ?3
                     WHERE project_id = ?4 AND job_type = ?5 AND scope_key = ?6",
                    params![
                        chapter_id,
                        source_artifact_id,
                        timestamp,
                        project_id,
                        job_type,
                        scope_key
                    ],
                )?;
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn requeue_project_search_job_tx(
    conn: &Connection,
    project_id: i64,
    timestamp: &str,
) -> AppResult<()> {
    let status = conn
        .query_row(
            "SELECT status FROM derived_index_jobs
             WHERE project_id = ?1 AND job_type = ?2 AND scope_key = 'project'",
            params![project_id, SEARCH_PROJECT_JOB],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match status.as_deref() {
        None => enqueue_project_search_job_tx(conn, project_id, timestamp),
        Some("pending") | Some("failed") => {
            conn.execute(
                "UPDATE derived_index_jobs
                 SET status = 'pending', attempt_count = 0, next_attempt_at = ?1,
                     last_error = NULL, started_at = NULL, finished_at = NULL, updated_at = ?1
                 WHERE project_id = ?2 AND job_type = ?3 AND scope_key = 'project'",
                params![timestamp, project_id, SEARCH_PROJECT_JOB],
            )?;
            Ok(())
        }
        Some(_) => Ok(()),
    }
}

pub(crate) fn enqueue_project_search_job(state: &AppState, project_id: i64) -> AppResult<()> {
    let result = state.with_conn(|conn| enqueue_project_search_job_tx(conn, project_id, &now()));
    if result.is_ok() {
        state.wake_index_worker();
    }
    result
}

pub(crate) fn retry_index_jobs(
    state: &AppState,
    input: RetryIndexJobsRequest,
) -> AppResult<Vec<DerivedIndexJob>> {
    state.get_project(input.project_id)?;
    let chapters = match input.chapter_id {
        Some(chapter_id) => vec![state
            .ensure_chapter(input.project_id, Some(chapter_id))?
            .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?],
        None => state.list_chapters(input.project_id)?,
    };
    let timestamp = now();

    state.with_conn(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            for chapter in &chapters {
                if let Some(artifact_id) = chapter.current_artifact_id {
                    requeue_chapter_index_jobs_tx(
                        conn,
                        input.project_id,
                        chapter.id,
                        artifact_id,
                        &timestamp,
                    )?;
                }
            }
            if input.chapter_id.is_none() {
                requeue_project_search_job_tx(conn, input.project_id, &timestamp)?;
            }
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
    })?;

    state.wake_index_worker();
    state.list_derived_index_jobs(input.project_id)
}

async fn process_next_job(state: &AppState) -> AppResult<bool> {
    let Some(job) = claim_next_job(state)? else {
        return Ok(false);
    };

    let result = match job.job_type.as_str() {
        STORY_CHAPTER_JOB => {
            let chapter_id = job
                .chapter_id
                .ok_or_else(|| AppError::Validation("资料索引任务缺少章节".to_string()))?;
            story_index::index_approved_chapter_for_job(state, job.project_id, chapter_id)
                .await
                .map(|_| ())
        }
        SEARCH_CHAPTER_JOB => {
            let chapter_id = job
                .chapter_id
                .ok_or_else(|| AppError::Validation("搜索索引任务缺少章节".to_string()))?;
            story_search::index_approved_chapter(state, job.project_id, chapter_id).await
        }
        SEARCH_PROJECT_JOB => story_search::rebuild_story_search_index(
            state,
            RebuildStorySearchIndexRequest {
                project_id: job.project_id,
            },
        )
        .await
        .map(|_| ()),
        other => Err(AppError::Validation(format!("未知索引任务类型：{other}"))),
    };

    finalize_job(state, &job, result)?;
    Ok(true)
}

fn claim_next_job(state: &AppState) -> AppResult<Option<DerivedIndexJob>> {
    state.with_conn(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut stmt = conn.prepare(&format!(
                "{JOB_SELECT}
                 WHERE status = 'pending' AND next_attempt_at <= ?1
                 ORDER BY next_attempt_at ASC, id ASC LIMIT 1"
            ))?;
            let job = stmt.query_row(params![now()], map_job).optional()?;
            let Some(mut job) = job else {
                return Ok(None);
            };
            let timestamp = now();
            conn.execute(
                "UPDATE derived_index_jobs
                 SET status = 'running', attempt_count = attempt_count + 1,
                     started_at = ?1, finished_at = NULL, updated_at = ?1
                 WHERE id = ?2 AND status = 'pending'",
                params![timestamp, job.id],
            )?;
            job.status = "running".to_string();
            job.attempt_count += 1;
            job.started_at = Some(timestamp.clone());
            job.finished_at = None;
            job.updated_at = timestamp;
            Ok(Some(job))
        })();

        match result {
            Ok(job) => {
                conn.execute_batch("COMMIT")?;
                Ok(job)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    })
}

fn claimed_job_is_current(state: &AppState, job: &DerivedIndexJob) -> AppResult<bool> {
    state.with_conn(|conn| {
        let current = conn
            .query_row(
                "SELECT status, attempt_count, updated_at
                 FROM derived_index_jobs WHERE id = ?1",
                params![job.id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        Ok(current.is_some_and(|(status, attempt_count, updated_at)| {
            status == "running"
                && attempt_count == job.attempt_count
                && updated_at == job.updated_at
        }))
    })
}

fn finalize_job(state: &AppState, job: &DerivedIndexJob, result: AppResult<()>) -> AppResult<()> {
    let timestamp = now();
    match result {
        Ok(()) => state.with_conn(|conn| {
            conn.execute(
                "UPDATE derived_index_jobs
                 SET status = 'succeeded', last_error = NULL, finished_at = ?1, updated_at = ?1
                 WHERE id = ?2 AND status = 'running'
                   AND (source_artifact_id IS NULL OR source_artifact_id = ?3)
                   AND attempt_count = ?4 AND updated_at = ?5",
                params![
                    timestamp,
                    job.id,
                    job.source_artifact_id,
                    job.attempt_count,
                    job.updated_at
                ],
            )?;
            Ok(())
        }),
        Err(error) => {
            let retry = job.attempt_count < MAX_JOB_ATTEMPTS && is_retryable(&error);
            if !retry {
                record_story_index_error_if_current(state, job, &error)?;
                record_search_index_error_if_current(state, job, &error)?;
            }
            let next_attempt_at = if retry {
                let delay_seconds = match job.attempt_count {
                    1 => 60,
                    2 => 300,
                    _ => 1_800,
                };
                (Utc::now() + ChronoDuration::seconds(delay_seconds)).to_rfc3339()
            } else {
                timestamp.clone()
            };
            let status = if retry { "pending" } else { "failed" };
            state.with_conn(|conn| {
                conn.execute(
                    "UPDATE derived_index_jobs
                     SET status = ?1, last_error = ?2, next_attempt_at = ?3,
                         finished_at = CASE WHEN ?1 = 'failed' THEN ?4 ELSE NULL END,
                         updated_at = ?4
                     WHERE id = ?5 AND status = 'running'
                       AND (source_artifact_id IS NULL OR source_artifact_id = ?6)
                       AND attempt_count = ?7 AND updated_at = ?8",
                    params![
                        status,
                        error.to_string(),
                        next_attempt_at,
                        timestamp,
                        job.id,
                        job.source_artifact_id,
                        job.attempt_count,
                        job.updated_at
                    ],
                )?;
                Ok(())
            })
        }
    }
}

fn record_story_index_error_if_current(
    state: &AppState,
    job: &DerivedIndexJob,
    error: &AppError,
) -> AppResult<()> {
    if job.job_type != STORY_CHAPTER_JOB {
        return Ok(());
    }
    if !claimed_job_is_current(state, job)? {
        return Ok(());
    }
    let (Some(chapter_id), Some(source_artifact_id)) = (job.chapter_id, job.source_artifact_id)
    else {
        return Ok(());
    };
    let Some(source) = state.latest_approved_chapter_body(job.project_id, chapter_id)? else {
        return Ok(());
    };
    if source.id != source_artifact_id {
        return Ok(());
    }
    state.record_story_index_failure(
        job.project_id,
        chapter_id,
        source.id,
        &chapter_memory::source_text_hash(&source.content),
        story_index::NORMALIZATION_VERSION,
        &error.to_string(),
    )
}

fn record_search_index_error_if_current(
    state: &AppState,
    job: &DerivedIndexJob,
    error: &AppError,
) -> AppResult<()> {
    if !claimed_job_is_current(state, job)? {
        return Ok(());
    }
    match job.job_type.as_str() {
        SEARCH_CHAPTER_JOB => {
            let (Some(chapter_id), Some(source_artifact_id)) =
                (job.chapter_id, job.source_artifact_id)
            else {
                return Ok(());
            };
            let Some(source) = state.latest_approved_chapter_body(job.project_id, chapter_id)?
            else {
                return Ok(());
            };
            if source.id != source_artifact_id {
                return Ok(());
            }
            state.with_conn(|conn| {
                conn.execute(
                    "UPDATE story_search_sources
                     SET status = 'failed', error = ?1, indexed_at = ?2
                     WHERE project_id = ?3 AND source_kind = 'chapter' AND source_id = ?4
                       AND source_artifact_id = ?5
                       AND EXISTS (
                           SELECT 1 FROM derived_index_jobs
                           WHERE id = ?6 AND status = 'running'
                             AND attempt_count = ?7 AND updated_at = ?8
                       )",
                    params![
                        error.to_string(),
                        now(),
                        job.project_id,
                        chapter_id,
                        source_artifact_id,
                        job.id,
                        job.attempt_count,
                        job.updated_at
                    ],
                )?;
                Ok(())
            })
        }
        SEARCH_PROJECT_JOB => state.with_conn(|conn| {
            conn.execute(
                "UPDATE story_search_sources
                 SET status = 'failed', error = ?1, indexed_at = ?2
                 WHERE project_id = ?3
                   AND EXISTS (
                       SELECT 1 FROM derived_index_jobs
                       WHERE id = ?4 AND status = 'running'
                         AND attempt_count = ?5 AND updated_at = ?6
                   )",
                params![
                    error.to_string(),
                    now(),
                    job.project_id,
                    job.id,
                    job.attempt_count,
                    job.updated_at
                ],
            )?;
            Ok(())
        }),
        _ => Ok(()),
    }
}

fn is_retryable(error: &AppError) -> bool {
    match error {
        AppError::Network(_) => true,
        AppError::Validation(message) => {
            let lower = message.to_ascii_lowercase();
            lower.contains("timeout")
                || lower.contains("超时")
                || lower.contains("没有建立")
                || lower.contains("没有返回")
                || lower.contains("没有返回可用内容")
                || lower.contains("无法解析")
                || lower.contains("json error")
                || lower.contains("ai 返回错误")
                || lower.contains("流式响应未正常结束")
                || lower.contains("输出达到长度上限")
                || lower.contains("http 408")
                || lower.contains("http 429")
                || (500..=599).any(|status| lower.contains(&format!("http {status}")))
        }
        AppError::Json(_) => true,
        AppError::Database(_) | AppError::Io(_) => false,
    }
}

fn map_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<DerivedIndexJob> {
    Ok(DerivedIndexJob {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_id: row.get(2)?,
        source_artifact_id: row.get(3)?,
        job_type: row.get(4)?,
        scope_key: row.get(5)?,
        status: row.get(6)?,
        attempt_count: row.get(7)?,
        next_attempt_at: row.get(8)?,
        last_error: row.get(9)?,
        created_at: row.get(10)?,
        started_at: row.get(11)?,
        finished_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewProject;

    fn state_with_project() -> (tempfile::NamedTempFile, AppState, i64) {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "索引任务测试".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        (temp, state, project.id)
    }

    #[test]
    fn claim_and_finalize_job_release_the_database_between_states() {
        let (_temp, state, project_id) = state_with_project();
        state
            .with_conn(|conn| enqueue_project_search_job_tx(conn, project_id, &now()))
            .unwrap();

        let claimed = claim_next_job(&state).unwrap().unwrap();
        assert_eq!(claimed.status, "running");
        assert_eq!(claimed.attempt_count, 1);
        state
            .with_conn(|conn| {
                let status: String = conn.query_row(
                    "SELECT status FROM derived_index_jobs WHERE id = ?1",
                    params![claimed.id],
                    |row| row.get(0),
                )?;
                assert_eq!(status, "running");
                Ok(())
            })
            .unwrap();

        finalize_job(&state, &claimed, Ok(())).unwrap();
        let jobs = state.list_derived_index_jobs(project_id).unwrap();
        assert_eq!(jobs[0].status, "succeeded");
        assert!(jobs[0].finished_at.is_some());
    }

    #[test]
    fn retryable_jobs_allow_initial_attempt_plus_three_retries() {
        let (_temp, state, project_id) = state_with_project();
        state
            .with_conn(|conn| enqueue_project_search_job_tx(conn, project_id, &now()))
            .unwrap();
        let temporary_error = || AppError::Validation("AI 请求失败：HTTP 503".to_string());

        let first = claim_next_job(&state).unwrap().unwrap();
        finalize_job(&state, &first, Err(temporary_error())).unwrap();
        let first_status = state.list_derived_index_jobs(project_id).unwrap().remove(0);
        assert_eq!(first_status.status, "pending");
        assert_eq!(first_status.attempt_count, 1);
        assert!(first_status.last_error.is_some());

        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE derived_index_jobs SET next_attempt_at = ?1 WHERE id = ?2",
                    params![now(), first.id],
                )?;
                Ok(())
            })
            .unwrap();
        let second = claim_next_job(&state).unwrap().unwrap();
        finalize_job(&state, &second, Err(temporary_error())).unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE derived_index_jobs SET next_attempt_at = ?1 WHERE id = ?2",
                    params![now(), second.id],
                )?;
                Ok(())
            })
            .unwrap();
        let third = claim_next_job(&state).unwrap().unwrap();
        finalize_job(&state, &third, Err(temporary_error())).unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE derived_index_jobs SET next_attempt_at = ?1 WHERE id = ?2",
                    params![now(), third.id],
                )?;
                Ok(())
            })
            .unwrap();
        let fourth = claim_next_job(&state).unwrap().unwrap();
        finalize_job(&state, &fourth, Err(temporary_error())).unwrap();

        let final_job = state.list_derived_index_jobs(project_id).unwrap().remove(0);
        assert_eq!(final_job.status, "failed");
        assert_eq!(final_job.attempt_count, 4);
        assert!(final_job.finished_at.is_some());
    }

    #[test]
    fn configuration_errors_fail_without_retry() {
        let (_temp, state, project_id) = state_with_project();
        state
            .with_conn(|conn| enqueue_project_search_job_tx(conn, project_id, &now()))
            .unwrap();
        let job = claim_next_job(&state).unwrap().unwrap();
        finalize_job(
            &state,
            &job,
            Err(AppError::Validation("请先设置 API Key".to_string())),
        )
        .unwrap();
        let job = state.list_derived_index_jobs(project_id).unwrap().remove(0);
        assert_eq!(job.status, "failed");
        assert_eq!(job.attempt_count, 1);
        assert!(job.last_error.unwrap().contains("API Key"));
    }

    #[test]
    fn superseded_job_cannot_finalize_over_new_artifact() {
        let (_temp, state, project_id) = state_with_project();
        let replacement = state
            .insert_artifact(project_id, None, "setting", "新资料", "内容", None)
            .unwrap();
        state
            .with_conn(|conn| enqueue_project_search_job_tx(conn, project_id, &now()))
            .unwrap();
        let old = claim_next_job(&state).unwrap().unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE derived_index_jobs
                     SET status = 'pending', source_artifact_id = ?1, updated_at = ?2
                     WHERE id = ?3",
                    params![replacement.id, now(), old.id],
                )?;
                Ok(())
            })
            .unwrap();
        finalize_job(&state, &old, Ok(())).unwrap();
        let job = state.list_derived_index_jobs(project_id).unwrap().remove(0);
        assert_eq!(job.status, "pending");
        assert_eq!(job.source_artifact_id, Some(replacement.id));
    }

    #[test]
    fn superseded_project_job_cannot_finalize_after_reclaim() {
        let (_temp, state, project_id) = state_with_project();
        state
            .with_conn(|conn| enqueue_project_search_job_tx(conn, project_id, &now()))
            .unwrap();
        let old = claim_next_job(&state).unwrap().unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE derived_index_jobs
                     SET status = 'pending', attempt_count = 0, updated_at = ?1
                     WHERE id = ?2",
                    params![now(), old.id],
                )?;
                Ok(())
            })
            .unwrap();
        let current = claim_next_job(&state).unwrap().unwrap();
        assert_eq!(current.attempt_count, 1);

        finalize_job(&state, &old, Ok(())).unwrap();
        let job = state.list_derived_index_jobs(project_id).unwrap().remove(0);
        assert_eq!(job.status, "running");
        assert_eq!(job.attempt_count, current.attempt_count);

        finalize_job(&state, &current, Ok(())).unwrap();
        assert_eq!(
            state.list_derived_index_jobs(project_id).unwrap()[0].status,
            "succeeded"
        );
    }

    #[test]
    fn retry_classifier_only_accepts_transient_errors() {
        let network_error = reqwest::Client::new().get("://").build().unwrap_err();
        assert!(is_retryable(&AppError::Network(network_error)));
        assert!(is_retryable(&AppError::Validation("HTTP 429".to_string())));
        assert!(is_retryable(&AppError::Validation("HTTP 501".to_string())));
        assert!(is_retryable(&AppError::Validation(
            "AI 没有返回可用内容".to_string()
        )));
        assert!(is_retryable(&AppError::Validation(
            "无法解析流式 AI 响应".to_string()
        )));
        assert!(is_retryable(&AppError::Json(
            serde_json::from_str::<serde_json::Value>("bad").unwrap_err(),
        )));
        assert!(!is_retryable(&AppError::Validation(
            "请先设置 API Key".to_string()
        )));
        assert!(!is_retryable(&AppError::Validation(
            "资料索引条目过多".to_string()
        )));
    }

    #[test]
    fn backfill_queues_only_missing_search_index_work() {
        let (_temp, state, project_id) = state_with_project();
        let chapter = state.list_chapters(project_id).unwrap().remove(0);
        let artifact = state
            .insert_artifact(
                project_id,
                Some(chapter.id),
                "draft",
                "第一章正文",
                "黑牌被封入石匣。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project_id, "draft", artifact.id, "通过")
            .unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "DELETE FROM derived_index_jobs WHERE project_id = ?1",
                    [project_id],
                )?;
                Ok(())
            })
            .unwrap();

        assert_eq!(enqueue_missing_search_jobs(&state).unwrap(), 1);
        let jobs = state.list_derived_index_jobs(project_id).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_type, SEARCH_CHAPTER_JOB);
        assert_eq!(jobs[0].chapter_id, Some(chapter.id));
        assert_eq!(jobs[0].source_artifact_id, Some(artifact.id));

        assert_eq!(enqueue_missing_search_jobs(&state).unwrap(), 1);
        assert_eq!(state.list_derived_index_jobs(project_id).unwrap().len(), 1);
    }
}
