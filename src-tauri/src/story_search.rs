use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

use crate::{
    db::AppState,
    error::{AppError, AppResult},
    models::{
        RebuildStorySearchIndexRequest, StoryContextSearchInput, StoryContextSnippet,
        StorySearchStatus,
    },
};

pub const MODEL_VERSION: &str = "bge-small-zh-v1.5";
pub const NORMALIZATION_VERSION: &str = "story-search-v2-small-512";
const VECTOR_DIMENSIONS: usize = 512;
const RRF_K: f64 = 60.0;
const FTS_LIMIT: usize = 30;
const VECTOR_LIMIT: usize = 30;

#[derive(Clone)]
struct SearchSourcePayload {
    project_id: i64,
    source_kind: &'static str,
    source_id: i64,
    chapter_id: Option<i64>,
    chapter_no_sort: Option<i64>,
    stage: Option<String>,
    source_artifact_id: Option<i64>,
    title: String,
    content: String,
    search_label: String,
    chunk_max: usize,
    overlap: usize,
    single_document_if_short: bool,
}

#[derive(Clone)]
struct SearchDocument {
    id: i64,
    source_kind: String,
    source_id: i64,
    chapter_no_sort: Option<i64>,
    title: String,
    content: String,
    search_text: String,
}

#[derive(Default)]
struct RankState {
    document: Option<SearchDocument>,
    score: f64,
    exact: bool,
}

#[derive(Deserialize)]
struct ModelManifest {
    checksums: BTreeMap<String, String>,
}

struct EmbeddingRuntime {
    tokenizer: Tokenizer,
    model: BertModel,
    device: Device,
}

pub async fn rebuild_story_search_index(
    state: &AppState,
    input: RebuildStorySearchIndexRequest,
) -> AppResult<StorySearchStatus> {
    state.get_project(input.project_id)?;
    ensure_sqlite_vec_loaded_if_present(state)?;
    clear_project_index(state, input.project_id)?;

    let runtime = EmbeddingRuntime::load(state);
    let runtime_error = runtime.as_ref().err().map(ToString::to_string);
    let vector_ready = runtime.as_ref().is_ok() && ensure_vector_table(state).is_ok();

    for payload in collect_project_sources(state, input.project_id)? {
        replace_source(
            state,
            &payload,
            runtime.as_ref().ok(),
            vector_ready,
            runtime_error.as_deref(),
        )?;
    }

    get_story_search_status(state, input.project_id)
}

pub async fn index_approved_chapter(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    ensure_sqlite_vec_loaded_if_present(state)?;
    let chapter = state
        .list_chapters(project_id)?
        .into_iter()
        .find(|chapter| chapter.id == chapter_id)
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;

    invalidate_chapters_from(state, project_id, chapter.id)?;

    let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter_id)? else {
        return Ok(());
    };

    replace_source_with_runtime(
        state,
        &SearchSourcePayload {
            project_id,
            source_kind: "chapter",
            source_id: chapter.id,
            chapter_id: Some(chapter.id),
            chapter_no_sort: Some(chapter.chapter_no),
            stage: Some(artifact.stage.clone()),
            source_artifact_id: Some(artifact.id),
            title: chapter.title.clone(),
            content: artifact.content,
            search_label: format!("第 {} 章 {}", chapter.chapter_no, chapter.title),
            chunk_max: 420,
            overlap: 60,
            single_document_if_short: false,
        },
    )
}

pub(crate) fn refresh_chapter_metadata(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    let chapter = state
        .ensure_chapter(project_id, Some(chapter_id))?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter_id)? else {
        return Ok(());
    };
    let hash = source_hash(&chapter.title, &artifact.content);
    let label = format!("第 {} 章 {}", chapter.chapter_no, chapter.title);
    let timestamp = now();
    state.with_conn(|conn| {
        conn.execute(
            "UPDATE story_search_documents
             SET title = ?1,
                 search_text = ?2 || char(10) || ?1 || char(10) || content,
                 chapter_no_sort = ?3,
                 visibility_cutoff_chapter_no = ?3,
                 source_text_hash = ?4,
                 updated_at = ?5
             WHERE project_id = ?6 AND source_kind = 'chapter' AND source_id = ?7",
            params![
                chapter.title,
                label,
                chapter.chapter_no,
                hash,
                timestamp,
                project_id,
                chapter_id
            ],
        )?;
        conn.execute(
            "UPDATE story_search_sources
             SET chapter_no_sort = ?1,
                 source_artifact_id = ?2,
                 source_text_hash = ?3,
                 indexed_at = ?4
             WHERE project_id = ?5 AND source_kind = 'chapter' AND source_id = ?6",
            params![
                chapter.chapter_no,
                artifact.id,
                hash,
                timestamp,
                project_id,
                chapter_id
            ],
        )?;
        Ok(())
    })
}

pub async fn refresh_approved_artifact_stage(
    state: &AppState,
    project_id: i64,
    stage: &str,
) -> AppResult<()> {
    if !matches!(stage, "setting" | "outline" | "characters") {
        return Ok(());
    }

    ensure_sqlite_vec_loaded_if_present(state)?;
    state.with_conn(|conn| delete_source_kind_stage(conn, project_id, "artifact", stage))?;

    if let Some(artifact) = state.approved_artifact(project_id, stage, None)? {
        replace_source_with_runtime(
            state,
            &artifact_payload(
                project_id,
                stage,
                artifact.id,
                artifact.title,
                artifact.content,
            ),
        )?;
    }

    Ok(())
}

pub async fn refresh_knowledge_card(
    state: &AppState,
    project_id: i64,
    card_id: i64,
) -> AppResult<()> {
    ensure_sqlite_vec_loaded_if_present(state)?;
    state.with_conn(|conn| delete_source(conn, project_id, "knowledge_card", card_id))?;

    if let Some(card) = state
        .list_knowledge_cards(project_id)?
        .into_iter()
        .find(|card| card.id == card_id && card.status == "approved")
    {
        replace_source_with_runtime(
            state,
            &SearchSourcePayload {
                project_id,
                source_kind: "knowledge_card",
                source_id: card.id,
                chapter_id: card.source_chapter_id,
                chapter_no_sort: chapter_no(state, project_id, card.source_chapter_id)?,
                stage: None,
                source_artifact_id: card.source_artifact_id,
                title: card.title.clone(),
                content: card.content,
                search_label: format!("知识卡：{}", card.title),
                chunk_max: 400,
                overlap: 60,
                single_document_if_short: true,
            },
        )?;
    }

    Ok(())
}

pub async fn refresh_foreshadowing(
    state: &AppState,
    project_id: i64,
    foreshadowing_id: i64,
) -> AppResult<()> {
    ensure_sqlite_vec_loaded_if_present(state)?;
    state.with_conn(|conn| delete_source(conn, project_id, "foreshadowing", foreshadowing_id))?;

    if let Some(item) = state
        .list_foreshadowings(project_id)?
        .into_iter()
        .find(|item| {
            item.id == foreshadowing_id
                && matches!(
                    item.status.as_str(),
                    "active" | "ready_for_payoff" | "approved"
                )
        })
    {
        replace_source_with_runtime(
            state,
            &SearchSourcePayload {
                project_id,
                source_kind: "foreshadowing",
                source_id: item.id,
                chapter_id: item.planted_chapter_id,
                chapter_no_sort: chapter_no(state, project_id, item.planted_chapter_id)?,
                stage: None,
                source_artifact_id: item.source_artifact_id,
                title: item.title.clone(),
                content: item.content,
                search_label: format!("伏笔：{}", item.title),
                chunk_max: 400,
                overlap: 60,
                single_document_if_short: true,
            },
        )?;
    }

    Ok(())
}

pub fn search_story_context(
    state: &AppState,
    input: &StoryContextSearchInput,
) -> AppResult<Vec<StoryContextSnippet>> {
    state.get_project(input.project_id)?;
    let query = normalize_text(&input.query);
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let upper_bound = search_upper_bound(
        state,
        input.project_id,
        input.chapter_id,
        input.include_immediate_previous,
    )?;

    let mut ranks = HashMap::<i64, RankState>::new();

    for (rank, document) in fts_candidates(state, input.project_id, upper_bound, &query)?
        .into_iter()
        .enumerate()
    {
        add_rank(&mut ranks, document, rank, &query);
    }

    if let Ok(runtime) = EmbeddingRuntime::load(state) {
        if ensure_vector_table(state).is_ok() {
            if let Ok(vector) = runtime.encode(&query) {
                for (rank, document) in
                    vector_candidates(state, input.project_id, upper_bound, &vector)?
                        .into_iter()
                        .enumerate()
                {
                    add_rank(&mut ranks, document, rank, &query);
                }
            }
        }
    }

    let limit = input.limit.unwrap_or(6).clamp(1, 12);
    let mut per_source = HashMap::<(String, i64), usize>::new();
    let mut ranked = ranks
        .into_values()
        .filter_map(|mut state| state.document.take().map(|document| (state, document)))
        .collect::<Vec<_>>();

    ranked.sort_by(|(left, _), (right, _)| {
        right
            .exact
            .cmp(&left.exact)
            .then_with(|| right.score.total_cmp(&left.score))
    });

    let mut output = Vec::new();
    for (state, document) in ranked {
        let source_key = (document.source_kind.clone(), document.source_id);
        let used = per_source.entry(source_key).or_default();
        if *used >= 2
            || output.iter().any(|snippet: &StoryContextSnippet| {
                same_content(&snippet.content, &document.content)
            })
        {
            continue;
        }
        *used += 1;
        output.push(StoryContextSnippet {
            source_label: source_label(&document),
            matched_term: matched_term(&query, &document),
            content: document.content,
            score: ((state.score * 1_000_000.0).round() as usize).max(1),
        });
        if output.len() == limit {
            break;
        }
    }

    Ok(output)
}

pub fn get_story_search_status(state: &AppState, project_id: i64) -> AppResult<StorySearchStatus> {
    state.get_project(project_id)?;
    let vector_table_error = ensure_vector_table(state).err();
    let sources = state.list_story_search_sources(project_id)?;
    let (document_count, embedding_count, last_indexed_at) = state.with_conn(|conn| {
        let document_count = conn.query_row(
            "SELECT COUNT(*) FROM story_search_documents WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )? as usize;

        let embedding_count = if table_exists(conn, "story_search_embeddings")? {
            conn.query_row(
                "SELECT COUNT(*) FROM story_search_embeddings e
                 INNER JOIN story_search_documents d ON d.id = e.rowid
                 WHERE d.project_id = ?1",
                params![project_id],
                |row| row.get::<_, i64>(0),
            )? as usize
        } else {
            0
        };

        let last_indexed_at = conn.query_row(
            "SELECT MAX(indexed_at) FROM story_search_sources WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, Option<String>>(0),
        )?;

        Ok((document_count, embedding_count, last_indexed_at))
    })?;

    let stale_sources = sources
        .iter()
        .filter(|source| source.status == "stale")
        .count();

    Ok(StorySearchStatus {
        project_id,
        model_version: MODEL_VERSION.to_string(),
        model_status: EmbeddingRuntime::probe(state)
            .unwrap_or_else(|error| format!("unavailable: {error}")),
        sqlite_vec_status: vector_table_error
            .map(|error| format!("unavailable: {error}"))
            .unwrap_or_else(|| {
                sqlite_vec_status(state).unwrap_or_else(|error| format!("unavailable: {error}"))
            }),
        document_count,
        embedding_count,
        indexed_source_count: sources
            .iter()
            .filter(|source| matches!(source.status.as_str(), "success" | "fts_only"))
            .count(),
        last_indexed_at,
        stale: stale_sources > 0,
        stale_sources,
        sources,
    })
}

pub(crate) fn chunk_text(
    text: &str,
    max_chars: usize,
    overlap: usize,
) -> Vec<(usize, usize, String)> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let hard_end = (start + max_chars).min(chars.len());
        let min_break = (start + max_chars.saturating_mul(2) / 3).min(hard_end);
        let end = if hard_end == chars.len() {
            hard_end
        } else {
            (min_break..hard_end)
                .rev()
                .find(|&index| matches!(chars[index], '\n' | '。' | '！' | '？' | '；'))
                .map(|index| index + 1)
                .unwrap_or(hard_end)
        };

        let chunk = chars[start..end]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
        if !chunk.is_empty() {
            chunks.push((start, end, chunk));
        }
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(overlap).max(start + 1);
    }

    chunks
}

fn replace_source_with_runtime(state: &AppState, payload: &SearchSourcePayload) -> AppResult<()> {
    let runtime = EmbeddingRuntime::load(state);
    let runtime_error = runtime.as_ref().err().map(ToString::to_string);
    let vector_ready = runtime.as_ref().is_ok() && ensure_vector_table(state).is_ok();
    replace_source(
        state,
        payload,
        runtime.as_ref().ok(),
        vector_ready,
        runtime_error.as_deref(),
    )
}

fn replace_source(
    state: &AppState,
    payload: &SearchSourcePayload,
    runtime: Option<&EmbeddingRuntime>,
    vector_ready: bool,
    unavailable_reason: Option<&str>,
) -> AppResult<()> {
    let hash = source_hash(&payload.title, &payload.content);
    let chunks = build_chunks(payload);
    let embeddings = if let Some(runtime) = runtime.filter(|_| vector_ready) {
        chunks
            .iter()
            .map(|(_, _, _, content)| runtime.encode(content))
            .collect::<Result<Vec<_>, _>>()
            .ok()
    } else {
        None
    };

    let status = if embeddings.is_some() {
        "success"
    } else {
        "fts_only"
    };
    let error = if embeddings.is_some() {
        None
    } else {
        unavailable_reason.or(Some("本地嵌入模型或 sqlite-vec 不可用；已保留 FTS5 检索"))
    };

    state.with_conn(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if payload.source_kind == "chapter" {
                let current_artifact_id = conn
                    .query_row(
                        "SELECT current_artifact_id FROM chapters
                         WHERE id = ?1 AND project_id = ?2",
                        params![payload.source_id, payload.project_id],
                        |row| row.get::<_, Option<i64>>(0),
                    )?;
                if current_artifact_id != payload.source_artifact_id {
                    return Err(AppError::Validation(
                        "章节正式正文已切换，丢弃旧的搜索索引".to_string(),
                    ));
                }
            }
            delete_source(conn, payload.project_id, payload.source_kind, payload.source_id)?;
            let timestamp = now();
            for (index, (chunk_no, start, end, content)) in chunks.iter().enumerate() {
                let search_text = format!("{}\n{}\n{}", payload.search_label, payload.title, content);
                conn.execute(
                    "INSERT INTO story_search_documents
                     (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, title, content, search_text,
                      chunk_no, chunk_start, chunk_end, visibility_cutoff_chapter_no, source_text_hash, normalization_version, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                    params![
                        payload.project_id,
                        payload.source_kind,
                        payload.source_id,
                        payload.chapter_id,
                        payload.chapter_no_sort,
                        payload.stage,
                        payload.title,
                        content,
                        search_text,
                        chunk_no,
                        start,
                        end,
                        payload.chapter_no_sort,
                        hash,
                        NORMALIZATION_VERSION,
                        timestamp
                    ],
                )?;

                if let Some(vectors) = embeddings.as_ref() {
                    conn.execute(
                        "INSERT INTO story_search_embeddings(rowid, embedding) VALUES (?1, ?2)",
                        params![conn.last_insert_rowid(), serde_json::to_string(&vectors[index])?],
                    )?;
                }
            }

            conn.execute(
                "INSERT INTO story_search_sources
                 (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, source_artifact_id, source_text_hash,
                  normalization_version, status, error, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
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
                params![
                    payload.project_id,
                    payload.source_kind,
                    payload.source_id,
                    payload.chapter_id,
                    payload.chapter_no_sort,
                    payload.stage,
                    payload.source_artifact_id,
                    hash,
                    NORMALIZATION_VERSION,
                    status,
                    error,
                    timestamp
                ],
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
    })
}

fn collect_project_sources(
    state: &AppState,
    project_id: i64,
) -> AppResult<Vec<SearchSourcePayload>> {
    let mut sources = Vec::new();

    for chapter in state.list_chapters(project_id)? {
        if let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter.id)? {
            sources.push(SearchSourcePayload {
                project_id,
                source_kind: "chapter",
                source_id: chapter.id,
                chapter_id: Some(chapter.id),
                chapter_no_sort: Some(chapter.chapter_no),
                stage: Some(artifact.stage.clone()),
                source_artifact_id: Some(artifact.id),
                title: chapter.title.clone(),
                content: artifact.content,
                search_label: format!("第 {} 章 {}", chapter.chapter_no, chapter.title),
                chunk_max: 420,
                overlap: 60,
                single_document_if_short: false,
            });
        }
    }

    for stage in ["setting", "outline", "characters"] {
        if let Some(artifact) = state.approved_artifact(project_id, stage, None)? {
            sources.push(artifact_payload(
                project_id,
                stage,
                artifact.id,
                artifact.title,
                artifact.content,
            ));
        }
    }

    for card in state
        .list_knowledge_cards(project_id)?
        .into_iter()
        .filter(|card| card.status == "approved")
    {
        sources.push(SearchSourcePayload {
            project_id,
            source_kind: "knowledge_card",
            source_id: card.id,
            chapter_id: card.source_chapter_id,
            chapter_no_sort: chapter_no(state, project_id, card.source_chapter_id)?,
            stage: None,
            source_artifact_id: card.source_artifact_id,
            title: card.title.clone(),
            content: card.content,
            search_label: format!("知识卡：{}", card.title),
            chunk_max: 400,
            overlap: 60,
            single_document_if_short: true,
        });
    }

    for item in state
        .list_foreshadowings(project_id)?
        .into_iter()
        .filter(|item| {
            matches!(
                item.status.as_str(),
                "active" | "ready_for_payoff" | "approved"
            )
        })
    {
        sources.push(SearchSourcePayload {
            project_id,
            source_kind: "foreshadowing",
            source_id: item.id,
            chapter_id: item.planted_chapter_id,
            chapter_no_sort: chapter_no(state, project_id, item.planted_chapter_id)?,
            stage: None,
            source_artifact_id: item.source_artifact_id,
            title: item.title.clone(),
            content: item.content,
            search_label: format!("伏笔：{}", item.title),
            chunk_max: 400,
            overlap: 60,
            single_document_if_short: true,
        });
    }

    Ok(sources)
}

fn artifact_payload(
    project_id: i64,
    stage: &str,
    artifact_id: i64,
    title: String,
    content: String,
) -> SearchSourcePayload {
    SearchSourcePayload {
        project_id,
        source_kind: "artifact",
        source_id: artifact_id,
        chapter_id: None,
        chapter_no_sort: None,
        stage: Some(stage.to_string()),
        source_artifact_id: Some(artifact_id),
        title: title.clone(),
        content,
        search_label: format!("{}：{}", artifact_label(stage), title),
        chunk_max: 360,
        overlap: 50,
        single_document_if_short: false,
    }
}

fn clear_project_index(state: &AppState, project_id: i64) -> AppResult<()> {
    state.with_conn(|conn| {
        ensure_sqlite_vec_loaded_if_present_on_connection(state, conn)?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if table_exists(conn, "story_search_embeddings")? {
                conn.execute(
                    "DELETE FROM story_search_embeddings WHERE rowid IN
                     (SELECT id FROM story_search_documents WHERE project_id = ?1)",
                    params![project_id],
                )?;
            }
            conn.execute(
                "DELETE FROM story_search_documents WHERE project_id = ?1",
                params![project_id],
            )?;
            conn.execute(
                "DELETE FROM story_search_sources WHERE project_id = ?1",
                params![project_id],
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
    })
}

pub(crate) fn invalidate_chapters_from(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    state.with_conn(|conn| {
        ensure_sqlite_vec_loaded_if_present_on_connection(state, conn)?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = invalidate_chapters_from_tx(conn, project_id, chapter_id);
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

pub(crate) fn invalidate_chapters_from_tx(
    conn: &Connection,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    let chapter_no = conn
        .query_row(
            "SELECT chapter_no FROM chapters WHERE id = ?1 AND project_id = ?2",
            params![chapter_id, project_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings WHERE rowid IN
             (SELECT id FROM story_search_documents
              WHERE project_id = ?1 AND source_kind = 'chapter' AND chapter_no_sort >= ?2)",
            params![project_id, chapter_no],
        )?;
    }
    conn.execute(
        "DELETE FROM story_search_documents
         WHERE project_id = ?1 AND source_kind = 'chapter' AND chapter_no_sort >= ?2",
        params![project_id, chapter_no],
    )?;
    let timestamp = now();
    conn.execute(
        "UPDATE story_search_sources
         SET status = 'stale', error = '章节正文已变更，等待重建', indexed_at = ?1
         WHERE project_id = ?2 AND source_kind = 'chapter' AND chapter_no_sort >= ?3",
        params![&timestamp, project_id, chapter_no],
    )?;
    crate::index_jobs::enqueue_following_chapter_index_jobs_tx(
        conn, project_id, chapter_id, &timestamp,
    )?;
    Ok(())
}

fn delete_source(conn: &Connection, project_id: i64, kind: &str, source_id: i64) -> AppResult<()> {
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings WHERE rowid IN
             (SELECT id FROM story_search_documents
              WHERE project_id = ?1 AND source_kind = ?2 AND source_id = ?3)",
            params![project_id, kind, source_id],
        )?;
    }
    conn.execute(
        "DELETE FROM story_search_documents
         WHERE project_id = ?1 AND source_kind = ?2 AND source_id = ?3",
        params![project_id, kind, source_id],
    )?;
    conn.execute(
        "DELETE FROM story_search_sources
         WHERE project_id = ?1 AND source_kind = ?2 AND source_id = ?3",
        params![project_id, kind, source_id],
    )?;
    Ok(())
}

fn delete_source_kind_stage(
    conn: &Connection,
    project_id: i64,
    kind: &str,
    stage: &str,
) -> AppResult<()> {
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings WHERE rowid IN
             (SELECT id FROM story_search_documents
              WHERE project_id = ?1 AND source_kind = ?2 AND stage = ?3)",
            params![project_id, kind, stage],
        )?;
    }
    conn.execute(
        "DELETE FROM story_search_documents
         WHERE project_id = ?1 AND source_kind = ?2 AND stage = ?3",
        params![project_id, kind, stage],
    )?;
    conn.execute(
        "DELETE FROM story_search_sources
         WHERE project_id = ?1 AND source_kind = ?2 AND stage = ?3",
        params![project_id, kind, stage],
    )?;
    Ok(())
}

fn fts_candidates(
    state: &AppState,
    project_id: i64,
    upper_bound: Option<i64>,
    query: &str,
) -> AppResult<Vec<SearchDocument>> {
    state.with_conn(|conn| {
        let mut documents = Vec::new();

        if query.chars().count() >= 3 {
            let phrase = format!("\"{}\"", query.replace('"', " "));
            let mut stmt = conn.prepare(
                "SELECT d.id, d.source_kind, d.source_id, d.chapter_no_sort, d.title, d.content, d.search_text
                 FROM story_search_documents_fts f
                 INNER JOIN story_search_documents d ON d.id = f.rowid
                 WHERE f.search_text MATCH ?1
                   AND d.project_id = ?2
                   AND (d.chapter_no_sort IS NULL OR ?3 IS NULL OR d.chapter_no_sort <= ?3)
                 ORDER BY bm25(story_search_documents_fts)
                 LIMIT ?4",
            )?;
            let rows = stmt.query_map(params![phrase, project_id, upper_bound, FTS_LIMIT as i64], map_document)?;
            documents.extend(rows.collect::<Result<Vec<_>, _>>()?);
        }

        let mut stmt = conn.prepare(
            "SELECT id, source_kind, source_id, chapter_no_sort, title, content, search_text
             FROM story_search_documents
             WHERE project_id = ?1
               AND (chapter_no_sort IS NULL OR ?2 IS NULL OR chapter_no_sort <= ?2)
               AND instr(search_text, ?3) > 0
             ORDER BY chapter_no_sort DESC, id DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![project_id, upper_bound, query, FTS_LIMIT as i64], map_document)?;
        for document in rows.collect::<Result<Vec<_>, _>>()? {
            if !documents.iter().any(|existing| existing.id == document.id) {
                documents.push(document);
            }
        }

        documents.truncate(FTS_LIMIT);
        Ok(documents)
    })
}

fn vector_candidates(
    state: &AppState,
    project_id: i64,
    upper_bound: Option<i64>,
    vector: &[f32],
) -> AppResult<Vec<SearchDocument>> {
    let vector_json = serde_json::to_string(vector)?;
    state.with_conn(|conn| {
        let mut ids_stmt = conn.prepare(
            "SELECT rowid
             FROM story_search_embeddings
             WHERE embedding MATCH ?1
             ORDER BY distance
             LIMIT 240",
        )?;
        let ids = ids_stmt
            .query_map(params![vector_json], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut documents = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, source_kind, source_id, chapter_no_sort, title, content, search_text
             FROM story_search_documents
             WHERE id = ?1
               AND project_id = ?2
               AND (chapter_no_sort IS NULL OR ?3 IS NULL OR chapter_no_sort <= ?3)",
        )?;

        for id in ids {
            if let Some(document) = stmt
                .query_row(params![id, project_id, upper_bound], map_document)
                .optional()?
            {
                documents.push(document);
                if documents.len() == VECTOR_LIMIT {
                    break;
                }
            }
        }

        Ok(documents)
    })
}

fn add_rank(
    ranks: &mut HashMap<i64, RankState>,
    document: SearchDocument,
    rank: usize,
    query: &str,
) {
    let exact = document.search_text.contains(query);
    let state = ranks.entry(document.id).or_default();
    state.score += 1.0 / (RRF_K + rank as f64 + 1.0);
    if exact {
        state.score += 0.02;
        state.exact = true;
    }
    state.document = Some(document);
}

fn search_upper_bound(
    state: &AppState,
    project_id: i64,
    chapter_id: Option<i64>,
    include_immediate_previous: bool,
) -> AppResult<Option<i64>> {
    let Some(chapter_id) = chapter_id else {
        return Ok(None);
    };
    let chapter_no = state
        .list_chapters(project_id)?
        .into_iter()
        .find(|chapter| chapter.id == chapter_id)
        .map(|chapter| chapter.chapter_no);
    Ok(chapter_no.map(|no| {
        if include_immediate_previous {
            no.saturating_sub(1)
        } else {
            no.saturating_sub(2)
        }
    }))
}

fn build_chunks(payload: &SearchSourcePayload) -> Vec<(i64, i64, i64, String)> {
    let content = normalize_text(&payload.content);
    if payload.single_document_if_short && content.chars().count() <= 450 {
        return vec![(0, 0, content.chars().count() as i64, content)];
    }
    chunk_text(&content, payload.chunk_max, payload.overlap)
        .into_iter()
        .enumerate()
        .map(|(index, (start, end, chunk))| (index as i64, start as i64, end as i64, chunk))
        .collect()
}

fn source_hash(title: &str, content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize_text(title));
    hasher.update(b"\n");
    hasher.update(normalize_text(content));
    format!("{:x}", hasher.finalize())
}

fn normalize_text(value: &str) -> String {
    value.replace("\r\n", "\n").trim().to_string()
}

fn chapter_no(
    state: &AppState,
    project_id: i64,
    chapter_id: Option<i64>,
) -> AppResult<Option<i64>> {
    let Some(chapter_id) = chapter_id else {
        return Ok(None);
    };
    Ok(state
        .list_chapters(project_id)?
        .into_iter()
        .find(|chapter| chapter.id == chapter_id)
        .map(|chapter| chapter.chapter_no))
}

fn table_exists(conn: &Connection, table: &str) -> AppResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE name = ?1",
            params![table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn ensure_vector_table(state: &AppState) -> AppResult<()> {
    state.with_conn(|conn| {
        ensure_sqlite_vec_loaded(state, conn)?;
        let existing_sql = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'story_search_embeddings'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        if existing_sql
            .as_deref()
            .is_some_and(|sql| !vector_table_matches_dimensions(sql))
        {
            conn.execute_batch("DROP TABLE story_search_embeddings")?;
            conn.execute(
                "UPDATE story_search_sources
                 SET status = 'stale',
                     error = ?1,
                     indexed_at = ?2
                 WHERE status != 'stale'",
                params![
                    format!(
                        "本地嵌入模型已切换为 {MODEL_VERSION}（{VECTOR_DIMENSIONS} 维），请重建检索索引"
                    ),
                    now()
                ],
            )?;
        }
        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS story_search_embeddings USING vec0(embedding float[{VECTOR_DIMENSIONS}])"
        ))?;
        Ok(())
    })
}

fn vector_table_matches_dimensions(sql: &str) -> bool {
    sql.to_ascii_lowercase()
        .contains(&format!("float[{VECTOR_DIMENSIONS}]"))
}

fn sqlite_vec_status(state: &AppState) -> AppResult<String> {
    state.with_conn(|conn| {
        ensure_sqlite_vec_loaded(state, conn)?;
        conn.query_row("SELECT vec_version()", [], |row| row.get::<_, String>(0))
            .map_err(AppError::from)
    })
}

pub(crate) fn ensure_sqlite_vec_loaded(state: &AppState, conn: &Connection) -> AppResult<()> {
    if conn
        .query_row("SELECT vec_version()", [], |_| Ok(()))
        .is_ok()
    {
        return Ok(());
    }

    let extension = sqlite_vec_library_path(state)?;
    // Only a packaged absolute path is allowed. SQLite's SQL load_extension()
    // remains disabled before and after this narrow Rust-side load.
    unsafe {
        conn.load_extension_enable()?;
        let loaded = conn.load_extension(&extension, None);
        let disabled = conn.load_extension_disable();
        loaded?;
        disabled?;
    }
    conn.query_row("SELECT vec_version()", [], |_| Ok(()))?;
    Ok(())
}

pub(crate) fn ensure_sqlite_vec_loaded_if_present_on_connection(
    state: &AppState,
    conn: &Connection,
) -> AppResult<()> {
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
        ensure_sqlite_vec_loaded(state, conn)?;
    }
    Ok(())
}

pub(crate) fn ensure_sqlite_vec_loaded_if_present(state: &AppState) -> AppResult<()> {
    state.with_conn(|conn| ensure_sqlite_vec_loaded_if_present_on_connection(state, conn))
}

fn sqlite_vec_library_path(state: &AppState) -> AppResult<PathBuf> {
    let candidates = [
        "sqlite-vec/vec0.dylib",
        "sqlite-vec/macos/vec0.dylib",
        "sqlite-vec/macos-aarch64/vec0.dylib",
    ];
    for root in state.story_search_resource_roots() {
        for relative in candidates {
            let candidate = root.join(relative);
            if candidate.is_file() {
                return fs::canonicalize(candidate).map_err(AppError::from);
            }
        }
    }
    Err(AppError::Validation(
        "缺少 sqlite-vec 扩展资源；本地语义检索不可用".to_string(),
    ))
}

fn map_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchDocument> {
    Ok(SearchDocument {
        id: row.get(0)?,
        source_kind: row.get(1)?,
        source_id: row.get(2)?,
        chapter_no_sort: row.get(3)?,
        title: row.get(4)?,
        content: row.get(5)?,
        search_text: row.get(6)?,
    })
}

fn source_label(document: &SearchDocument) -> String {
    match document.source_kind.as_str() {
        "chapter" => format!(
            "第 {} 章 {}",
            document.chapter_no_sort.unwrap_or_default(),
            document.title
        ),
        "artifact" => format!("资料：{}", document.title),
        "knowledge_card" => format!("知识卡：{}", document.title),
        "foreshadowing" => format!("伏笔：{}", document.title),
        _ => document.title.clone(),
    }
}

fn matched_term(query: &str, document: &SearchDocument) -> String {
    if document.search_text.contains(query) {
        query.to_string()
    } else {
        document.title.clone()
    }
}

fn same_content(left: &str, right: &str) -> bool {
    left == right
        || left.chars().take(80).collect::<String>() == right.chars().take(80).collect::<String>()
}

fn artifact_label(stage: &str) -> &'static str {
    match stage {
        "setting" => "设定",
        "outline" => "大纲",
        "characters" => "角色",
        _ => "资料",
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl EmbeddingRuntime {
    fn probe(state: &AppState) -> Result<String, AppError> {
        let directory = model_directory(state)?;
        verify_model_package(&directory)?;
        Ok("available (lazy load)".to_string())
    }

    fn load(state: &AppState) -> Result<Self, AppError> {
        let directory = model_directory(state)?;
        verify_model_package(&directory)?;
        let device = embedding_device();
        let config: BertConfig = serde_json::from_slice(&fs::read(directory.join("config.json"))?)?;
        let tokenizer =
            Tokenizer::from_file(directory.join("tokenizer.json")).map_err(|error| {
                AppError::Validation(format!("本地检索 tokenizer 无法加载：{error}"))
            })?;
        let weights = directory.join("pytorch_model.bin");
        let builder = VarBuilder::from_pth(&weights, DType::F32, &device)
            .map_err(|error| AppError::Validation(format!("本地检索模型权重无法加载：{error}")))?;
        let model = BertModel::load(builder, &config)
            .map_err(|error| AppError::Validation(format!("本地检索模型结构不兼容：{error}")))?;
        Ok(Self {
            tokenizer,
            model,
            device,
        })
    }

    fn encode(&self, text: &str) -> Result<Vec<f32>, AppError> {
        let encoding = self.tokenizer.encode(text, true).map_err(|error| {
            AppError::Validation(format!("本地检索 tokenizer 编码失败：{error}"))
        })?;

        let ids = encoding
            .get_ids()
            .iter()
            .take(512)
            .copied()
            .collect::<Vec<_>>();
        let type_ids = encoding
            .get_type_ids()
            .iter()
            .take(ids.len())
            .copied()
            .collect::<Vec<_>>();
        let mask = encoding
            .get_attention_mask()
            .iter()
            .take(ids.len())
            .copied()
            .collect::<Vec<_>>();

        let input_ids = Tensor::new(ids.as_slice(), &self.device)
            .and_then(|value| value.unsqueeze(0))
            .map_err(candle_error)?;
        let token_type_ids = Tensor::new(type_ids.as_slice(), &self.device)
            .and_then(|value| value.unsqueeze(0))
            .map_err(candle_error)?;
        let attention_mask = Tensor::new(mask.as_slice(), &self.device)
            .and_then(|value| value.unsqueeze(0))
            .map_err(candle_error)?;

        let cls = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))
            .and_then(|value| value.i((0, 0)))
            .map_err(candle_error)?;

        let norm = cls
            .sqr()
            .and_then(|value| value.sum_all())
            .and_then(|value| value.sqrt())
            .map_err(candle_error)?;

        cls.broadcast_div(&norm)
            .and_then(|value| value.to_vec1::<f32>())
            .map_err(candle_error)
    }
}

fn embedding_device() -> Device {
    Device::new_metal(0).unwrap_or(Device::Cpu)
}

fn model_directory(state: &AppState) -> AppResult<PathBuf> {
    state
        .story_search_resource_roots()
        .iter()
        .map(|root| root.join("models").join(MODEL_VERSION))
        .find(|path| path.exists())
        .ok_or_else(|| AppError::Validation(format!("缺少本地检索模型资源 models/{MODEL_VERSION}")))
}

fn verify_model_package(directory: &Path) -> AppResult<()> {
    for file in [
        "config.json",
        "tokenizer.json",
        "special_tokens_map.json",
        "manifest.json",
    ] {
        if !directory.join(file).is_file() {
            return Err(AppError::Validation(format!(
                "本地检索模型资源不完整：缺少 {file}"
            )));
        }
    }
    if !directory.join("pytorch_model.bin").is_file() {
        return Err(AppError::Validation(
            "本地检索模型资源不完整：缺少 pytorch_model.bin".to_string(),
        ));
    }

    let manifest: ModelManifest =
        serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
    for (file, expected) in manifest.checksums {
        let actual = format!("{:x}", Sha256::digest(fs::read(directory.join(&file))?));
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(AppError::Validation(format!(
                "本地检索模型校验失败：{file}"
            )));
        }
    }
    Ok(())
}

fn candle_error(error: candle_core::Error) -> AppError {
    AppError::Validation(format!("本地检索推理失败：{error}"))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, process::Command, thread, time::Duration};

    use super::{
        chunk_text, get_story_search_status, invalidate_chapters_from, search_story_context,
        EmbeddingRuntime, NORMALIZATION_VERSION,
    };
    use crate::{db::AppState, models::StoryContextSearchInput};
    use rusqlite::params;
    use tempfile::tempdir;

    #[test]
    fn chunking_is_stable_and_overlaps() {
        let text = "甲".repeat(500);
        let chunks = chunk_text(&text, 300, 60);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].0, 0);
        assert_eq!(chunks[0].1, 300);
        assert_eq!(chunks[1].0, 240);
    }

    fn seeded_state() -> (tempfile::TempDir, AppState, i64, [i64; 3]) {
        let dir = tempdir().unwrap();
        let state = AppState::from_path(PathBuf::from(dir.path()).join("search.sqlite3")).unwrap();
        let project_id = state
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO projects (title, genre, target_words, premise, status, created_at, updated_at)
                     VALUES ('检索测试', '玄幻', 100000, '', 'active', 'now', 'now')",
                    [],
                )?;
                let project_id = conn.last_insert_rowid();
                for chapter_no in 1..=3 {
                    conn.execute(
                        "INSERT INTO chapters (project_id, chapter_no, title, status, created_at, updated_at)
                         VALUES (?1, ?2, ?3, 'approved', 'now', 'now')",
                        params![project_id, chapter_no, format!("第 {chapter_no} 章")],
                    )?;
                }
                Ok(project_id)
            })
            .unwrap();
        let mut chapter_ids = state
            .list_chapters(project_id)
            .unwrap()
            .into_iter()
            .map(|chapter| chapter.id);
        (
            dir,
            state,
            project_id,
            [
                chapter_ids.next().unwrap(),
                chapter_ids.next().unwrap(),
                chapter_ids.next().unwrap(),
            ],
        )
    }

    fn insert_document(
        state: &AppState,
        project_id: i64,
        chapter_id: i64,
        chapter_no: i64,
        source_id: i64,
        content: &str,
    ) {
        state
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO story_search_documents
                     (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, title, content, search_text,
                      chunk_no, chunk_start, chunk_end, visibility_cutoff_chapter_no, source_text_hash, normalization_version, updated_at)
                     VALUES (?1, 'chapter', ?2, ?3, ?4, 'draft', '正文', ?5, ?5, 0, 0, 10, ?4, 'hash', ?6, 'now')",
                    params![project_id, source_id, chapter_id, chapter_no, content, NORMALIZATION_VERSION],
                )?;
                conn.execute(
                    "INSERT INTO story_search_sources
                     (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, source_artifact_id,
                      source_text_hash, normalization_version, status, error, indexed_at)
                     VALUES (?1, 'chapter', ?2, ?3, ?4, 'draft', NULL, 'hash', ?5, 'success', NULL, 'now')",
                    params![project_id, source_id, chapter_id, chapter_no, NORMALIZATION_VERSION],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn migration_creates_fts_and_chinese_search_respects_history_boundary() {
        let (_dir, state, project_id, chapter_ids) = seeded_state();
        insert_document(
            &state,
            project_id,
            chapter_ids[0],
            1,
            101,
            "宁烬在黑石矿场取得玄铁令。",
        );
        insert_document(
            &state,
            project_id,
            chapter_ids[1],
            2,
            102,
            "宁烬把玄铁令交给了林月。",
        );
        let rows = search_story_context(
            &state,
            &StoryContextSearchInput {
                project_id,
                chapter_id: Some(chapter_ids[2]),
                query: "玄铁令".to_string(),
                limit: Some(6),
                include_immediate_previous: false,
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].content.contains("黑石矿场"));
        assert!(!rows[0].content.contains("交给了林月"));
    }

    #[test]
    fn chapter_invalidation_marks_current_and_future_sources_stale() {
        let (_dir, state, project_id, chapter_ids) = seeded_state();
        insert_document(&state, project_id, chapter_ids[0], 1, 101, "第一章");
        insert_document(&state, project_id, chapter_ids[1], 2, 102, "第二章");
        invalidate_chapters_from(&state, project_id, chapter_ids[1]).unwrap();
        let sources = state.list_story_search_sources(project_id).unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].status, "success");
        assert_eq!(sources[1].status, "stale");
    }

    #[test]
    fn chapter_invalidation_queues_following_approved_chapters() {
        let (_dir, state, project_id, chapter_ids) = seeded_state();
        let first = state
            .insert_artifact(
                project_id,
                Some(chapter_ids[0]),
                "draft",
                "第一章正文",
                "第一章正文",
                None,
            )
            .unwrap();
        let second = state
            .insert_artifact(
                project_id,
                Some(chapter_ids[1]),
                "draft",
                "第二章正文",
                "第二章正文",
                None,
            )
            .unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE chapters
                     SET current_artifact_id = ?1, status = 'approved'
                     WHERE id = ?2",
                    params![first.id, chapter_ids[0]],
                )?;
                conn.execute(
                    "UPDATE chapters
                     SET current_artifact_id = ?1, status = 'approved'
                     WHERE id = ?2",
                    params![second.id, chapter_ids[1]],
                )?;
                Ok(())
            })
            .unwrap();

        invalidate_chapters_from(&state, project_id, chapter_ids[0]).unwrap();
        let jobs = state.list_derived_index_jobs(project_id).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs
            .iter()
            .all(|job| job.chapter_id == Some(chapter_ids[1])));
        assert!(jobs
            .iter()
            .all(|job| job.source_artifact_id == Some(second.id)));
    }

    #[test]
    fn deleting_a_middle_chapter_clears_future_fts_vectors_and_queues_rebuild() {
        let (_dir, state, project_id, chapter_ids) = seeded_state();
        state
            .with_conn(|conn| {
                conn.execute("CREATE TABLE story_search_embeddings (embedding TEXT)", [])?;
                Ok(())
            })
            .unwrap();
        insert_document(&state, project_id, chapter_ids[0], 1, 101, "第一章保留词");
        insert_document(&state, project_id, chapter_ids[1], 2, 102, "第二章删除词");
        insert_document(&state, project_id, chapter_ids[2], 3, 103, "第三章后续词");
        state
            .with_conn(|conn| {
                let ids = conn
                    .prepare("SELECT id FROM story_search_documents ORDER BY id")?
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                for id in ids {
                    conn.execute(
                        "INSERT INTO story_search_embeddings(rowid, embedding) VALUES (?1, 'vector')",
                        params![id],
                    )?;
                }
                Ok(())
            })
            .unwrap();

        state.delete_chapter(project_id, chapter_ids[1]).unwrap();
        let counts = state
            .with_conn(|conn| {
                let documents: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM story_search_documents WHERE project_id = ?1",
                    params![project_id],
                    |row| row.get(0),
                )?;
                let vectors: i64 =
                    conn.query_row("SELECT COUNT(*) FROM story_search_embeddings", [], |row| {
                        row.get(0)
                    })?;
                let fts: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM story_search_documents_fts
                     WHERE story_search_documents_fts MATCH '后续词'",
                    [],
                    |row| row.get(0),
                )?;
                Ok((documents, vectors, fts))
            })
            .unwrap();
        assert_eq!(counts, (1, 1, 0));
        assert_eq!(
            state.list_story_search_sources(project_id).unwrap().len(),
            1
        );
        let jobs = state.list_derived_index_jobs(project_id).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_type, crate::index_jobs::SEARCH_PROJECT_JOB);
        assert_eq!(jobs[0].status, "pending");
    }

    #[test]
    fn renaming_a_chapter_refreshes_search_metadata_without_reembedding() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(crate::models::NewProject {
                title: "重命名检索".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let artifact = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "正文",
                "保留原文内容",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", artifact.id, "通过")
            .unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO story_search_documents
                        (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, title,
                         content, search_text, chunk_no, chunk_start, chunk_end,
                         visibility_cutoff_chapter_no, source_text_hash, normalization_version, updated_at)
                     VALUES (?1, 'chapter', ?2, ?2, 1, 'draft', '旧标题', '保留原文内容',
                             '第 1 章 旧标题\n旧标题\n保留原文内容', 0, 0, 6, 1, 'old-hash', ?3, 'now')",
                    params![project.id, chapter.id, NORMALIZATION_VERSION],
                )?;
                conn.execute(
                    "INSERT INTO story_search_sources
                        (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage,
                         source_artifact_id, source_text_hash, normalization_version, status, error, indexed_at)
                     VALUES (?1, 'chapter', ?2, ?2, 1, 'draft', ?3, 'old-hash', ?4, 'success', NULL, 'now')",
                    params![project.id, chapter.id, artifact.id, NORMALIZATION_VERSION],
                )?;
                Ok(())
            })
            .unwrap();

        state
            .update_chapter(crate::models::ChapterUpdate {
                id: chapter.id,
                title: "新标题".to_string(),
                status: "approved".to_string(),
            })
            .unwrap();
        let updated = state
            .ensure_chapter(project.id, Some(chapter.id))
            .unwrap()
            .unwrap();
        super::refresh_chapter_metadata(&state, updated.project_id, updated.id).unwrap();

        let (title, search_text, hash, embedding_count) = state
            .with_conn(|conn| {
                let metadata = conn.query_row(
                    "SELECT title, search_text, source_text_hash
                     FROM story_search_documents WHERE project_id = ?1 AND source_kind = 'chapter' AND source_id = ?2",
                    params![project.id, chapter.id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )?;
                let embedding_count = if super::table_exists(conn, "story_search_embeddings")? {
                    conn.query_row("SELECT COUNT(*) FROM story_search_embeddings", [], |row| row.get(0))?
                } else {
                    0
                };
                Ok((metadata.0, metadata.1, metadata.2, embedding_count))
            })
            .unwrap();
        assert_eq!(title, "新标题");
        assert!(search_text.contains("第 1 章 新标题"));
        assert!(search_text.contains("新标题"));
        assert_eq!(hash, super::source_hash("新标题", "保留原文内容"));
        assert_eq!(embedding_count, 0);
    }

    #[test]
    fn absent_model_and_extension_are_reported_without_breaking_status() {
        let (_dir, state, project_id, _) = seeded_state();
        let status = get_story_search_status(&state, project_id).unwrap();
        assert!(
            status.model_status == "available (lazy load)"
                || status.model_status.starts_with("unavailable:")
        );
        assert!(
            status.sqlite_vec_status.starts_with("unavailable:")
                || status.sqlite_vec_status.starts_with('v')
        );
    }

    #[test]
    #[ignore = "manual measurement probe"]
    fn manual_embedding_runtime_load_reports_rss() {
        let dir = tempdir().unwrap();
        let state = AppState::from_path(PathBuf::from(dir.path()).join("search.sqlite3")).unwrap();

        let baseline = current_rss_kb();
        let runtime = EmbeddingRuntime::load(&state).expect("runtime should load");
        let after_load = current_rss_kb();
        let vector = runtime
            .encode("测试本地混合检索内存占用")
            .expect("encoding should succeed");
        let after_encode = current_rss_kb();
        assert_eq!(vector.len(), 512);

        drop(runtime);
        thread::sleep(Duration::from_millis(500));
        let after_drop = current_rss_kb();

        eprintln!(
            "RSS_KB baseline={} after_load={} after_encode={} after_drop={}",
            baseline, after_load, after_encode, after_drop
        );
    }

    fn current_rss_kb() -> usize {
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .expect("ps should succeed");
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<usize>()
            .expect("rss should parse")
    }

    #[test]
    fn old_base_vector_table_is_not_accepted_for_small_model() {
        assert!(!super::vector_table_matches_dimensions(
            "CREATE VIRTUAL TABLE story_search_embeddings USING vec0(embedding float[768])"
        ));
        assert!(super::vector_table_matches_dimensions(
            "CREATE VIRTUAL TABLE story_search_embeddings USING vec0(embedding float[512])"
        ));
    }
}
