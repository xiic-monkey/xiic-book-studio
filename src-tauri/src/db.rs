use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use chrono::Utc;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension};
use tauri::{AppHandle, Manager};
use tokio::sync::{broadcast, Notify};

use crate::{
    agent_tools,
    error::{AppError, AppResult},
    genre_agent, genre_skill,
    models::{
        Agent, AiProvider, AiSettings, Approval, Artifact, ArtifactFilters, Chapter,
        ChapterMemoryRecord, ChapterUpdate, ContinuityLedgerEntry, DerivedIndexJob, Foreshadowing,
        GenreAgentProfile, ImportReferenceTextRequest, KnowledgeCard, Message, NewChapter,
        NewProject, Project, ProjectDetail, ProjectUpdate, ReferenceMaterial, RunEvent,
        SaveAgentSettings, SaveAiProvider, SaveAiSettings, SaveForeshadowing, SaveKnowledgeCard,
        SaveWritingSkill, StoryArc, StoryBible, StoryBibleReview, StoryEntity, StoryEvent,
        StoryEventParticipant, StoryFact, StoryIndexSource, StorySearchSource, StoryThread,
        UpdateReferenceMaterialRequest, WorkflowRun, WritingSkill,
    },
    reference::ReferenceStore,
    workflow,
};

mod lifecycle;

// Every AI-capable role resolves to one row. Empty per-agent values inherit the
// global settings, which keeps existing databases and projects working.
const AGENT_SELECT_SQL: &str = r#"SELECT a.id, a.stage, a.name, a.role, a.system_prompt, a.temperature,
    COALESCE(NULLIF(a.provider_base_url, ''), (SELECT value FROM settings WHERE key = 'ai.base_url'), '') AS provider_base_url,
    COALESCE(NULLIF(a.model, ''), (SELECT value FROM settings WHERE key = 'ai.model'), '') AS model,
    CASE
        WHEN NULLIF(a.provider_base_url, '') IS NULL AND NULLIF(a.model, '') IS NULL THEN
            CASE WHEN COALESCE((SELECT value FROM settings WHERE key = 'ai.thinking_enabled'), 'false') IN ('true', '1')
                 THEN 1 ELSE 0 END
        ELSE a.thinking_enabled
    END AS thinking_enabled,
    CASE
        WHEN NULLIF(a.provider_base_url, '') IS NULL AND NULLIF(a.model, '') IS NULL THEN
            COALESCE((SELECT value FROM settings WHERE key = 'ai.thinking_level'), 'off')
        WHEN a.thinking_enabled = 1 THEN COALESCE(NULLIF(a.thinking_level, ''), 'medium')
        ELSE 'off'
    END AS thinking_level,
    CASE
        WHEN NULLIF(a.provider_base_url, '') IS NULL AND NULLIF(a.model, '') IS NULL THEN 1
        ELSE 0
    END AS uses_global_runtime_settings,
    COALESCE(NULLIF(a.enabled_tool_keys, ''), '["history_context","reference_materials","chapter_memory","continuity_check","quality_analysis","chapter_split","web_search"]') AS enabled_tool_keys,
    COALESCE(NULLIF(a.allowed_skill_keys, ''), '["continuity_and_agency"]') AS allowed_skill_keys
    FROM agents a"#;

#[derive(Clone)]
pub struct AppState {
    pool: Arc<Pool<SqliteConnectionManager>>,
    resource_roots: Arc<Vec<PathBuf>>,
    reference_store: Arc<Mutex<ReferenceStore>>,
    index_worker_started: Arc<AtomicBool>,
    index_worker_notify: Arc<Notify>,
    pub(crate) run_event_tx: broadcast::Sender<RunEvent>,
}

impl AppState {
    pub fn new(app: &AppHandle) -> AppResult<Self> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|err| AppError::Validation(format!("cannot resolve app data dir: {err}")))?;
        fs::create_dir_all(&data_dir)?;
        let mut resource_roots = Vec::new();
        if let Ok(resource_dir) = app.path().resource_dir() {
            resource_roots.push(resource_dir);
        }
        let legacy_path = data_dir.join("book-studio.sqlite3");
        let v2_path = data_dir.join("book-studio-v2.sqlite3");
        let first_v2_start = !v2_path.exists();
        if first_v2_start && legacy_path.is_file() {
            crate::v2_storage::backup_legacy_database(&legacy_path)?;
        }
        let state = match Self::from_path_with_resources(v2_path.clone(), resource_roots) {
            Ok(state) => state,
            Err(error) => {
                if first_v2_start {
                    crate::v2_storage::remove_new_database_files(&v2_path);
                }
                return Err(error);
            }
        };
        if first_v2_start && legacy_path.is_file() {
            if let Err(error) = crate::v2_storage::import_legacy_configuration(&state, &legacy_path)
            {
                drop(state);
                crate::v2_storage::remove_new_database_files(&v2_path);
                return Err(error);
            }
        }
        state.migrate_legacy_api_keys()?;
        Ok(state)
    }

    pub fn from_path(path: PathBuf) -> AppResult<Self> {
        Self::from_path_with_resources(path, Vec::new())
    }

    pub fn migrate_legacy_api_keys(&self) -> AppResult<()> {
        lifecycle::migrate_legacy_api_keys(self)
    }

    fn from_path_with_resources(
        path: PathBuf,
        mut resource_roots: Vec<PathBuf>,
    ) -> AppResult<Self> {
        resource_roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
        let manager = SqliteConnectionManager::file(path).with_init(|conn| {
            conn.busy_timeout(Duration::from_secs(5))?;
            conn.pragma_update(None, "foreign_keys", "ON")?;
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
            Ok(())
        });
        let pool = Pool::builder()
            .max_size(6)
            .connection_timeout(Duration::from_secs(5))
            .build(manager)
            .map_err(|error| {
                AppError::Validation(format!("cannot create SQLite connection pool: {error}"))
            })?;
        let (run_event_tx, _) = broadcast::channel(512);
        let state = Self {
            pool: Arc::new(pool),
            resource_roots: Arc::new(resource_roots),
            reference_store: Arc::new(Mutex::new(ReferenceStore::default())),
            index_worker_started: Arc::new(AtomicBool::new(false)),
            index_worker_notify: Arc::new(Notify::new()),
            run_event_tx,
        };
        lifecycle::initialize(&state)?;
        Ok(state)
    }

    pub(crate) fn story_search_resource_roots(&self) -> &[PathBuf] {
        self.resource_roots.as_slice()
    }

    pub(crate) fn bundled_resource_roots(&self) -> &[PathBuf] {
        self.resource_roots.as_slice()
    }

    pub(crate) fn start_index_worker(&self) {
        if self.index_worker_started.swap(true, Ordering::AcqRel) {
            return;
        }
        crate::index_jobs::start_worker(self.clone());
    }

    pub(crate) fn wake_index_worker(&self) {
        self.index_worker_notify.notify_one();
    }

    pub(crate) async fn wait_for_index_work(&self) {
        self.index_worker_notify.notified().await;
    }

    pub fn subscribe_run_events(&self) -> broadcast::Receiver<RunEvent> {
        self.run_event_tx.subscribe()
    }

    pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let conn = self.pool.get().map_err(|error| {
            AppError::Validation(format!("cannot acquire SQLite connection: {error}"))
        })?;
        crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, &conn)?;
        f(&conn)
    }

    pub(crate) fn with_reference_store<T>(
        &self,
        f: impl FnOnce(&mut ReferenceStore) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut store = self
            .reference_store
            .lock()
            .map_err(|_| AppError::Validation("临时参考资料状态锁已损坏".to_string()))?;
        f(&mut store)
    }

    pub fn import_reference_text(
        &self,
        input: ImportReferenceTextRequest,
    ) -> AppResult<ReferenceMaterial> {
        crate::reference::import_reference_text(
            self,
            input.project_id,
            &input.file_name,
            &input.content,
            input.tags,
        )
    }

    pub fn list_reference_materials(&self, project_id: i64) -> AppResult<Vec<ReferenceMaterial>> {
        crate::reference::list_reference_materials(self, project_id)
    }

    pub fn update_reference_material(
        &self,
        input: UpdateReferenceMaterialRequest,
    ) -> AppResult<ReferenceMaterial> {
        crate::reference::update_reference_material(
            self,
            input.project_id,
            input.reference_id,
            input.enabled,
            input.tags,
        )
    }

    pub fn remove_reference_material(&self, project_id: i64, reference_id: u64) -> AppResult<()> {
        crate::reference::remove_reference_material(self, project_id, reference_id)
    }

    pub fn clear_reference_materials(&self, project_id: i64) {
        crate::reference::clear_project(self, project_id);
    }

    pub(super) fn migrate(&self) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS projects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    genre TEXT NOT NULL,
                    target_words INTEGER NOT NULL DEFAULT 100000,
                    premise TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT 'active',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS agents (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    stage TEXT NOT NULL UNIQUE,
                    name TEXT NOT NULL,
                    role TEXT NOT NULL,
                    system_prompt TEXT NOT NULL,
                    temperature REAL NOT NULL DEFAULT 0.75,
                    provider_base_url TEXT NOT NULL DEFAULT '',
                    model TEXT NOT NULL DEFAULT '',
                    thinking_enabled INTEGER NOT NULL DEFAULT 0,
                    thinking_level TEXT NOT NULL DEFAULT 'off',
                    enabled_tool_keys TEXT NOT NULL DEFAULT '["history_context","reference_materials","chapter_memory","continuity_check","quality_analysis","chapter_split","web_search"]',
                    allowed_skill_keys TEXT NOT NULL DEFAULT '["continuity_and_agency"]'
                );

                CREATE TABLE IF NOT EXISTS chapters (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_no INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'planning',
                    current_artifact_id INTEGER,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(project_id, chapter_no),
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(current_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
                );

                CREATE TABLE IF NOT EXISTS artifacts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER,
                    stage TEXT NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    version INTEGER NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending_review',
                    parent_artifact_id INTEGER,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
                    FOREIGN KEY(parent_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
                );

                CREATE TABLE IF NOT EXISTS workflow_runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER,
                    stage TEXT NOT NULL,
                    input TEXT NOT NULL,
                    output TEXT NOT NULL,
                    status TEXT NOT NULL,
                    error TEXT,
                    elapsed_ms INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS approvals (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER,
                    stage TEXT NOT NULL,
                    artifact_id INTEGER NOT NULL,
                    note TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
                    FOREIGN KEY(artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS ai_providers (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    label TEXT NOT NULL,
                    base_url TEXT NOT NULL UNIQUE,
                    model TEXT NOT NULL,
                    temperature REAL NOT NULL DEFAULT 0.75,
                    thinking_enabled INTEGER NOT NULL DEFAULT 0,
                    thinking_level TEXT NOT NULL DEFAULT 'off',
                    api_key TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS story_threads (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    thread_key TEXT NOT NULL,
                    label TEXT NOT NULL,
                    kind TEXT NOT NULL DEFAULT 'fact',
                    status TEXT NOT NULL DEFAULT 'active',
                    current_cost TEXT,
                    last_seen_chapter_no INTEGER,
                    last_artifact_id INTEGER,
                    notes TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(project_id, thread_key),
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(last_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
                );

                CREATE TABLE IF NOT EXISTS writing_skills (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    skill_key TEXT NOT NULL UNIQUE,
                    name TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT 'genre',
                    description TEXT NOT NULL DEFAULT '',
                    content TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS project_genre_agents (
                    project_id INTEGER PRIMARY KEY,
                    agent_key TEXT NOT NULL,
                    assigned_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS knowledge_cards (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    category TEXT NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending_human_approval',
                    source_artifact_id INTEGER,
                    source_chapter_id INTEGER,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL,
                    FOREIGN KEY(source_chapter_id) REFERENCES chapters(id) ON DELETE SET NULL
                );

                CREATE TABLE IF NOT EXISTS foreshadowings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending_human_approval',
                    planted_chapter_id INTEGER,
                    planned_payoff_chapter_id INTEGER,
                    planned_payoff_note TEXT NOT NULL DEFAULT '',
                    source_artifact_id INTEGER,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(planted_chapter_id) REFERENCES chapters(id) ON DELETE SET NULL,
                    FOREIGN KEY(planned_payoff_chapter_id) REFERENCES chapters(id) ON DELETE SET NULL,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
                );

                CREATE TABLE IF NOT EXISTS chapter_memories (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER NOT NULL UNIQUE,
                    source_artifact_id INTEGER NOT NULL,
                    source_text_hash TEXT NOT NULL,
                    normalization_version TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS continuity_ledger_sources (
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER NOT NULL UNIQUE,
                    source_artifact_id INTEGER NOT NULL,
                    source_text_hash TEXT NOT NULL,
                    normalization_version TEXT NOT NULL,
                    built_at TEXT NOT NULL,
                    PRIMARY KEY(project_id, chapter_id),
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS continuity_ledger_entries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER NOT NULL,
                    source_artifact_id INTEGER NOT NULL,
                    source_text_hash TEXT NOT NULL,
                    normalization_version TEXT NOT NULL,
                    entity_kind TEXT NOT NULL,
                    entity_key TEXT NOT NULL,
                    entity_label TEXT NOT NULL,
                    state_kind TEXT NOT NULL,
                    state_value TEXT NOT NULL,
                    evidence_quote TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_continuity_ledger_lookup
                    ON continuity_ledger_entries(project_id, entity_key, state_kind, chapter_id, id);

                CREATE TABLE IF NOT EXISTS story_entities (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    kind TEXT NOT NULL,
                    name TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'active',
                    first_seen_chapter_id INTEGER,
                    source_artifact_id INTEGER,
                    source_quote TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(project_id, kind, name),
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(first_seen_chapter_id) REFERENCES chapters(id) ON DELETE SET NULL,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
                );
                CREATE INDEX IF NOT EXISTS idx_story_entities_project_kind
                    ON story_entities(project_id, kind, name);

                CREATE TABLE IF NOT EXISTS story_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'occurred',
                    story_time TEXT NOT NULL DEFAULT '',
                    summary TEXT NOT NULL DEFAULT '',
                    narrative_chapter_id INTEGER,
                    source_artifact_id INTEGER NOT NULL,
                    source_quote TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(narrative_chapter_id) REFERENCES chapters(id) ON DELETE SET NULL,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_story_events_project_chapter
                    ON story_events(project_id, narrative_chapter_id, id);

                CREATE TABLE IF NOT EXISTS story_event_participants (
                    event_id INTEGER NOT NULL,
                    entity_id INTEGER NOT NULL,
                    role TEXT NOT NULL,
                    PRIMARY KEY(event_id, entity_id, role),
                    FOREIGN KEY(event_id) REFERENCES story_events(id) ON DELETE CASCADE,
                    FOREIGN KEY(entity_id) REFERENCES story_entities(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS story_facts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    entity_id INTEGER NOT NULL,
                    event_id INTEGER,
                    dimension TEXT NOT NULL,
                    value TEXT NOT NULL,
                    visibility TEXT NOT NULL DEFAULT 'world',
                    status TEXT NOT NULL DEFAULT 'active',
                    narrative_chapter_id INTEGER,
                    source_artifact_id INTEGER NOT NULL,
                    source_quote TEXT NOT NULL,
                    supersedes_fact_id INTEGER,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(entity_id) REFERENCES story_entities(id) ON DELETE CASCADE,
                    FOREIGN KEY(event_id) REFERENCES story_events(id) ON DELETE SET NULL,
                    FOREIGN KEY(narrative_chapter_id) REFERENCES chapters(id) ON DELETE SET NULL,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE,
                    FOREIGN KEY(supersedes_fact_id) REFERENCES story_facts(id) ON DELETE SET NULL
                );
                CREATE INDEX IF NOT EXISTS idx_story_facts_entity_dimension
                    ON story_facts(project_id, entity_id, dimension, narrative_chapter_id, id);

                CREATE TABLE IF NOT EXISTS story_index_sources (
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER NOT NULL,
                    source_artifact_id INTEGER NOT NULL,
                    source_text_hash TEXT NOT NULL,
                    normalization_version TEXT NOT NULL,
                    status TEXT NOT NULL,
                    error TEXT,
                    indexed_at TEXT NOT NULL,
                    PRIMARY KEY(project_id, chapter_id),
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS story_search_documents (
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
                );
                CREATE INDEX IF NOT EXISTS idx_story_search_documents_source
                    ON story_search_documents(project_id, source_kind, source_id, chunk_no);
                CREATE INDEX IF NOT EXISTS idx_story_search_documents_chapter
                    ON story_search_documents(project_id, chapter_no_sort, source_kind);

                CREATE TABLE IF NOT EXISTS story_search_document_terms (
                    project_id INTEGER NOT NULL,
                    document_id INTEGER NOT NULL,
                    term TEXT NOT NULL,
                    PRIMARY KEY(project_id, term, document_id),
                    FOREIGN KEY(document_id) REFERENCES story_search_documents(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_story_search_document_terms_document
                    ON story_search_document_terms(document_id);

                CREATE VIRTUAL TABLE IF NOT EXISTS story_search_documents_fts
                    USING fts5(search_text, content='story_search_documents', content_rowid='id', tokenize='trigram');

                CREATE TRIGGER IF NOT EXISTS story_search_documents_ai AFTER INSERT ON story_search_documents BEGIN
                    INSERT INTO story_search_documents_fts(rowid, search_text) VALUES (new.id, new.search_text);
                END;
                CREATE TRIGGER IF NOT EXISTS story_search_documents_ad AFTER DELETE ON story_search_documents BEGIN
                    INSERT INTO story_search_documents_fts(story_search_documents_fts, rowid, search_text)
                    VALUES ('delete', old.id, old.search_text);
                END;
                CREATE TRIGGER IF NOT EXISTS story_search_documents_au AFTER UPDATE ON story_search_documents BEGIN
                    INSERT INTO story_search_documents_fts(story_search_documents_fts, rowid, search_text)
                    VALUES ('delete', old.id, old.search_text);
                    INSERT INTO story_search_documents_fts(rowid, search_text) VALUES (new.id, new.search_text);
                END;

                CREATE TABLE IF NOT EXISTS story_search_sources (
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
                );
                CREATE INDEX IF NOT EXISTS idx_story_search_sources_status
                    ON story_search_sources(project_id, status, chapter_no_sort);

                CREATE TABLE IF NOT EXISTS derived_index_jobs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    chapter_id INTEGER,
                    source_artifact_id INTEGER,
                    job_type TEXT NOT NULL,
                    scope_key TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    attempt_count INTEGER NOT NULL DEFAULT 0,
                    next_attempt_at TEXT NOT NULL,
                    last_error TEXT,
                    created_at TEXT NOT NULL,
                    started_at TEXT,
                    finished_at TEXT,
                    updated_at TEXT NOT NULL,
                    UNIQUE(project_id, job_type, scope_key),
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_derived_index_jobs_ready
                    ON derived_index_jobs(status, next_attempt_at, id);
                CREATE INDEX IF NOT EXISTS idx_derived_index_jobs_project
                    ON derived_index_jobs(project_id, status, updated_at DESC);

                CREATE TABLE IF NOT EXISTS adoption_proposals (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    source_artifact_id INTEGER NOT NULL,
                    target_kind TEXT NOT NULL,
                    target_id INTEGER,
                    operation TEXT NOT NULL,
                    data_json TEXT NOT NULL,
                    evidence_quote TEXT NOT NULL,
                    target_snapshot TEXT,
                    status TEXT NOT NULL DEFAULT 'pending',
                    validation_error TEXT,
                    decision_note TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_adoption_proposals_project_source
                    ON adoption_proposals(project_id, source_artifact_id, status);

                CREATE TABLE IF NOT EXISTS story_bibles (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL UNIQUE,
                    reader_promise TEXT NOT NULL DEFAULT '',
                    protagonist_engine TEXT NOT NULL DEFAULT '',
                    core_conflict TEXT NOT NULL DEFAULT '',
                    endgame_direction TEXT NOT NULL DEFAULT '',
                    immutable_rules TEXT NOT NULL DEFAULT '',
                    canon_version INTEGER NOT NULL DEFAULT 0,
                    status TEXT NOT NULL DEFAULT 'draft',
                    source_artifact_id INTEGER,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
                );

                CREATE TABLE IF NOT EXISTS story_arcs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    arc_no INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    objective TEXT NOT NULL DEFAULT '',
                    entry_state TEXT NOT NULL DEFAULT '',
                    exit_change TEXT NOT NULL DEFAULT '',
                    core_conflict TEXT NOT NULL DEFAULT '',
                    involved_characters TEXT NOT NULL DEFAULT '',
                    chapter_start INTEGER,
                    chapter_end INTEGER,
                    status TEXT NOT NULL DEFAULT 'planning',
                    source_artifact_id INTEGER,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(project_id, arc_no),
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
                    FOREIGN KEY(source_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
                );

                CREATE TABLE IF NOT EXISTS story_bible_reviews (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project_id INTEGER NOT NULL,
                    canon_fingerprint TEXT NOT NULL,
                    verdict TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    issues_json TEXT NOT NULL DEFAULT '[]',
                    status TEXT NOT NULL DEFAULT 'pending_human_confirmation',
                    note TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    confirmed_at TEXT,
                    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_story_bible_reviews_project_fingerprint
                    ON story_bible_reviews(project_id, canon_fingerprint, id DESC);
                "#,
            )?;

            conn.execute(
                "DELETE FROM agents WHERE stage IN ('setting', 'outline', 'characters')",
                [],
            )?;

            for (stage, name, role, prompt, temperature) in default_agents() {
                conn.execute(
                    "INSERT INTO agents (stage, name, role, system_prompt, temperature)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(stage) DO NOTHING",
                    params![stage, name, role, prompt, temperature],
                )?;
            }
            for (stage, name, role, prompt, temperature) in default_background_agents() {
                conn.execute(
                    "INSERT INTO agents (stage, name, role, system_prompt, temperature)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(stage) DO NOTHING",
                    params![stage, name, role, prompt, temperature],
                )?;
            }
            backfill_project_genre_agents(conn)?;

            set_default_setting(conn, "ai.base_url", "https://api.deepseek.com")?;
            set_default_setting(conn, "ai.model", "deepseek-v4-pro")?;
            set_default_setting(conn, "ai.temperature", "0.75")?;
            set_default_setting(conn, "ai.thinking_enabled", "false")?;
            set_default_setting(conn, "ai.thinking_level", "off")?;
            set_default_setting(
                conn,
                "prompts.default_version",
                crate::prompt_templates::DEFAULT_PROMPT_VERSION,
            )?;

            let now = now();
            for (label, base_url, model, temperature, thinking_enabled, thinking_level) in
                default_ai_providers()
            {
                conn.execute(
                    "INSERT INTO ai_providers
                        (label, base_url, model, temperature, thinking_enabled, thinking_level, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                     ON CONFLICT(base_url) DO NOTHING",
                    params![
                        label,
                        base_url,
                        model,
                        temperature,
                        thinking_enabled as i64,
                        thinking_level,
                        now
                    ],
                )?;
            }
            for skill in genre_skill::default_writing_skills() {
                conn.execute(
                    "INSERT OR IGNORE INTO writing_skills
                        (skill_key, name, category, description, content, enabled, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
                    params![
                        skill.skill_key,
                        skill.name,
                        skill.category,
                        skill.description,
                        skill.content,
                        now
                    ],
                )?;
            }
            patch_default_writing_skills(conn)?;
            dedupe_approvals(conn)?;
            Ok(())
        })
    }

    pub fn create_project(&self, input: NewProject) -> AppResult<Project> {
        if input.title.trim().is_empty() {
            return Err(AppError::Validation("项目标题不能为空".to_string()));
        }
        if input.genre.trim().is_empty() {
            return Err(AppError::Validation("项目题材不能为空".to_string()));
        }
        if input.target_words <= 0 {
            return Err(AppError::Validation("预计总字数必须大于 0".to_string()));
        }
        let now = now();
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                conn.execute(
                    "INSERT INTO projects (title, genre, target_words, premise, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)",
                    params![
                        input.title.trim(),
                        input.genre.trim(),
                        input.target_words,
                        input.premise.trim(),
                        &now
                    ],
                )?;
                let project_id = conn.last_insert_rowid();
                let genre_agent = genre_agent::detect_genre_agent(input.genre.trim());
                conn.execute(
                    "INSERT INTO project_genre_agents (project_id, agent_key, assigned_at)
                     VALUES (?1, ?2, ?3)",
                    params![project_id, genre_agent.agent_key, &now],
                )?;
                conn.execute(
                    "INSERT INTO chapters (project_id, chapter_no, title, status, created_at, updated_at)
                     VALUES (?1, 1, '第 1 章', 'planning', ?2, ?2)",
                    params![project_id, &now],
                )?;
                query_project_by_id(conn, project_id)
            })();
            match result {
                Ok(project) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(project)
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(error)
                }
            }
        })
    }

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, title, genre, target_words, premise, status, created_at, updated_at
                 FROM projects ORDER BY updated_at DESC, id DESC",
            )?;
            let rows = stmt.query_map([], map_project)?;
            collect_rows(rows)
        })
    }

    pub fn get_project(&self, id: i64) -> AppResult<Project> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, title, genre, target_words, premise, status, created_at, updated_at
                 FROM projects WHERE id = ?1",
                params![id],
                map_project,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("项目不存在".to_string()))
        })
    }

    pub fn update_project(&self, input: ProjectUpdate) -> AppResult<Project> {
        if input.title.trim().is_empty() {
            return Err(AppError::Validation("项目标题不能为空".to_string()));
        }
        if input.genre.trim().is_empty() {
            return Err(AppError::Validation("项目题材不能为空".to_string()));
        }
        if input.target_words <= 0 {
            return Err(AppError::Validation("预计总字数必须大于 0".to_string()));
        }
        if !matches!(input.status.trim(), "active" | "paused" | "archived") {
            return Err(AppError::Validation("项目状态无效".to_string()));
        }
        let now = now();
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                let changed = conn.execute(
                    "UPDATE projects
                     SET title = ?1, genre = ?2, target_words = ?3, premise = ?4, status = ?5, updated_at = ?6
                     WHERE id = ?7",
                    params![
                        input.title.trim(),
                        input.genre.trim(),
                        input.target_words,
                        input.premise.trim(),
                        input.status.trim(),
                        &now,
                        input.id
                    ],
                )?;
                if changed == 0 {
                    return Err(AppError::Validation("项目不存在".to_string()));
                }
                query_project_by_id(conn, input.id)
            })();
            match result {
                Ok(project) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(project)
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(error)
                }
            }
        })
    }

    pub fn delete_project(&self, id: i64) -> AppResult<()> {
        let result = self.with_conn(|conn| {
            crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, conn)?;
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                delete_project_search_data_tx(conn, id)?;
                conn.execute(
                    "DELETE FROM derived_index_jobs WHERE project_id = ?1",
                    params![id],
                )?;
                let deleted = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
                if deleted == 0 {
                    return Err(AppError::Validation("项目不存在".to_string()));
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
        });
        if result.is_ok() {
            self.clear_reference_materials(id);
        }
        result
    }

    pub fn get_detail(&self, project_id: i64) -> AppResult<ProjectDetail> {
        Ok(ProjectDetail {
            project: self.get_project(project_id)?,
            genre_agent: self.get_genre_agent_for_project(project_id)?,
            chapters: self.list_chapters(project_id)?,
            agents: self.list_agents_for_project(project_id)?,
            artifacts: self.list_artifacts(ArtifactFilters {
                project_id,
                stage: None,
                chapter_id: None,
            })?,
            approvals: self.list_approvals(project_id)?,
            messages: self.list_messages(project_id)?,
            workflow_runs: self.list_workflow_runs(project_id)?,
            story_threads: self.list_story_threads(project_id)?,
            knowledge_cards: self.list_knowledge_cards(project_id)?,
            foreshadowings: self.list_foreshadowings(project_id)?,
            story_entities: self.list_story_entities(project_id)?,
            story_events: self.list_story_events(project_id)?,
            story_event_participants: self.list_story_event_participants(project_id)?,
            story_facts: self.list_story_facts(project_id)?,
            story_index_sources: self.list_story_index_sources(project_id)?,
            story_search_sources: self.list_story_search_sources(project_id)?,
            index_jobs: self.list_derived_index_jobs(project_id)?,
            adoption_proposals: self.list_adoption_proposals(project_id, None)?,
            story_bible: self.get_story_bible(project_id)?,
            story_arcs: self.list_story_arcs(project_id)?,
            story_bible_review: self.latest_story_bible_review(project_id)?,
            canonical_fingerprint: crate::story_architecture::canonical_fingerprint(
                self, project_id,
            )?,
            settings: self.get_ai_settings()?,
        })
    }

    pub fn list_agents(&self) -> AppResult<Vec<Agent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(AGENT_SELECT_SQL)?;
            let rows = stmt.query_map([], map_agent)?;
            collect_rows(rows)
        })
    }

    pub fn get_agent(&self, stage: &str) -> AppResult<Agent> {
        self.with_conn(|conn| {
            conn.query_row(
                &format!("{} WHERE a.stage = ?1", AGENT_SELECT_SQL),
                params![stage],
                map_agent,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("Agent 不存在".to_string()))
        })
    }

    pub fn get_agent_by_id(&self, agent_id: i64) -> AppResult<Agent> {
        self.with_conn(|conn| {
            conn.query_row(
                &format!("{} WHERE a.id = ?1", AGENT_SELECT_SQL),
                params![agent_id],
                map_agent,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("Agent 不存在".to_string()))
        })
    }

    pub fn replace_agent_prompt(&self, agent_id: i64, prompt: &str) -> AppResult<Agent> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(AppError::Validation("Agent Prompt 不能为空".to_string()));
        }
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE agents SET system_prompt = ?1 WHERE id = ?2",
                params![prompt, agent_id],
            )?;
            if changed == 0 {
                return Err(AppError::Validation("Agent 不存在".to_string()));
            }
            conn.query_row(
                &format!("{} WHERE a.id = ?1", AGENT_SELECT_SQL),
                params![agent_id],
                map_agent,
            )
            .map_err(AppError::from)
        })
    }

    pub fn save_agent_settings(&self, input: SaveAgentSettings) -> AppResult<Agent> {
        self.with_conn(|conn| {
            let current = conn
                .query_row(
                    "SELECT name, role, system_prompt, temperature, thinking_enabled,
                            thinking_level, enabled_tool_keys, allowed_skill_keys
                     FROM agents WHERE id = ?1",
                    params![input.agent_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, f64>(3)?,
                            row.get::<_, i64>(4)? != 0,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| AppError::Validation("Agent 不存在".to_string()))?;

            let uses_global = input.uses_global_runtime_settings.unwrap_or(false);
            let provider_base_url = if uses_global {
                String::new()
            } else {
                input.provider_base_url.trim().to_string()
            };
            let model = if uses_global {
                String::new()
            } else {
                input.model.trim().to_string()
            };
            if !uses_global && provider_base_url.is_empty() {
                return Err(AppError::Validation("Agent 供应商地址不能为空".to_string()));
            }
            if !uses_global && model.is_empty() {
                return Err(AppError::Validation("Agent 模型名称不能为空".to_string()));
            }

            let name = input
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(current.0.as_str())
                .to_string();
            let role = input
                .role
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(current.1.as_str())
                .to_string();
            let system_prompt = input
                .system_prompt
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(current.2.as_str())
                .to_string();
            let temperature = input.temperature.unwrap_or(current.3);
            if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
                return Err(AppError::Validation(
                    "Agent Temperature 必须在 0 到 2 之间".to_string(),
                ));
            }

            let thinking_enabled = if uses_global {
                false
            } else {
                input.thinking_enabled
            };
            let thinking_level = if uses_global {
                "off".to_string()
            } else {
                crate::models::normalize_thinking_level(
                    thinking_enabled,
                    input
                        .thinking_level
                        .as_deref()
                        .unwrap_or(current.5.as_str()),
                )
                .map_err(AppError::Validation)?
            };
            let enabled_tool_keys = input
                .enabled_tool_keys
                .as_deref()
                .map(agent_tools::normalize_keys)
                .unwrap_or_else(|| parse_agent_keys(&current.6, true));
            let allowed_skill_keys = input
                .allowed_skill_keys
                .as_deref()
                .map(normalize_skill_keys)
                .unwrap_or_else(|| parse_agent_keys(&current.7, false));

            let changed = conn.execute(
                "UPDATE agents
                 SET name = ?1, role = ?2, system_prompt = ?3, temperature = ?4,
                     provider_base_url = ?5, model = ?6, thinking_enabled = ?7,
                     thinking_level = ?8, enabled_tool_keys = ?9, allowed_skill_keys = ?10
                 WHERE id = ?11",
                params![
                    name,
                    role,
                    system_prompt,
                    temperature,
                    provider_base_url,
                    model,
                    thinking_enabled as i64,
                    thinking_level,
                    serde_json::to_string(&enabled_tool_keys)?,
                    serde_json::to_string(&allowed_skill_keys)?,
                    input.agent_id,
                ],
            )?;
            if changed == 0 {
                return Err(AppError::Validation("Agent 不存在".to_string()));
            }
            conn.query_row(
                &format!("{} WHERE a.id = ?1", AGENT_SELECT_SQL),
                params![input.agent_id],
                map_agent,
            )
            .map_err(AppError::from)
        })
    }

    pub fn get_agent_for_stage(&self, stage: &str) -> AppResult<Agent> {
        let key = match stage {
            "setting" | "outline" | "characters" => "story_architect",
            other => other,
        };
        self.get_agent(key)
    }

    pub fn get_ai_settings_for_agent(&self, agent_key: &str) -> AppResult<AiSettings> {
        let agent = self.get_agent(agent_key)?;
        let mut settings = agent.ai_settings();
        settings.has_api_key = self.get_api_key_for_base_url(&settings.base_url)?.is_some();
        Ok(settings)
    }

    pub fn list_ai_providers(&self) -> AppResult<Vec<AiProvider>> {
        let providers = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, label, base_url, model, temperature, thinking_enabled, thinking_level, tool_protocol
                 FROM ai_providers ORDER BY id",
            )?;
            let rows = stmt.query_map([], map_ai_provider)?;
            collect_rows(rows)
        })?;

        providers
            .into_iter()
            .map(|mut provider| {
                provider.has_api_key = self.get_api_key_for_base_url(&provider.base_url)?.is_some();
                Ok(provider)
            })
            .collect()
    }

    fn get_ai_provider(&self, provider_id: i64) -> AppResult<AiProvider> {
        let mut provider = self
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT id, label, base_url, model, temperature, thinking_enabled, thinking_level, tool_protocol
                 FROM ai_providers WHERE id = ?1",
                    params![provider_id],
                    map_ai_provider,
                )
                .optional()
                .map_err(AppError::from)
            })?
            .ok_or_else(|| AppError::Validation("供应商不存在".to_string()))?;
        provider.has_api_key = self.get_api_key_for_base_url(&provider.base_url)?.is_some();
        Ok(provider)
    }

    pub fn save_ai_provider(&self, input: SaveAiProvider) -> AppResult<AiProvider> {
        let label = input.label.trim().to_string();
        let base_url = input.base_url.trim().to_string();
        let model = input.model.trim().to_string();
        if label.is_empty() {
            return Err(AppError::Validation("供应商名称不能为空".to_string()));
        }
        if base_url.is_empty() {
            return Err(AppError::Validation("供应商地址不能为空".to_string()));
        }
        if model.is_empty() {
            return Err(AppError::Validation("供应商模型不能为空".to_string()));
        }
        if !input.temperature.is_finite() || !(0.0..=2.0).contains(&input.temperature) {
            return Err(AppError::Validation(
                "供应商 Temperature 必须在 0 到 2 之间".to_string(),
            ));
        }
        let thinking_level =
            crate::models::normalize_thinking_level(input.thinking_enabled, &input.thinking_level)
                .map_err(AppError::Validation)?;
        let tool_protocol = input.tool_protocol.as_str();

        let updated_at = now();
        let provider_id = self.with_conn(|conn| {
            let existing_id = conn
                .query_row(
                    "SELECT id FROM ai_providers WHERE base_url = ?1",
                    params![base_url],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;

            let id = match input.id {
                Some(id) => {
                    if existing_id.is_some_and(|existing| existing != id) {
                        return Err(AppError::Validation(
                            "供应商地址已被其他配置使用".to_string(),
                        ));
                    }
                    let changed = conn.execute(
                        "UPDATE ai_providers
                         SET label = ?1, base_url = ?2, model = ?3, temperature = ?4,
                             thinking_enabled = ?5, thinking_level = ?6, tool_protocol = ?7,
                             api_key = CASE WHEN base_url = ?2 THEN api_key ELSE '' END,
                             detected_tool_protocol = NULL, tool_capability_error = NULL,
                             tool_capability_updated_at = NULL, updated_at = ?8
                         WHERE id = ?9",
                        params![
                            label,
                            base_url,
                            model,
                            input.temperature,
                            input.thinking_enabled as i64,
                            thinking_level,
                            tool_protocol,
                            updated_at,
                            id
                        ],
                    )?;
                    if changed == 0 {
                        return Err(AppError::Validation("供应商不存在".to_string()));
                    }
                    id
                }
                None => {
                    if let Some(id) = existing_id {
                        conn.execute(
                            "UPDATE ai_providers
                             SET label = ?1, model = ?2, temperature = ?3,
                                 thinking_enabled = ?4, thinking_level = ?5, tool_protocol = ?6,
                                 detected_tool_protocol = NULL, tool_capability_error = NULL,
                                 tool_capability_updated_at = NULL, updated_at = ?7
                             WHERE id = ?8",
                            params![
                                label,
                                model,
                                input.temperature,
                                input.thinking_enabled as i64,
                                thinking_level,
                                tool_protocol,
                                updated_at,
                                id
                            ],
                        )?;
                        id
                    } else {
                        conn.execute(
                            "INSERT INTO ai_providers
                            (label, base_url, model, temperature, thinking_enabled, thinking_level,
                             tool_protocol, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                            params![
                                label,
                                base_url,
                                model,
                                input.temperature,
                                input.thinking_enabled as i64,
                                thinking_level,
                                tool_protocol,
                                updated_at
                            ],
                        )?;
                        conn.last_insert_rowid()
                    }
                }
            };
            Ok(id)
        })?;

        self.get_ai_provider(provider_id)
    }

    pub fn delete_ai_provider(&self, provider_id: i64) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                let base_url = conn
                    .query_row(
                        "SELECT base_url FROM ai_providers WHERE id = ?1",
                        params![provider_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .ok_or_else(|| AppError::Validation("供应商不存在".to_string()))?;
                let provider_count: i64 =
                    conn.query_row("SELECT COUNT(*) FROM ai_providers", [], |row| row.get(0))?;
                if provider_count <= 1 {
                    return Err(AppError::Validation("至少保留一个供应商".to_string()));
                }

                conn.execute(
                    "DELETE FROM ai_providers WHERE id = ?1",
                    params![provider_id],
                )?;

                let active_base_url = get_setting(conn, "ai.base_url")?;
                if active_base_url
                    .as_deref()
                    .is_some_and(|value| value.trim() == base_url)
                {
                    let fallback = conn
                        .query_row(
                            "SELECT base_url, model, temperature, thinking_enabled, thinking_level
                             FROM ai_providers ORDER BY id ASC LIMIT 1",
                            [],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, f64>(2)?,
                                    row.get::<_, i64>(3)? != 0,
                                    row.get::<_, String>(4)?,
                                ))
                            },
                        )
                        .optional()?;
                    let (
                        fallback_base_url,
                        fallback_model,
                        fallback_temperature,
                        fallback_thinking,
                        fallback_thinking_level,
                    ) = fallback.ok_or_else(|| {
                        AppError::Validation("删除供应商后没有可用的备用配置".to_string())
                    })?;
                    upsert_setting(conn, "ai.base_url", &fallback_base_url)?;
                    upsert_setting(conn, "ai.model", &fallback_model)?;
                    upsert_setting(conn, "ai.temperature", &fallback_temperature.to_string())?;
                    upsert_setting(
                        conn,
                        "ai.thinking_enabled",
                        if fallback_thinking { "true" } else { "false" },
                    )?;
                    upsert_setting(conn, "ai.thinking_level", &fallback_thinking_level)?;
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

        Ok(())
    }

    pub fn get_genre_agent_for_project(&self, project_id: i64) -> AppResult<GenreAgentProfile> {
        self.with_conn(|conn| resolve_project_genre_agent(conn, project_id))
    }

    pub fn list_agents_for_project(&self, project_id: i64) -> AppResult<Vec<Agent>> {
        let profile = self.get_genre_agent_for_project(project_id)?;
        Ok(self
            .list_agents()?
            .into_iter()
            .map(|agent| genre_agent::compose_stage_agent(agent, &profile))
            .collect())
    }

    pub fn get_agent_for_project_stage(&self, project_id: i64, stage: &str) -> AppResult<Agent> {
        let profile = self.get_genre_agent_for_project(project_id)?;
        Ok(genre_agent::compose_stage_agent(
            self.get_agent_for_stage(stage)?,
            &profile,
        ))
    }

    pub fn list_chapters(&self, project_id: i64) -> AppResult<Vec<Chapter>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, chapter_no, title, status, current_artifact_id, created_at, updated_at
                 FROM chapters WHERE project_id = ?1 ORDER BY chapter_no ASC",
            )?;
            let rows = stmt.query_map(params![project_id], map_chapter)?;
            collect_rows(rows)
        })
    }

    pub fn create_chapter(&self, input: NewChapter) -> AppResult<Chapter> {
        if input.project_id <= 0 {
            return Err(AppError::Validation("项目不存在".to_string()));
        }
        self.get_project(input.project_id)?;
        let now = now();
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                let next_no: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(chapter_no), 0) + 1 FROM chapters WHERE project_id = ?1",
                    params![input.project_id],
                    |row| row.get(0),
                )?;
                let title = input
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("第 {next_no} 章"));
                conn.execute(
                    "INSERT INTO chapters (project_id, chapter_no, title, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'planning', ?4, ?4)",
                    params![input.project_id, next_no, title, &now],
                )?;
                let id = conn.last_insert_rowid();
                conn.execute(
                    "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                    params![&now, input.project_id],
                )?;
                conn.query_row(
                    "SELECT id, project_id, chapter_no, title, status, current_artifact_id, created_at, updated_at
                     FROM chapters WHERE id = ?1",
                    params![id],
                    map_chapter,
                )
                .optional()?
                .ok_or_else(|| AppError::Validation("章节创建失败".to_string()))
            })();
            match result {
                Ok(chapter) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(chapter)
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(error)
                }
            }
        })
    }

    pub fn delete_chapter(&self, project_id: i64, chapter_id: i64) -> AppResult<()> {
        if project_id <= 0 || chapter_id <= 0 {
            return Err(AppError::Validation("项目或章节不存在".to_string()));
        }

        self.with_conn(|conn| {
            crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, conn)?;
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                let chapter_no: i64 = conn
                    .query_row(
                        "SELECT chapter_no FROM chapters WHERE id = ?1 AND project_id = ?2",
                        params![chapter_id, project_id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;

                delete_chapter_search_data_tx(conn, project_id, chapter_id, chapter_no)?;
                conn.execute(
                    "DELETE FROM chapters WHERE id = ?1 AND project_id = ?2",
                    params![chapter_id, project_id],
                )?;
                let timestamp = now();
                conn.execute(
                    "UPDATE chapters
                    SET chapter_no = chapter_no - 1, updated_at = ?1
                     WHERE project_id = ?2 AND chapter_no > ?3",
                    params![&timestamp, project_id, chapter_no],
                )?;
                conn.execute(
                    "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                    params![&timestamp, project_id],
                )?;
                crate::index_jobs::enqueue_project_search_job_tx(conn, project_id, &timestamp)?;
                Ok(())
            })();

            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(err)
                }
            }
        })
    }

    pub fn update_chapter(&self, input: ChapterUpdate) -> AppResult<Chapter> {
        if input.project_id <= 0 || input.id <= 0 {
            return Err(AppError::Validation("项目或章节不存在".to_string()));
        }
        if input.title.trim().is_empty() {
            return Err(AppError::Validation("章节标题不能为空".to_string()));
        }
        if !matches!(input.status.trim(), "planning" | "approved") {
            return Err(AppError::Validation("章节状态无效".to_string()));
        }
        let now = now();
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                let changed = conn.execute(
                    "UPDATE chapters
                     SET title = ?1, status = ?2, updated_at = ?3
                     WHERE id = ?4 AND project_id = ?5",
                    params![input.title.trim(), input.status.trim(), &now, input.id, input.project_id],
                )?;
                if changed == 0 {
                    return Err(AppError::Validation("章节不存在".to_string()));
                }
                conn.execute(
                    "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                    params![&now, input.project_id],
                )?;
                conn.query_row(
                    "SELECT id, project_id, chapter_no, title, status, current_artifact_id, created_at, updated_at
                     FROM chapters WHERE id = ?1 AND project_id = ?2",
                    params![input.id, input.project_id],
                    map_chapter,
                )
                .optional()?
                .ok_or_else(|| AppError::Validation("章节不存在".to_string()))
            })();
            match result {
                Ok(chapter) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(chapter)
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(error)
                }
            }
        })
    }

    pub fn ensure_chapter(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
    ) -> AppResult<Option<Chapter>> {
        match chapter_id {
            Some(id) => self.with_conn(|conn| {
                conn.query_row(
                    "SELECT id, project_id, chapter_no, title, status, current_artifact_id, created_at, updated_at
                     FROM chapters WHERE id = ?1 AND project_id = ?2",
                    params![id, project_id],
                    map_chapter,
                )
                .optional()?
                .map(Some)
                .ok_or_else(|| AppError::Validation("章节不存在".to_string()))
            }),
            None => Ok(None),
        }
    }

    pub fn list_artifacts(&self, filters: ArtifactFilters) -> AppResult<Vec<Artifact>> {
        self.with_conn(|conn| {
            let mut sql = String::from(
                "SELECT id, project_id, chapter_id, stage, title, content, version, status, parent_artifact_id, created_at
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

            let params = rusqlite::params_from_iter(values.iter().map(|v| &**v));
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params, map_artifact)?;
            collect_rows(rows)
        })
    }

    pub fn get_artifact(&self, artifact_id: i64) -> AppResult<Artifact> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, project_id, chapter_id, stage, title, content, version, status, parent_artifact_id, created_at
                 FROM artifacts WHERE id = ?1",
                params![artifact_id],
                map_artifact,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("产物不存在".to_string()))
        })
    }

    pub fn latest_artifact(
        &self,
        project_id: i64,
        stage: &str,
        chapter_id: Option<i64>,
    ) -> AppResult<Option<Artifact>> {
        self.with_conn(|conn| {
            if let Some(chapter_id) = chapter_id {
                conn.query_row(
                    "SELECT id, project_id, chapter_id, stage, title, content, version, status, parent_artifact_id, created_at
                     FROM artifacts WHERE project_id = ?1 AND stage = ?2 AND chapter_id = ?3
                     ORDER BY version DESC, id DESC LIMIT 1",
                    params![project_id, stage, chapter_id],
                    map_artifact,
                )
                .optional()
                .map_err(AppError::from)
            } else {
                conn.query_row(
                    "SELECT id, project_id, chapter_id, stage, title, content, version, status, parent_artifact_id, created_at
                     FROM artifacts WHERE project_id = ?1 AND stage = ?2 AND chapter_id IS NULL
                     ORDER BY version DESC, id DESC LIMIT 1",
                    params![project_id, stage],
                    map_artifact,
                )
                .optional()
                .map_err(AppError::from)
            }
        })
    }

    pub fn approved_artifact(
        &self,
        project_id: i64,
        stage: &str,
        chapter_id: Option<i64>,
    ) -> AppResult<Option<Artifact>> {
        self.with_conn(|conn| {
            if let Some(chapter_id) = chapter_id {
                conn.query_row(
                    "SELECT a.id, a.project_id, a.chapter_id, a.stage, a.title, a.content, a.version, a.status, a.parent_artifact_id, a.created_at
                     FROM artifacts a
                     INNER JOIN approvals ap ON ap.artifact_id = a.id
                     WHERE a.project_id = ?1 AND a.stage = ?2 AND a.chapter_id = ?3
                     ORDER BY ap.created_at DESC, ap.id DESC LIMIT 1",
                    params![project_id, stage, chapter_id],
                    map_artifact,
                )
                .optional()
                .map_err(AppError::from)
            } else {
                conn.query_row(
                    "SELECT a.id, a.project_id, a.chapter_id, a.stage, a.title, a.content, a.version, a.status, a.parent_artifact_id, a.created_at
                     FROM artifacts a
                     INNER JOIN approvals ap ON ap.artifact_id = a.id
                     WHERE a.project_id = ?1 AND a.stage = ?2 AND a.chapter_id IS NULL
                     ORDER BY ap.created_at DESC, ap.id DESC LIMIT 1",
                    params![project_id, stage],
                    map_artifact,
                )
                .optional()
                .map_err(AppError::from)
            }
        })
    }

    pub fn latest_approved_chapter_body(
        &self,
        project_id: i64,
        chapter_id: i64,
    ) -> AppResult<Option<Artifact>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT a.id, a.project_id, a.chapter_id, a.stage, a.title, a.content, a.version, a.status, a.parent_artifact_id, a.created_at
                 FROM artifacts a
                 INNER JOIN approvals ap ON ap.artifact_id = a.id
                 WHERE a.project_id = ?1
                   AND a.chapter_id = ?2
                   AND a.stage IN ('draft', 'revision')
                 ORDER BY ap.created_at DESC, ap.id DESC
                 LIMIT 1",
                params![project_id, chapter_id],
                map_artifact,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn continuity_ledger_source_is_current(
        &self,
        project_id: i64,
        chapter_id: i64,
        source_artifact_id: i64,
        source_text_hash: &str,
        normalization_version: &str,
    ) -> AppResult<bool> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM continuity_ledger_sources
                     WHERE project_id = ?1 AND chapter_id = ?2 AND source_artifact_id = ?3
                       AND source_text_hash = ?4 AND normalization_version = ?5",
                    params![
                        project_id,
                        chapter_id,
                        source_artifact_id,
                        source_text_hash,
                        normalization_version
                    ],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    pub fn replace_continuity_ledger_chapter_cas(
        &self,
        project_id: i64,
        chapter_id: i64,
        source_artifact_id: i64,
        source_text_hash: &str,
        normalization_version: &str,
        entries: &[ContinuityLedgerEntry],
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            let current_content = conn
                .query_row(
                    "SELECT a.content FROM chapters c
                     INNER JOIN artifacts a ON a.id = c.current_artifact_id
                     WHERE c.id = ?1 AND c.project_id = ?2 AND a.id = ?3
                       AND a.stage IN ('draft', 'revision') AND a.status = 'approved'",
                    params![chapter_id, project_id, source_artifact_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let Some(current_content) = current_content else {
                return Err(AppError::Validation("章节正式正文已切换，丢弃旧的连续性账本".to_string()));
            };
            if crate::chapter_memory::source_text_hash(&current_content) != source_text_hash {
                return Err(AppError::Validation("章节正文已变化，丢弃旧的连续性账本".to_string()));
            }

            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM continuity_ledger_entries WHERE project_id = ?1 AND chapter_id = ?2",
                params![project_id, chapter_id],
            )?;
            tx.execute(
                "INSERT INTO continuity_ledger_sources
                    (project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version, built_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(chapter_id) DO UPDATE SET
                    project_id = excluded.project_id,
                    source_artifact_id = excluded.source_artifact_id,
                    source_text_hash = excluded.source_text_hash,
                    normalization_version = excluded.normalization_version,
                    built_at = excluded.built_at",
                params![project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version, now()],
            )?;
            for entry in entries {
                tx.execute(
                    "INSERT INTO continuity_ledger_entries
                       (project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version,
                        entity_kind, entity_key, entity_label, state_kind, state_value, evidence_quote, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version,
                        entry.entity_kind, entry.entity_key, entry.entity_label, entry.state_kind,
                        entry.state_value, entry.evidence_quote, now()
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    pub fn invalidate_continuity_ledger_from_chapter(
        &self,
        project_id: i64,
        chapter_id: i64,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = invalidate_continuity_ledger_from_chapter_tx(conn, project_id, chapter_id);
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

    pub fn story_index_source_is_current(
        &self,
        project_id: i64,
        chapter_id: i64,
        source_artifact_id: i64,
        source_text_hash: &str,
        normalization_version: &str,
    ) -> AppResult<bool> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM story_index_sources
                     WHERE project_id = ?1 AND chapter_id = ?2 AND source_artifact_id = ?3
                       AND source_text_hash = ?4 AND normalization_version = ?5 AND status = 'success'",
                    params![project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    pub(crate) fn replace_story_index_chapter_cas(
        &self,
        project_id: i64,
        chapter_id: i64,
        source_artifact_id: i64,
        source_text_hash: &str,
        normalization_version: &str,
        indexed: &crate::story_index::IndexedChapter,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            let current_content = conn
                .query_row(
                    "SELECT a.content FROM chapters c
                     INNER JOIN artifacts a ON a.id = c.current_artifact_id
                     WHERE c.id = ?1 AND c.project_id = ?2 AND a.id = ?3
                       AND a.stage IN ('draft', 'revision') AND a.status = 'approved'",
                    params![chapter_id, project_id, source_artifact_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let Some(current_content) = current_content else {
                return Err(AppError::Validation("章节正式正文已切换，丢弃旧的资料索引".to_string()));
            };
            if crate::chapter_memory::source_text_hash(&current_content) != source_text_hash {
                return Err(AppError::Validation("章节正文已变化，丢弃旧的资料索引".to_string()));
            }

            let timestamp = now();
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM story_facts WHERE project_id = ?1 AND source_artifact_id = ?2",
                params![project_id, source_artifact_id],
            )?;
            tx.execute(
                "DELETE FROM story_events WHERE project_id = ?1 AND source_artifact_id = ?2",
                params![project_id, source_artifact_id],
            )?;

            let mut entity_ids = std::collections::HashMap::new();
            for entity in &indexed.entities {
                tx.execute(
                    "INSERT INTO story_entities
                        (project_id, kind, name, status, first_seen_chapter_id, source_artifact_id, source_quote, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?7)
                     ON CONFLICT(project_id, kind, name) DO UPDATE SET updated_at = excluded.updated_at",
                    params![project_id, entity.kind, entity.name, chapter_id, source_artifact_id, entity.evidence_quote, timestamp],
                )?;
                let id = tx.query_row(
                    "SELECT id FROM story_entities WHERE project_id = ?1 AND kind = ?2 AND name = ?3",
                    params![project_id, entity.kind, entity.name],
                    |row| row.get::<_, i64>(0),
                )?;
                entity_ids.insert(entity.key(), id);
            }

            let mut event_ids = std::collections::HashMap::new();
            for event in &indexed.events {
                tx.execute(
                    "INSERT INTO story_events
                        (project_id, title, kind, status, story_time, summary, narrative_chapter_id,
                         source_artifact_id, source_quote, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                    params![project_id, event.title, event.kind, event.status, event.story_time, event.summary,
                        chapter_id, source_artifact_id, event.evidence_quote, timestamp],
                )?;
                let event_id = tx.last_insert_rowid();
                event_ids.insert(event.key(), event_id);
                for participant in &event.participants {
                    let key = participant.key();
                    if let Some(entity_id) = entity_ids.get(&key) {
                        tx.execute(
                            "INSERT OR IGNORE INTO story_event_participants (event_id, entity_id, role)
                             VALUES (?1, ?2, ?3)",
                            params![event_id, entity_id, participant.role],
                        )?;
                    }
                }
            }

            for fact in &indexed.facts {
                let Some(entity_id) = entity_ids.get(&fact.entity_key()) else {
                    continue;
                };
                let event_id = fact.event_title.as_ref().and_then(|key| event_ids.get(key)).copied();
                tx.execute(
                    "INSERT INTO story_facts
                        (project_id, entity_id, event_id, dimension, value, visibility, status,
                         narrative_chapter_id, source_artifact_id, source_quote, supersedes_fact_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?9, NULL, ?10)",
                    params![project_id, entity_id, event_id, fact.dimension, fact.value, fact.visibility,
                        chapter_id, source_artifact_id, fact.evidence_quote, timestamp],
                )?;
            }

            tx.execute(
                "INSERT INTO story_index_sources
                    (project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version,
                     status, error, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'success', NULL, ?6)
                 ON CONFLICT(project_id, chapter_id) DO UPDATE SET
                    source_artifact_id = excluded.source_artifact_id,
                    source_text_hash = excluded.source_text_hash,
                    normalization_version = excluded.normalization_version,
                    status = excluded.status,
                    error = NULL,
                    indexed_at = excluded.indexed_at",
                params![project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version, timestamp],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn record_story_index_failure(
        &self,
        project_id: i64,
        chapter_id: i64,
        source_artifact_id: i64,
        source_text_hash: &str,
        normalization_version: &str,
        error: &str,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO story_index_sources
                    (project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version,
                     status, error, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'failed', ?6, ?7)
                 ON CONFLICT(project_id, chapter_id) DO UPDATE SET
                    source_artifact_id = excluded.source_artifact_id,
                    source_text_hash = excluded.source_text_hash,
                    normalization_version = excluded.normalization_version,
                    status = excluded.status,
                    error = excluded.error,
                    indexed_at = excluded.indexed_at",
                params![project_id, chapter_id, source_artifact_id, source_text_hash, normalization_version, error, now()],
            )?;
            Ok(())
        })
    }

    pub fn invalidate_story_index_from_chapter(
        &self,
        project_id: i64,
        chapter_id: i64,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = invalidate_story_index_from_chapter_tx(conn, project_id, chapter_id);
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

    pub fn list_continuity_ledger_entries(
        &self,
        project_id: i64,
    ) -> AppResult<Vec<ContinuityLedgerEntry>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.id, e.project_id, e.chapter_id, e.source_artifact_id, e.source_text_hash,
                        e.normalization_version, e.entity_kind, e.entity_key, e.entity_label,
                        e.state_kind, e.state_value, e.evidence_quote, e.created_at
                 FROM continuity_ledger_entries e
                 INNER JOIN chapters c ON c.id = e.chapter_id
                 WHERE e.project_id = ?1 ORDER BY c.chapter_no ASC, e.id ASC",
            )?;
            let rows = stmt.query_map(params![project_id], map_continuity_ledger_entry)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
        })
    }

    pub fn get_chapter_memory(
        &self,
        project_id: i64,
        chapter_id: i64,
    ) -> AppResult<Option<ChapterMemoryRecord>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, project_id, chapter_id, source_artifact_id, source_text_hash,
                        normalization_version, content, created_at, updated_at
                 FROM chapter_memories
                 WHERE project_id = ?1 AND chapter_id = ?2",
                params![project_id, chapter_id],
                map_chapter_memory,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_chapter_memory_cas(
        &self,
        project_id: i64,
        chapter_id: i64,
        source_artifact_id: i64,
        source_text_hash: &str,
        normalization_version: &str,
        content: &str,
    ) -> AppResult<ChapterMemoryRecord> {
        self.with_conn(|conn| {
            let current_content = conn
                .query_row(
                    "SELECT a.content
                     FROM chapters c
                     INNER JOIN artifacts a ON a.id = c.current_artifact_id
                     WHERE c.id = ?1 AND c.project_id = ?2 AND a.id = ?3
                       AND a.stage IN ('draft', 'revision') AND a.status = 'approved'",
                    params![chapter_id, project_id, source_artifact_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let Some(current_content) = current_content else {
                return Err(AppError::Validation(
                    "章节正式正文已切换，丢弃旧的交接记忆".to_string(),
                ));
            };
            if crate::chapter_memory::source_text_hash(&current_content) != source_text_hash {
                return Err(AppError::Validation(
                    "章节正文已变化，丢弃旧的交接记忆".to_string(),
                ));
            }

            let timestamp = now();
            conn.execute(
                "INSERT INTO chapter_memories
                    (project_id, chapter_id, source_artifact_id, source_text_hash,
                     normalization_version, content, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(chapter_id) DO UPDATE SET
                    project_id = excluded.project_id,
                    source_artifact_id = excluded.source_artifact_id,
                    source_text_hash = excluded.source_text_hash,
                    normalization_version = excluded.normalization_version,
                    content = excluded.content,
                    updated_at = excluded.updated_at",
                params![
                    project_id,
                    chapter_id,
                    source_artifact_id,
                    source_text_hash,
                    normalization_version,
                    content,
                    timestamp
                ],
            )?;
            conn.query_row(
                "SELECT id, project_id, chapter_id, source_artifact_id, source_text_hash,
                        normalization_version, content, created_at, updated_at
                 FROM chapter_memories WHERE chapter_id = ?1",
                params![chapter_id],
                map_chapter_memory,
            )
            .map_err(AppError::from)
        })
    }

    pub fn insert_artifact(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
        stage: &str,
        title: &str,
        content: &str,
        parent_artifact_id: Option<i64>,
    ) -> AppResult<Artifact> {
        self.get_project(project_id)?;
        if let Some(chapter_id) = chapter_id {
            if self.ensure_chapter(project_id, Some(chapter_id))?.is_none() {
                return Err(AppError::Validation("产物章节不属于当前项目".to_string()));
            }
        }
        if let Some(parent_id) = parent_artifact_id {
            let parent = self.get_artifact(parent_id)?;
            if parent.project_id != project_id {
                return Err(AppError::Validation("父产物不属于当前项目".to_string()));
            }
        }
        let now = now();
        for attempt in 0..4 {
            let result = self.with_conn(|conn| {
                conn.execute_batch("BEGIN IMMEDIATE")?;
                let result = (|| {
                    let next_version: i64 = if let Some(chapter_id) = chapter_id {
                        conn.query_row(
                            "SELECT COALESCE(MAX(version), 0) + 1
                             FROM artifacts WHERE project_id = ?1 AND chapter_id = ?2 AND stage = ?3",
                            params![project_id, chapter_id, stage],
                            |row| row.get(0),
                        )?
                    } else {
                        conn.query_row(
                            "SELECT COALESCE(MAX(version), 0) + 1
                             FROM artifacts WHERE project_id = ?1 AND chapter_id IS NULL AND stage = ?2",
                            params![project_id, stage],
                            |row| row.get(0),
                        )?
                    };
                    conn.execute(
                        "INSERT INTO artifacts (project_id, chapter_id, stage, title, content, version, status, parent_artifact_id, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending_human_approval', ?7, ?8)",
                        params![
                            project_id,
                            chapter_id,
                            stage,
                            title,
                            content,
                            next_version,
                            parent_artifact_id,
                            &now
                        ],
                    )?;
                    let id = conn.last_insert_rowid();
                    query_artifact_by_id(conn, id)
                })();
                match result {
                    Ok(artifact) => {
                        conn.execute_batch("COMMIT")?;
                        Ok(artifact)
                    }
                    Err(error) => {
                        let _ = conn.execute_batch("ROLLBACK");
                        Err(error)
                    }
                }
            });
            match result {
                Ok(artifact) => return Ok(artifact),
                Err(error) if attempt < 3 && retryable_artifact_insert_error(&error) => {
                    std::thread::sleep(Duration::from_millis(20 * (attempt + 1) as u64));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("artifact insert retry loop always returns")
    }

    pub fn delete_artifact(&self, project_id: i64, artifact_id: i64) -> AppResult<()> {
        let artifact = self.get_artifact(artifact_id)?;
        if artifact.project_id != project_id {
            return Err(AppError::Validation("产物不属于当前项目".to_string()));
        }
        if let Some(reason) = self.protected_artifact_reason(&artifact)? {
            return Err(AppError::Validation(reason));
        }
        let message = format!(
            "删除历史版本：{} v{}",
            stage_label(&artifact.stage),
            artifact.version
        );
        let timestamp = now();
        self.with_conn(|conn| {
            crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, conn)?;
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                delete_artifact_search_data_tx(conn, project_id, artifact_id)?;
                let deleted = conn.execute(
                    "DELETE FROM artifacts WHERE id = ?1 AND project_id = ?2",
                    params![artifact_id, project_id],
                )?;
                if deleted == 0 {
                    return Err(AppError::Validation("产物不存在".to_string()));
                }
                conn.execute(
                    "INSERT INTO messages (project_id, chapter_id, role, content, created_at)
                     VALUES (?1, ?2, 'human_instruction', ?3, ?4)",
                    params![project_id, artifact.chapter_id, &message, &timestamp],
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

    pub fn clear_chapter_history(
        &self,
        project_id: i64,
        chapter_id: i64,
        keep_artifact_ids: &[i64],
    ) -> AppResult<crate::models::HistoryCleanupResult> {
        let chapter = self
            .ensure_chapter(project_id, Some(chapter_id))?
            .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
        let mut keep_set = HashSet::new();
        for artifact_id in keep_artifact_ids {
            let artifact = self.get_artifact(*artifact_id)?;
            if artifact.project_id != project_id || artifact.chapter_id != Some(chapter_id) {
                return Err(AppError::Validation("保留的产物不属于当前章节".to_string()));
            }
            keep_set.insert(*artifact_id);
        }
        if let Some(current_id) = chapter.current_artifact_id {
            keep_set.insert(current_id);
        }
        let artifacts = self.list_artifacts(ArtifactFilters {
            project_id,
            stage: None,
            chapter_id: Some(chapter_id),
        })?;
        let mut to_delete = Vec::new();
        for artifact in &artifacts {
            if keep_set.contains(&artifact.id) {
                continue;
            }
            if self.protected_artifact_reason(&artifact)?.is_some() {
                keep_set.insert(artifact.id);
                continue;
            }
            to_delete.push(artifact.clone());
        }
        let deleted = to_delete
            .iter()
            .map(|artifact| artifact.id)
            .collect::<Vec<_>>();
        let timestamp = now();
        let message = format!(
            "清理章节历史版本：删除 {} 个，保留 {} 个。",
            deleted.len(),
            keep_set.len()
        );
        self.with_conn(|conn| {
            crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, conn)?;
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                for artifact in &to_delete {
                    delete_artifact_search_data_tx(conn, project_id, artifact.id)?;
                    let changed = conn.execute(
                        "DELETE FROM artifacts WHERE id = ?1 AND project_id = ?2 AND chapter_id = ?3",
                        params![artifact.id, project_id, chapter_id],
                    )?;
                    if changed == 0 {
                        return Err(AppError::Validation("清理历史时产物已不存在".to_string()));
                    }
                }
                conn.execute(
                    "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                    params![&timestamp, project_id],
                )?;
                conn.execute(
                    "INSERT INTO messages (project_id, chapter_id, role, content, created_at)
                     VALUES (?1, ?2, 'human_instruction', ?3, ?4)",
                    params![project_id, chapter_id, &message, &timestamp],
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
        })?;
        let mut kept_artifact_ids = keep_set.into_iter().collect::<Vec<_>>();
        kept_artifact_ids.sort_unstable();
        Ok(crate::models::HistoryCleanupResult {
            deleted_artifact_ids: deleted,
            kept_artifact_ids,
        })
    }

    fn protected_artifact_reason(&self, artifact: &Artifact) -> AppResult<Option<String>> {
        if let Some(chapter_id) = artifact.chapter_id {
            let chapter = self
                .ensure_chapter(artifact.project_id, Some(chapter_id))?
                .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
            if chapter.current_artifact_id == Some(artifact.id) {
                return Ok(Some("当前正式正文不能删除".to_string()));
            }
            return Ok(None);
        }
        if self
            .approved_artifact(artifact.project_id, &artifact.stage, None)?
            .is_some_and(|approved| approved.id == artifact.id)
        {
            return Ok(Some(format!(
                "当前已批准的{}不能删除",
                stage_label(&artifact.stage)
            )));
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_workflow_run(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
        stage: &str,
        input: &str,
        output: &str,
        status: &str,
        error: Option<&str>,
        elapsed_ms: i64,
    ) -> AppResult<WorkflowRun> {
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO workflow_runs (project_id, chapter_id, stage, input, output, status, error, elapsed_ms, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![project_id, chapter_id, stage, input, output, status, error, elapsed_ms, now],
            )?;
            let id = conn.last_insert_rowid();
            conn.query_row(
                "SELECT id, project_id, chapter_id, stage, input, output, status, error, elapsed_ms, created_at
                 FROM workflow_runs WHERE id = ?1",
                params![id],
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
            .map_err(AppError::from)
        })
    }

    pub fn update_workflow_run(
        &self,
        run_id: i64,
        output: &str,
        status: &str,
        error: Option<&str>,
        elapsed_ms: i64,
    ) -> AppResult<WorkflowRun> {
        self.with_conn(|conn| {
            let updated = conn.execute(
                "UPDATE workflow_runs
                 SET output = ?1, status = ?2, error = ?3, elapsed_ms = ?4
                 WHERE id = ?5",
                params![output, status, error, elapsed_ms, run_id],
            )?;
            if updated == 0 {
                return Err(AppError::Validation("运行记录不存在".to_string()));
            }
            conn.query_row(
                "SELECT id, project_id, chapter_id, stage, input, output, status, error, elapsed_ms, created_at
                 FROM workflow_runs WHERE id = ?1",
                params![run_id],
                map_workflow_run,
            )
            .map_err(AppError::from)
        })
    }

    pub fn list_workflow_runs(&self, project_id: i64) -> AppResult<Vec<WorkflowRun>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, chapter_id, stage, input, output, status, error, elapsed_ms, created_at
                 FROM workflow_runs WHERE project_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 100",
            )?;
            let rows = stmt.query_map(params![project_id], map_workflow_run)?;
            collect_rows(rows)
        })
    }

    pub fn insert_message(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
        role: &str,
        content: &str,
    ) -> AppResult<Message> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(AppError::Validation("消息内容不能为空".to_string()));
        }
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO messages (project_id, chapter_id, role, content, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![project_id, chapter_id, role, trimmed, now],
            )?;
            let id = conn.last_insert_rowid();
            conn.query_row(
                "SELECT id, project_id, chapter_id, role, content, created_at
                 FROM messages WHERE id = ?1",
                params![id],
                map_message,
            )
            .map_err(AppError::from)
        })
    }

    pub fn list_messages(&self, project_id: i64) -> AppResult<Vec<Message>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, chapter_id, role, content, created_at
                 FROM messages WHERE project_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 200",
            )?;
            let rows = stmt.query_map(params![project_id], map_message)?;
            collect_rows(rows)
        })
    }

    pub fn list_story_threads(&self, project_id: i64) -> AppResult<Vec<StoryThread>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, thread_key, label, kind, status, current_cost, last_seen_chapter_no,
                        last_artifact_id, notes, created_at, updated_at
                 FROM story_threads
                 WHERE project_id = ?1
                 ORDER BY
                    CASE status
                        WHEN 'active' THEN 0
                        WHEN 'due' THEN 1
                        WHEN 'deferred' THEN 2
                        WHEN 'resolved' THEN 3
                        ELSE 4
                    END,
                    updated_at DESC,
                    id DESC",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_thread)?;
            collect_rows(rows)
        })
    }

    pub fn clear_story_threads(&self, project_id: i64) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM story_threads WHERE project_id = ?1",
                params![project_id],
            )?;
            Ok(())
        })
    }

    pub fn list_knowledge_cards(&self, project_id: i64) -> AppResult<Vec<KnowledgeCard>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, category, title, content, status, source_artifact_id, source_chapter_id, created_at, updated_at
                 FROM knowledge_cards WHERE project_id = ?1
                 ORDER BY CASE status WHEN 'approved' THEN 0 WHEN 'pending_human_approval' THEN 1 ELSE 2 END, category, updated_at DESC, id DESC",
            )?;
            let rows = stmt.query_map(params![project_id], map_knowledge_card)?;
            collect_rows(rows)
        })
    }

    pub fn save_knowledge_card(&self, input: SaveKnowledgeCard) -> AppResult<KnowledgeCard> {
        crate::adoption::save_human_knowledge_card(self, input)
    }

    pub fn list_foreshadowings(&self, project_id: i64) -> AppResult<Vec<Foreshadowing>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, title, content, status, planted_chapter_id, planned_payoff_chapter_id,
                        planned_payoff_note, source_artifact_id, created_at, updated_at
                 FROM foreshadowings WHERE project_id = ?1
                 ORDER BY CASE status WHEN 'active' THEN 0 WHEN 'ready_for_payoff' THEN 1 WHEN 'pending_human_approval' THEN 2 WHEN 'resolved' THEN 3 ELSE 4 END,
                          updated_at DESC, id DESC",
            )?;
            let rows = stmt.query_map(params![project_id], map_foreshadowing)?;
            collect_rows(rows)
        })
    }

    pub fn save_foreshadowing(&self, input: SaveForeshadowing) -> AppResult<Foreshadowing> {
        crate::adoption::save_human_foreshadowing(self, input)
    }

    pub fn list_story_entities(&self, project_id: i64) -> AppResult<Vec<StoryEntity>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, kind, name, status, first_seen_chapter_id, source_artifact_id,
                        source_quote, created_at, updated_at
                 FROM story_entities WHERE project_id = ?1
                 ORDER BY CASE kind WHEN 'character' THEN 0 WHEN 'item' THEN 1 WHEN 'resource' THEN 2
                                    WHEN 'location' THEN 3 ELSE 4 END, name COLLATE NOCASE, id",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_entity)?;
            collect_rows(rows)
        })
    }

    pub fn list_story_events(&self, project_id: i64) -> AppResult<Vec<StoryEvent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, title, kind, status, story_time, summary, narrative_chapter_id,
                        source_artifact_id, source_quote, created_at, updated_at
                 FROM story_events WHERE project_id = ?1
                 ORDER BY COALESCE(narrative_chapter_id, 0), id",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_event)?;
            collect_rows(rows)
        })
    }

    pub fn list_story_event_participants(
        &self,
        project_id: i64,
    ) -> AppResult<Vec<StoryEventParticipant>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT p.event_id, p.entity_id, e.name, p.role
                 FROM story_event_participants p
                 INNER JOIN story_entities e ON e.id = p.entity_id
                 INNER JOIN story_events se ON se.id = p.event_id
                 WHERE se.project_id = ?1
                 ORDER BY p.event_id, p.role, e.name COLLATE NOCASE",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_event_participant)?;
            collect_rows(rows)
        })
    }

    pub fn list_story_facts(&self, project_id: i64) -> AppResult<Vec<StoryFact>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, entity_id, event_id, dimension, value, visibility, status,
                        narrative_chapter_id, source_artifact_id, source_quote, supersedes_fact_id, created_at
                 FROM story_facts WHERE project_id = ?1
                 ORDER BY COALESCE(narrative_chapter_id, 0), id",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_fact)?;
            collect_rows(rows)
        })
    }

    pub fn list_story_index_sources(&self, project_id: i64) -> AppResult<Vec<StoryIndexSource>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, chapter_id, source_artifact_id, status, error, indexed_at
                 FROM story_index_sources WHERE project_id = ?1 ORDER BY chapter_id, indexed_at DESC",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_index_source)?;
            collect_rows(rows)
        })
    }

    pub fn list_story_search_sources(&self, project_id: i64) -> AppResult<Vec<StorySearchSource>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage,
                        source_artifact_id, source_text_hash, normalization_version, status, error, indexed_at
                 FROM story_search_sources
                 WHERE project_id = ?1
                 ORDER BY CASE source_kind WHEN 'chapter' THEN 0 WHEN 'artifact' THEN 1 WHEN 'knowledge_card' THEN 2 ELSE 3 END,
                          COALESCE(chapter_no_sort, 0), source_id",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_search_source)?;
            collect_rows(rows)
        })
    }

    pub fn list_derived_index_jobs(&self, project_id: i64) -> AppResult<Vec<DerivedIndexJob>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, chapter_id, source_artifact_id, job_type, scope_key,
                        status, attempt_count, next_attempt_at, last_error, created_at,
                        started_at, finished_at, updated_at
                 FROM derived_index_jobs
                 WHERE project_id = ?1
                 ORDER BY CASE status WHEN 'running' THEN 0 WHEN 'pending' THEN 1 WHEN 'failed' THEN 2 ELSE 3 END,
                          updated_at DESC, id DESC",
            )?;
            let rows = stmt.query_map(params![project_id], map_derived_index_job)?;
            collect_rows(rows)
        })
    }

    pub fn list_writing_skills(&self) -> AppResult<Vec<WritingSkill>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_key, name, category, description, content, enabled, created_at, updated_at
                 FROM writing_skills WHERE category <> 'legacy'
                 ORDER BY category ASC, id ASC",
            )?;
            let rows = stmt.query_map([], map_writing_skill)?;
            collect_rows(rows)
        })
    }

    pub fn get_writing_skill_by_key(&self, skill_key: &str) -> AppResult<Option<WritingSkill>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, skill_key, name, category, description, content, enabled, created_at, updated_at
                 FROM writing_skills WHERE skill_key = ?1",
                params![skill_key],
                map_writing_skill,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn save_writing_skill(&self, input: SaveWritingSkill) -> AppResult<WritingSkill> {
        let skill_key = input.skill_key.trim();
        let name = input.name.trim();
        let content = input.content.trim();
        if skill_key.is_empty() || name.is_empty() || content.is_empty() {
            return Err(AppError::Validation(
                "技能标识、名称和内容不能为空".to_string(),
            ));
        }

        let now = now();
        self.with_conn(|conn| {
            if let Some(id) = input.id {
                let existing_key = conn
                    .query_row(
                        "SELECT skill_key FROM writing_skills WHERE id = ?1",
                        params![id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .ok_or_else(|| AppError::Validation("技能不存在".to_string()))?;
                if existing_key != skill_key {
                    return Err(AppError::Validation("技能标识不能修改".to_string()));
                }
                conn.execute(
                    "UPDATE writing_skills
                     SET name = ?1, category = ?2, description = ?3, content = ?4,
                         enabled = ?5, updated_at = ?6
                     WHERE id = ?7",
                    params![
                        name,
                        input.category.trim(),
                        input.description.trim(),
                        content,
                        if input.enabled { 1 } else { 0 },
                        now,
                        id
                    ],
                )?;
                conn.query_row(
                    "SELECT id, skill_key, name, category, description, content, enabled, created_at, updated_at
                     FROM writing_skills WHERE id = ?1",
                    params![id],
                    map_writing_skill,
                )
                .map_err(AppError::from)
            } else {
                conn.execute(
                    "INSERT INTO writing_skills
                        (skill_key, name, category, description, content, enabled, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                     ON CONFLICT(skill_key) DO UPDATE SET
                        name = excluded.name,
                        category = excluded.category,
                        description = excluded.description,
                        content = excluded.content,
                        enabled = excluded.enabled,
                        updated_at = excluded.updated_at",
                    params![
                        skill_key,
                        name,
                        input.category.trim(),
                        input.description.trim(),
                        content,
                        if input.enabled { 1 } else { 0 },
                        now
                    ],
                )?;
                conn.query_row(
                    "SELECT id, skill_key, name, category, description, content, enabled, created_at, updated_at
                     FROM writing_skills WHERE skill_key = ?1",
                    params![skill_key],
                    map_writing_skill,
                )
                .map_err(AppError::from)
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_story_thread(
        &self,
        project_id: i64,
        thread_key: &str,
        label: &str,
        kind: &str,
        status: &str,
        current_cost: Option<&str>,
        last_seen_chapter_no: Option<i64>,
        last_artifact_id: Option<i64>,
        notes: &str,
    ) -> AppResult<StoryThread> {
        let key = thread_key.trim();
        let label = label.trim();
        if key.is_empty() || label.is_empty() {
            return Err(AppError::Validation(
                "故事线程 key 和 label 不能为空".to_string(),
            ));
        }
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO story_threads (
                    project_id, thread_key, label, kind, status, current_cost, last_seen_chapter_no,
                    last_artifact_id, notes, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
                 ON CONFLICT(project_id, thread_key) DO UPDATE SET
                    label = excluded.label,
                    kind = excluded.kind,
                    status = excluded.status,
                    current_cost = excluded.current_cost,
                    last_seen_chapter_no = excluded.last_seen_chapter_no,
                    last_artifact_id = excluded.last_artifact_id,
                    notes = excluded.notes,
                    updated_at = excluded.updated_at",
                params![
                    project_id,
                    key,
                    label,
                    kind,
                    status,
                    current_cost,
                    last_seen_chapter_no,
                    last_artifact_id,
                    notes.trim(),
                    now
                ],
            )?;
            conn.query_row(
                "SELECT id, project_id, thread_key, label, kind, status, current_cost, last_seen_chapter_no,
                        last_artifact_id, notes, created_at, updated_at
                 FROM story_threads
                 WHERE project_id = ?1 AND thread_key = ?2",
                params![project_id, key],
                map_story_thread,
            )
            .map_err(AppError::from)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_story_thread_statuses_after_chapter(
        &self,
        project_id: i64,
        chapter_no: i64,
        artifact_id: i64,
        active_keys: &[String],
        deferred_keys: &[String],
        resolved_keys: &[String],
        current_cost: Option<&str>,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            let now = now();
            for key in active_keys {
                conn.execute(
                    "UPDATE story_threads
                     SET status = 'active',
                         current_cost = COALESCE(?1, current_cost),
                         last_seen_chapter_no = ?2,
                         last_artifact_id = ?3,
                         updated_at = ?4
                     WHERE project_id = ?5 AND thread_key = ?6",
                    params![current_cost, chapter_no, artifact_id, now, project_id, key],
                )?;
            }
            for key in deferred_keys {
                conn.execute(
                    "UPDATE story_threads
                     SET status = 'deferred',
                         last_seen_chapter_no = COALESCE(last_seen_chapter_no, ?1),
                         updated_at = ?2
                     WHERE project_id = ?3 AND thread_key = ?4",
                    params![chapter_no, now, project_id, key],
                )?;
            }
            for key in resolved_keys {
                conn.execute(
                    "UPDATE story_threads
                     SET status = 'resolved',
                         last_seen_chapter_no = ?1,
                         last_artifact_id = ?2,
                         updated_at = ?3
                     WHERE project_id = ?4 AND thread_key = ?5",
                    params![chapter_no, artifact_id, now, project_id, key],
                )?;
            }
            conn.execute(
                "UPDATE story_threads
                 SET status = CASE
                     WHEN status = 'resolved' THEN status
                     WHEN last_seen_chapter_no IS NULL THEN status
                     WHEN (?1 - last_seen_chapter_no) >= 2 AND status = 'active' THEN 'due'
                     ELSE status
                 END,
                 updated_at = CASE
                     WHEN status = 'resolved' THEN updated_at
                     WHEN last_seen_chapter_no IS NULL THEN updated_at
                     WHEN (?1 - last_seen_chapter_no) >= 2 AND status = 'active' THEN ?2
                     ELSE updated_at
                 END
                 WHERE project_id = ?3",
                params![chapter_no, now, project_id],
            )?;
            Ok(())
        })
    }

    pub fn approve_stage(
        &self,
        project_id: i64,
        stage: &str,
        artifact_id: i64,
        note: &str,
    ) -> AppResult<Approval> {
        let artifact = self.get_artifact(artifact_id)?;
        if artifact.project_id != project_id || artifact.stage != stage {
            return Err(AppError::Validation("审批目标与阶段不匹配".to_string()));
        }
        if matches!(stage, "draft" | "revision") {
            let chapter_id = artifact
                .chapter_id
                .ok_or_else(|| AppError::Validation("正文产物必须属于章节".to_string()))?;
            if self.ensure_chapter(project_id, Some(chapter_id))?.is_none() {
                return Err(AppError::Validation(
                    "正文产物的章节不属于当前项目".to_string(),
                ));
            }
        } else if let Some(chapter_id) = artifact.chapter_id {
            if self.ensure_chapter(project_id, Some(chapter_id))?.is_none() {
                return Err(AppError::Validation("产物章节不属于当前项目".to_string()));
            }
        }
        let now = now();
        let approval = self.with_conn(|conn| {
            crate::story_search::ensure_sqlite_vec_loaded_if_present_on_connection(self, conn)?;
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                if let Some(existing) = conn
                    .query_row(
                        "SELECT id, project_id, chapter_id, stage, artifact_id, note, created_at
                         FROM approvals WHERE artifact_id = ?1 ORDER BY id DESC LIMIT 1",
                        params![artifact_id],
                        map_approval,
                    )
                    .optional()?
                {
                    return Ok(existing);
                }
                conn.execute(
                    "INSERT INTO approvals (project_id, chapter_id, stage, artifact_id, note, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![project_id, artifact.chapter_id, stage, artifact_id, note, &now],
                )?;
                let approval_id = conn.last_insert_rowid();
                conn.execute(
                    "UPDATE artifacts SET status = 'approved' WHERE id = ?1",
                    params![artifact_id],
                )?;
                if matches!(stage, "draft" | "revision") {
                    let chapter_id = artifact.chapter_id.ok_or_else(|| {
                        AppError::Validation("正文产物必须属于章节".to_string())
                    })?;
                    conn.execute(
                        "UPDATE chapters
                         SET status = 'approved', current_artifact_id = ?1, updated_at = ?2
                         WHERE id = ?3 AND project_id = ?4",
                        params![artifact_id, &now, chapter_id, project_id],
                    )?;
                    crate::story_search::invalidate_chapters_from_tx(
                        conn,
                        project_id,
                        chapter_id,
                    )?;
                    invalidate_continuity_ledger_from_chapter_tx(
                        conn,
                        project_id,
                        chapter_id,
                    )?;
                    invalidate_story_index_from_chapter_tx(conn, project_id, chapter_id)?;
                    crate::index_jobs::enqueue_chapter_index_jobs_tx(
                        conn,
                        project_id,
                        chapter_id,
                        artifact_id,
                        &now,
                    )?;
                }
                if matches!(stage, "setting" | "outline" | "characters") {
                    crate::index_jobs::enqueue_project_search_job_tx(conn, project_id, &now)?;
                    mark_story_bible_changed_tx(conn, project_id, &now)?;
                }
                conn.execute(
                    "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                    params![&now, project_id],
                )?;
                let approval = conn.query_row(
                    "SELECT id, project_id, chapter_id, stage, artifact_id, note, created_at
                     FROM approvals WHERE id = ?1",
                    params![approval_id],
                    map_approval,
                )?;
                conn.execute(
                    "INSERT INTO messages (project_id, chapter_id, role, content, created_at)
                     VALUES (?1, ?2, 'approval_note', ?3, ?4)",
                    params![
                        project_id,
                        artifact.chapter_id,
                        format!("{} 已人工通过。{}", stage, note.trim()),
                        &now
                    ],
                )?;
                Ok(approval)
            })();
            match result {
                Ok(approval) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(approval)
                }
                Err(error) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(error)
                }
            }
        })?;
        if stage == "draft" || stage == "revision" {
            if let Err(error) = workflow::sync_story_threads_from_artifact(self, &artifact) {
                eprintln!("story thread refresh unavailable after approval: {error}");
            }
        }
        Ok(approval)
    }

    pub fn list_approvals(&self, project_id: i64) -> AppResult<Vec<Approval>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, chapter_id, stage, artifact_id, note, created_at
                 FROM approvals WHERE project_id = ?1 ORDER BY created_at DESC, id DESC",
            )?;
            let rows = stmt.query_map(params![project_id], map_approval)?;
            collect_rows(rows)
        })
    }

    pub fn save_ai_settings(&self, input: SaveAiSettings) -> AppResult<AiSettings> {
        let base_url = input.base_url.trim().to_string();
        let model = input.model.trim().to_string();
        if base_url.is_empty() {
            return Err(AppError::Validation("AI 地址不能为空".to_string()));
        }
        if model.is_empty() {
            return Err(AppError::Validation("模型名称不能为空".to_string()));
        }
        if !input.temperature.is_finite() || !(0.0..=2.0).contains(&input.temperature) {
            return Err(AppError::Validation(
                "Temperature 必须在 0 到 2 之间".to_string(),
            ));
        }
        let thinking_level =
            crate::models::normalize_thinking_level(input.thinking_enabled, &input.thinking_level)
                .map_err(AppError::Validation)?;
        let api_key = input
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let updated_at = now();

        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                upsert_setting(conn, "ai.base_url", &base_url)?;
                upsert_setting(conn, "ai.model", &model)?;
                upsert_setting(conn, "ai.temperature", &input.temperature.to_string())?;
                upsert_setting(
                    conn,
                    "ai.thinking_enabled",
                    if input.thinking_enabled {
                        "true"
                    } else {
                        "false"
                    },
                )?;
                upsert_setting(conn, "ai.thinking_level", &thinking_level)?;
                conn.execute(
                    "INSERT INTO ai_providers
                        (label, base_url, model, temperature, thinking_enabled, thinking_level, api_key, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                     ON CONFLICT(base_url) DO UPDATE SET
                        model = excluded.model,
                        temperature = excluded.temperature,
                        thinking_enabled = excluded.thinking_enabled,
                        thinking_level = excluded.thinking_level,
                        api_key = CASE
                            WHEN excluded.api_key <> '' THEN excluded.api_key
                            ELSE ai_providers.api_key
                        END,
                        updated_at = excluded.updated_at",
                    params![
                        ai_provider_label(&base_url),
                        base_url,
                        model,
                        input.temperature,
                        input.thinking_enabled as i64,
                        thinking_level,
                        api_key.as_deref().unwrap_or(""),
                        updated_at
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
        })?;
        self.get_ai_settings()
    }

    pub fn get_ai_settings(&self) -> AppResult<AiSettings> {
        let mut settings = self.with_conn(|conn| {
            Ok(AiSettings {
                base_url: get_setting(conn, "ai.base_url")?
                    .unwrap_or_else(|| AiSettings::default().base_url),
                model: get_setting(conn, "ai.model")?
                    .unwrap_or_else(|| AiSettings::default().model),
                temperature: get_setting(conn, "ai.temperature")?
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(AiSettings::default().temperature),
                thinking_enabled: get_setting(conn, "ai.thinking_enabled")?
                    .map(|value| value == "true" || value == "1")
                    .unwrap_or(AiSettings::default().thinking_enabled),
                thinking_level: get_setting(conn, "ai.thinking_level")?.unwrap_or_else(|| {
                    if get_setting(conn, "ai.thinking_enabled")
                        .ok()
                        .flatten()
                        .is_some_and(|value| value == "true" || value == "1")
                    {
                        "medium".to_string()
                    } else {
                        AiSettings::default().thinking_level
                    }
                }),
                has_api_key: false,
            })
        })?;
        settings.has_api_key = self.get_api_key_for_base_url(&settings.base_url)?.is_some();
        Ok(settings)
    }

    pub fn get_api_key(&self) -> AppResult<Option<String>> {
        let settings = self.get_ai_settings()?;
        self.get_api_key_for_base_url(&settings.base_url)
    }

    pub fn get_api_key_for_base_url(&self, base_url: &str) -> AppResult<Option<String>> {
        let normalized = base_url.trim().to_string();
        let stored = self.with_conn(|conn| {
            conn.query_row(
                "SELECT NULLIF(api_key, '') FROM ai_providers WHERE base_url = ?1",
                params![normalized],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(|value| value.flatten())
            .map_err(AppError::from)
        })?;

        Ok(stored)
    }

    pub fn get_story_bible(&self, project_id: i64) -> AppResult<Option<StoryBible>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, project_id, reader_promise, protagonist_engine, core_conflict,
                        endgame_direction, immutable_rules, canon_version, status,
                        source_artifact_id, created_at, updated_at
                 FROM story_bibles WHERE project_id = ?1",
                params![project_id],
                map_story_bible,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn upsert_story_bible_from_artifact(
        &self,
        project_id: i64,
        artifact: &Artifact,
        status: &str,
    ) -> AppResult<StoryBible> {
        let project = self.get_project(project_id)?;
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO story_bibles
                    (project_id, reader_promise, protagonist_engine, core_conflict, endgame_direction,
                     immutable_rules, canon_version, status, source_artifact_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, '', '', '', 1, ?4, ?5, ?6, ?6)
                 ON CONFLICT(project_id) DO UPDATE SET
                    reader_promise = excluded.reader_promise,
                    protagonist_engine = excluded.protagonist_engine,
                    canon_version = story_bibles.canon_version + 1,
                    status = excluded.status,
                    source_artifact_id = excluded.source_artifact_id,
                    updated_at = excluded.updated_at",
                params![
                    project_id,
                    artifact.content,
                    project.premise,
                    status,
                    artifact.id,
                    now
                ],
            )?;
            conn.query_row(
                "SELECT id, project_id, reader_promise, protagonist_engine, core_conflict,
                        endgame_direction, immutable_rules, canon_version, status,
                        source_artifact_id, created_at, updated_at
                 FROM story_bibles WHERE project_id = ?1",
                params![project_id],
                map_story_bible,
            )
            .map_err(AppError::from)
        })
    }

    pub fn list_story_arcs(&self, project_id: i64) -> AppResult<Vec<StoryArc>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, arc_no, title, objective, entry_state, exit_change,
                        core_conflict, involved_characters, chapter_start, chapter_end, status,
                        source_artifact_id, created_at, updated_at
                 FROM story_arcs WHERE project_id = ?1 ORDER BY arc_no",
            )?;
            let rows = stmt.query_map(params![project_id], map_story_arc)?;
            collect_rows(rows)
        })
    }

    pub fn active_story_arc(&self, project_id: i64) -> AppResult<Option<StoryArc>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, project_id, arc_no, title, objective, entry_state, exit_change,
                        core_conflict, involved_characters, chapter_start, chapter_end, status,
                        source_artifact_id, created_at, updated_at
                 FROM story_arcs WHERE project_id = ?1 AND status = 'active'
                 ORDER BY arc_no DESC LIMIT 1",
                params![project_id],
                map_story_arc,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn ensure_active_story_arc_from_outline(
        &self,
        project_id: i64,
        artifact: &Artifact,
    ) -> AppResult<StoryArc> {
        if let Some(active) = self.active_story_arc(project_id)? {
            return Ok(active);
        }
        let now = now();
        self.with_conn(|conn| {
            let arc_no: i64 = conn.query_row(
                "SELECT COALESCE(MAX(arc_no), 0) + 1 FROM story_arcs WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO story_arcs
                    (project_id, arc_no, title, objective, entry_state, exit_change, core_conflict,
                     involved_characters, status, source_artifact_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, '', '', '', '', 'active', ?5, ?6, ?6)",
                params![
                    project_id,
                    arc_no,
                    format!("第 {} 故事阶段", arc_no),
                    artifact.content,
                    artifact.id,
                    now
                ],
            )?;
            conn.query_row(
                "SELECT id, project_id, arc_no, title, objective, entry_state, exit_change,
                        core_conflict, involved_characters, chapter_start, chapter_end, status,
                        source_artifact_id, created_at, updated_at
                 FROM story_arcs WHERE id = ?1",
                params![conn.last_insert_rowid()],
                map_story_arc,
            )
            .map_err(AppError::from)
        })
    }

    pub fn insert_story_bible_review(
        &self,
        project_id: i64,
        fingerprint: &str,
        verdict: &str,
        summary: &str,
        issues_json: &str,
    ) -> AppResult<StoryBibleReview> {
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO story_bible_reviews
                    (project_id, canon_fingerprint, verdict, summary, issues_json, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending_human_confirmation', ?6)",
                params![project_id, fingerprint, verdict, summary, issues_json, now],
            )?;
            query_story_bible_review(conn, conn.last_insert_rowid())
        })
    }

    pub fn latest_story_bible_review(
        &self,
        project_id: i64,
    ) -> AppResult<Option<StoryBibleReview>> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, project_id, canon_fingerprint, verdict, summary, issues_json,
                        status, note, created_at, confirmed_at
                 FROM story_bible_reviews WHERE project_id = ?1 ORDER BY id DESC LIMIT 1",
                params![project_id],
                map_story_bible_review,
            )
            .optional()
            .map_err(AppError::from)
        })
    }

    pub fn confirm_story_bible_review(
        &self,
        project_id: i64,
        review_id: i64,
        note: &str,
    ) -> AppResult<StoryBibleReview> {
        let now = now();
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE story_bible_reviews SET status = 'confirmed', note = ?1, confirmed_at = ?2
                 WHERE id = ?3 AND project_id = ?4 AND status = 'pending_human_confirmation'",
                params![note.trim(), now, review_id, project_id],
            )?;
            if changed == 0 {
                return Err(AppError::Validation("审校记录不存在或已处理".to_string()));
            }
            query_story_bible_review(conn, review_id)
        })
    }

    pub fn mark_story_bible_changed(&self, project_id: i64) -> AppResult<()> {
        self.with_conn(|conn| mark_story_bible_changed_tx(conn, project_id, &now()))
    }

    pub fn mark_story_bible_confirmed(&self, project_id: i64) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE story_bibles SET status = 'confirmed', updated_at = ?1 WHERE project_id = ?2",
                params![now(), project_id],
            )?;
            Ok(())
        })
    }
}

fn backfill_project_genre_agents(conn: &Connection) -> AppResult<()> {
    let projects = {
        let mut stmt = conn.prepare("SELECT id, genre FROM projects")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let assigned_at = now();

    for (project_id, genre) in projects {
        let profile = genre_agent::detect_genre_agent(&genre);
        let assigned_key = conn
            .query_row(
                "SELECT agent_key FROM project_genre_agents WHERE project_id = ?1",
                params![project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if assigned_key.as_deref() == Some("urban_mystery") {
            conn.execute(
                "UPDATE project_genre_agents
                 SET agent_key = ?1, assigned_at = ?2
                 WHERE project_id = ?3",
                params![profile.agent_key, assigned_at, project_id],
            )?;
        } else if assigned_key.is_none() {
            conn.execute(
                "INSERT INTO project_genre_agents (project_id, agent_key, assigned_at)
                 VALUES (?1, ?2, ?3)",
                params![project_id, profile.agent_key, assigned_at],
            )?;
        }
    }
    Ok(())
}

fn resolve_project_genre_agent(conn: &Connection, project_id: i64) -> AppResult<GenreAgentProfile> {
    let project = query_project_by_id(conn, project_id)?;
    let assigned_key = conn
        .query_row(
            "SELECT agent_key FROM project_genre_agents WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    if let Some(profile) = assigned_key
        .as_deref()
        .and_then(genre_agent::profile_for_key)
    {
        return Ok(profile);
    }

    let profile = genre_agent::detect_genre_agent(&project.genre);
    conn.execute(
        "INSERT INTO project_genre_agents (project_id, agent_key, assigned_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(project_id) DO UPDATE SET
            agent_key = excluded.agent_key,
            assigned_at = excluded.assigned_at",
        params![project_id, profile.agent_key, now()],
    )?;
    Ok(profile)
}

fn ai_provider_label(base_url: &str) -> String {
    let normalized = base_url.to_ascii_lowercase();
    if normalized.contains("deepseek") {
        "DeepSeek".to_string()
    } else if normalized.contains("minimax") {
        "MiniMax".to_string()
    } else if normalized.contains("agentrouter.org") {
        "AgentRouter".to_string()
    } else {
        "自定义供应商".to_string()
    }
}

fn default_ai_providers() -> [(
    &'static str,
    &'static str,
    &'static str,
    f64,
    bool,
    &'static str,
); 3] {
    [
        (
            "DeepSeek",
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            0.75,
            false,
            "off",
        ),
        (
            "MiniMax",
            "https://api.minimaxi.com/v1",
            "MiniMax-M3",
            0.72,
            true,
            "medium",
        ),
        (
            "AgentRouter",
            "http://45.145.229.236:38001/v1",
            "kimi-k3",
            0.75,
            false,
            "off",
        ),
    ]
}

fn default_background_agents() -> Vec<(&'static str, &'static str, &'static str, &'static str, f64)>
{
    vec![
        (
            "adoption",
            "资料整理 Agent",
            "从已批准产物提取知识卡与伏笔候选",
            crate::prompt_templates::default_prompt("adoption").unwrap(),
            0.1,
        ),
        (
            "story_index",
            "故事索引 Agent",
            "从已批准正文提取实体、事件和原子事实",
            crate::prompt_templates::default_prompt("story_index").unwrap(),
            0.0,
        ),
        (
            "chapter_memory",
            "章节记忆 Agent",
            "生成下一章使用的事实交接记忆",
            crate::prompt_templates::default_prompt("chapter_memory").unwrap(),
            0.1,
        ),
        (
            "continuity_ledger",
            "状态账本 Agent",
            "从已批准正文提取物件、资源和状态变化",
            crate::prompt_templates::default_prompt("continuity_ledger").unwrap(),
            0.0,
        ),
        (
            "continuity_check",
            "状态核对 Agent",
            "核对候选稿是否越过已批准状态边界",
            crate::prompt_templates::default_prompt("continuity_check").unwrap(),
            0.0,
        ),
        (
            "context_search_plan",
            "上下文规划 Agent",
            "规划需要检索的历史事实",
            crate::prompt_templates::default_prompt("context_search_plan").unwrap(),
            0.0,
        ),
        (
            "context_search_rerank",
            "上下文筛选 Agent",
            "从检索候选中筛选直接相关的原文证据",
            crate::prompt_templates::default_prompt("context_search_rerank").unwrap(),
            0.0,
        ),
        (
            "continuity_review",
            "连续性审校 Agent",
            "审校多章衔接和事实连续性",
            crate::prompt_templates::default_prompt("continuity_review").unwrap(),
            0.0,
        ),
        (
            "chapter_split_plan",
            "拆章规划 Agent",
            "将超载章节拆成可执行的章节任务",
            crate::prompt_templates::default_prompt("chapter_split_plan").unwrap(),
            0.0,
        ),
        (
            "artifact_revision",
            "局部修订 Agent",
            "只重写用户指定的局部片段",
            crate::prompt_templates::default_prompt("artifact_revision").unwrap(),
            0.35,
        ),
    ]
}

fn default_agents() -> Vec<(&'static str, &'static str, &'static str, &'static str, f64)> {
    vec![
        (
            "story_architect",
            "故事架构 Agent",
            "负责统一维护创作基准、阶段大纲、角色与事实一致性",
            crate::prompt_templates::default_prompt("story_architect").unwrap(),
            0.62,
        ),
        (
            "draft",
            "写作 Agent",
            "负责章节正文草稿",
            crate::prompt_templates::default_prompt("draft").unwrap(),
            0.78,
        ),
        (
            "review",
            "试读 Agent",
            "负责挑出问题",
            crate::prompt_templates::default_prompt("review").unwrap(),
            0.35,
        ),
        (
            "revision",
            "修订 Agent",
            "负责根据反馈改稿",
            crate::prompt_templates::default_prompt("revision").unwrap(),
            0.64,
        ),
    ]
}

fn set_default_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

fn upsert_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

fn patch_default_writing_skills(conn: &Connection) -> AppResult<()> {
    let now = now();

    conn.execute(
        "UPDATE writing_skills
         SET name = '旧版都市异能/悬疑（已停用）',
             category = 'legacy',
             description = '已拆分为 urban_supernatural 与 mystery；保留原内容仅用于历史追溯。',
             enabled = 0,
             updated_at = ?1
         WHERE skill_key = 'urban_mystery' AND category = 'genre'",
        params![now],
    )?;

    if let Some(existing) = conn
        .query_row(
            "SELECT content FROM writing_skills WHERE skill_key = 'general_serialized'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        let is_rigid_built_in = existing.contains("上一章抛出的最强钩子")
            && existing.contains("章末必须留下更具体的下一步")
            && existing.contains("大纲阶段：每章必须写清章节功能");
        if is_rigid_built_in {
            conn.execute(
                "UPDATE writing_skills
                 SET content = ?1, updated_at = ?2
                 WHERE skill_key = 'general_serialized'",
                params![
                    crate::genre_skill::GenreSkillKind::GeneralSerialized.fallback_template(),
                    now
                ],
            )?;
        }
    }

    conn.execute(
        "UPDATE writing_skills
         SET content = replace(content, ?1, ?2), updated_at = ?3
         WHERE skill_key = 'xianxia_power_fantasy' AND instr(content, ?1) > 0",
        params![
            "- 修订阶段：若章末钩子不足，优先补新的行动压力、外部变化、回收代价或更高层次的追查，不要只补一句“他决定继续变强”。",
            "- 修订阶段：若结尾功能不足，优先把本章已有的成长结果、资源代价、关系变化、决定或下一步行动落稳；只有前文已经存在相应压力时才推进追查或威胁，不凭空补新危险。",
            now
        ],
    )?;

    if let Some(existing) = conn
        .query_row(
            "SELECT content FROM writing_skills WHERE skill_key = 'xianxia_power_fantasy'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        if existing.contains("## App Library Patch: continuity-and-agency")
            || existing.contains("黑牌、碎片、布片、刻字、梦境、旁人口供")
            || existing.contains("重要物件、禁制、令牌、炉门、阵眼、药物、伤势")
            || (existing.contains("把“前期能拿到的第一口肉”")
                && !existing.contains("代价必须可在故事现场被观察"))
            || (existing.contains("代价必须可在故事现场被观察")
                && !existing.contains("已批准设定是能力合同"))
        {
            conn.execute(
                "UPDATE writing_skills
                 SET content = ?1, updated_at = ?2
                 WHERE skill_key = 'xianxia_power_fantasy'",
                params![
                    crate::genre_skill::GenreSkillKind::XianxiaPowerFantasy.fallback_template(),
                    now
                ],
            )?;
        }
    }

    if let Some(existing) = conn
        .query_row(
            "SELECT content FROM writing_skills WHERE skill_key = 'continuity_and_agency'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        if existing.trim().is_empty() {
            conn.execute(
                "UPDATE writing_skills
                 SET content = ?1, updated_at = ?2
                 WHERE skill_key = 'continuity_and_agency'",
                params![
                    crate::genre_skill::default_writing_skills()
                        .into_iter()
                        .find(|skill| skill.skill_key == "continuity_and_agency")
                        .map(|skill| skill.content)
                        .unwrap_or(""),
                    now
                ],
            )?;
        }
    }

    Ok(())
}

fn query_project_by_id(conn: &Connection, id: i64) -> AppResult<Project> {
    conn.query_row(
        "SELECT id, title, genre, target_words, premise, status, created_at, updated_at
         FROM projects WHERE id = ?1",
        params![id],
        map_project,
    )
    .optional()?
    .ok_or_else(|| AppError::Validation("项目不存在".to_string()))
}

fn query_artifact_by_id(conn: &Connection, id: i64) -> AppResult<Artifact> {
    conn.query_row(
        "SELECT id, project_id, chapter_id, stage, title, content, version, status, parent_artifact_id, created_at
         FROM artifacts WHERE id = ?1",
        params![id],
        map_artifact,
    )
    .optional()?
    .ok_or_else(|| AppError::Validation("产物不存在".to_string()))
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn retryable_artifact_insert_error(error: &AppError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || (message.contains("unique constraint failed") && message.contains("artifacts"))
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

fn invalidate_continuity_ledger_from_chapter_tx(
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
    conn.execute(
        "DELETE FROM continuity_ledger_entries
         WHERE project_id = ?1 AND chapter_id IN
           (SELECT id FROM chapters WHERE project_id = ?1 AND chapter_no >= ?2)",
        params![project_id, chapter_no],
    )?;
    conn.execute(
        "DELETE FROM continuity_ledger_sources
         WHERE project_id = ?1 AND chapter_id IN
           (SELECT id FROM chapters WHERE project_id = ?1 AND chapter_no >= ?2)",
        params![project_id, chapter_no],
    )?;
    Ok(())
}

fn invalidate_story_index_from_chapter_tx(
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
    conn.execute(
        "DELETE FROM story_facts
         WHERE project_id = ?1 AND narrative_chapter_id IN
           (SELECT id FROM chapters WHERE project_id = ?1 AND chapter_no >= ?2)",
        params![project_id, chapter_no],
    )?;
    conn.execute(
        "DELETE FROM story_events
         WHERE project_id = ?1 AND narrative_chapter_id IN
           (SELECT id FROM chapters WHERE project_id = ?1 AND chapter_no >= ?2)",
        params![project_id, chapter_no],
    )?;
    conn.execute(
        "DELETE FROM story_index_sources
         WHERE project_id = ?1 AND chapter_id IN
           (SELECT id FROM chapters WHERE project_id = ?1 AND chapter_no >= ?2)",
        params![project_id, chapter_no],
    )?;
    // An entity registry entry without a surviving fact or event participant
    // came only from invalidated text and is no longer usable canon.
    conn.execute(
        "DELETE FROM story_entities
         WHERE project_id = ?1
           AND NOT EXISTS (SELECT 1 FROM story_facts f WHERE f.entity_id = story_entities.id)
           AND NOT EXISTS (SELECT 1 FROM story_event_participants p WHERE p.entity_id = story_entities.id)",
        params![project_id],
    )?;
    Ok(())
}

pub(crate) fn mark_story_bible_changed_tx(
    conn: &Connection,
    project_id: i64,
    timestamp: &str,
) -> AppResult<()> {
    conn.execute(
        "UPDATE story_bibles
         SET canon_version = canon_version + 1, status = 'needs_review', updated_at = ?1
         WHERE project_id = ?2",
        params![timestamp, project_id],
    )?;
    Ok(())
}

fn delete_project_search_data_tx(conn: &Connection, project_id: i64) -> AppResult<()> {
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings
             WHERE rowid IN (SELECT id FROM story_search_documents WHERE project_id = ?1)",
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
}

fn delete_chapter_search_data_tx(
    conn: &Connection,
    project_id: i64,
    chapter_id: i64,
    chapter_no: i64,
) -> AppResult<()> {
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings
             WHERE rowid IN (
                 SELECT id FROM story_search_documents
                 WHERE project_id = ?1
                   AND (chapter_id = ?2 OR (chapter_no_sort IS NOT NULL AND chapter_no_sort >= ?3))
             )",
            params![project_id, chapter_id, chapter_no],
        )?;
    }
    conn.execute(
        "DELETE FROM story_search_documents
         WHERE project_id = ?1
           AND (chapter_id = ?2 OR (chapter_no_sort IS NOT NULL AND chapter_no_sort >= ?3))",
        params![project_id, chapter_id, chapter_no],
    )?;
    conn.execute(
        "DELETE FROM story_search_sources
         WHERE project_id = ?1
           AND (chapter_id = ?2 OR (chapter_no_sort IS NOT NULL AND chapter_no_sort >= ?3))",
        params![project_id, chapter_id, chapter_no],
    )?;
    Ok(())
}

fn delete_artifact_search_data_tx(
    conn: &Connection,
    project_id: i64,
    artifact_id: i64,
) -> AppResult<()> {
    if table_exists(conn, "story_search_embeddings")? {
        conn.execute(
            "DELETE FROM story_search_embeddings
             WHERE rowid IN (
                 SELECT id FROM story_search_documents
                 WHERE project_id = ?1
                   AND source_kind = 'artifact' AND source_id = ?2
             )",
            params![project_id, artifact_id],
        )?;
    }
    conn.execute(
        "DELETE FROM story_search_documents
         WHERE project_id = ?1
           AND source_kind = 'artifact' AND source_id = ?2",
        params![project_id, artifact_id],
    )?;
    conn.execute(
        "DELETE FROM story_search_sources
         WHERE project_id = ?1
           AND (source_artifact_id = ?2 OR (source_kind = 'artifact' AND source_id = ?2))",
        params![project_id, artifact_id],
    )?;
    Ok(())
}

fn map_ai_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiProvider> {
    Ok(AiProvider {
        id: row.get(0)?,
        label: row.get(1)?,
        base_url: row.get(2)?,
        model: row.get(3)?,
        temperature: row.get(4)?,
        thinking_enabled: row.get::<_, i64>(5)? != 0,
        thinking_level: row.get(6)?,
        tool_protocol: crate::models::ToolProtocol::parse(&row.get::<_, String>(7)?),
        has_api_key: false,
    })
}

fn map_agent(row: &rusqlite::Row<'_>) -> rusqlite::Result<Agent> {
    let system_prompt: String = row.get(4)?;
    let enabled_tool_keys = parse_agent_keys(&row.get::<_, String>(11)?, true);
    let allowed_skill_keys = parse_agent_keys(&row.get::<_, String>(12)?, false);
    Ok(Agent {
        id: row.get(0)?,
        stage: row.get(1)?,
        name: row.get(2)?,
        role: row.get(3)?,
        editable_role: row.get(3)?,
        system_prompt: system_prompt.clone(),
        editable_system_prompt: system_prompt,
        temperature: row.get(5)?,
        provider_base_url: row.get(6)?,
        model: row.get(7)?,
        thinking_enabled: row.get::<_, i64>(8)? != 0,
        thinking_level: row.get(9)?,
        uses_global_runtime_settings: row.get::<_, i64>(10)? != 0,
        enabled_tool_keys,
        allowed_skill_keys,
    })
}

fn parse_agent_keys(raw: &str, tools: bool) -> Vec<String> {
    let parsed = serde_json::from_str::<Vec<String>>(raw).unwrap_or_default();
    if tools {
        agent_tools::normalize_keys(&parsed)
    } else {
        normalize_skill_keys(&parsed)
    }
}

fn normalize_skill_keys(keys: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    keys.iter()
        .map(|key| key.trim())
        .filter(|key| !key.is_empty())
        .filter(|key| seen.insert((*key).to_string()))
        .map(ToOwned::to_owned)
        .collect()
}

fn map_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        title: row.get(1)?,
        genre: row.get(2)?,
        target_words: row.get(3)?,
        premise: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn map_chapter(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chapter> {
    Ok(Chapter {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_no: row.get(2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        current_artifact_id: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn map_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_id: row.get(2)?,
        stage: row.get(3)?,
        title: row.get(4)?,
        content: row.get(5)?,
        version: row.get(6)?,
        status: row.get(7)?,
        parent_artifact_id: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn map_story_bible(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryBible> {
    Ok(StoryBible {
        id: row.get(0)?,
        project_id: row.get(1)?,
        reader_promise: row.get(2)?,
        protagonist_engine: row.get(3)?,
        core_conflict: row.get(4)?,
        endgame_direction: row.get(5)?,
        immutable_rules: row.get(6)?,
        canon_version: row.get(7)?,
        status: row.get(8)?,
        source_artifact_id: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn map_story_arc(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryArc> {
    Ok(StoryArc {
        id: row.get(0)?,
        project_id: row.get(1)?,
        arc_no: row.get(2)?,
        title: row.get(3)?,
        objective: row.get(4)?,
        entry_state: row.get(5)?,
        exit_change: row.get(6)?,
        core_conflict: row.get(7)?,
        involved_characters: row.get(8)?,
        chapter_start: row.get(9)?,
        chapter_end: row.get(10)?,
        status: row.get(11)?,
        source_artifact_id: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn query_story_bible_review(conn: &Connection, id: i64) -> AppResult<StoryBibleReview> {
    conn.query_row(
        "SELECT id, project_id, canon_fingerprint, verdict, summary, issues_json,
                status, note, created_at, confirmed_at
         FROM story_bible_reviews WHERE id = ?1",
        params![id],
        map_story_bible_review,
    )
    .map_err(AppError::from)
}

fn map_story_bible_review(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryBibleReview> {
    let issues_json: String = row.get(5)?;
    let issues = serde_json::from_str(&issues_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(StoryBibleReview {
        id: row.get(0)?,
        project_id: row.get(1)?,
        canon_fingerprint: row.get(2)?,
        verdict: row.get(3)?,
        summary: row.get(4)?,
        issues,
        status: row.get(6)?,
        note: row.get(7)?,
        created_at: row.get(8)?,
        confirmed_at: row.get(9)?,
    })
}

fn map_approval(row: &rusqlite::Row<'_>) -> rusqlite::Result<Approval> {
    Ok(Approval {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_id: row.get(2)?,
        stage: row.get(3)?,
        artifact_id: row.get(4)?,
        note: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_id: row.get(2)?,
        role: row.get(3)?,
        content: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn map_workflow_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowRun> {
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
}

fn map_chapter_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChapterMemoryRecord> {
    Ok(ChapterMemoryRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_id: row.get(2)?,
        source_artifact_id: row.get(3)?,
        source_text_hash: row.get(4)?,
        normalization_version: row.get(5)?,
        content: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn map_continuity_ledger_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContinuityLedgerEntry> {
    Ok(ContinuityLedgerEntry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        chapter_id: row.get(2)?,
        source_artifact_id: row.get(3)?,
        source_text_hash: row.get(4)?,
        normalization_version: row.get(5)?,
        entity_kind: row.get(6)?,
        entity_key: row.get(7)?,
        entity_label: row.get(8)?,
        state_kind: row.get(9)?,
        state_value: row.get(10)?,
        evidence_quote: row.get(11)?,
        created_at: row.get(12)?,
    })
}

fn map_story_thread(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryThread> {
    Ok(StoryThread {
        id: row.get(0)?,
        project_id: row.get(1)?,
        thread_key: row.get(2)?,
        label: row.get(3)?,
        kind: row.get(4)?,
        status: row.get(5)?,
        current_cost: row.get(6)?,
        last_seen_chapter_no: row.get(7)?,
        last_artifact_id: row.get(8)?,
        notes: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn map_story_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryEntity> {
    Ok(StoryEntity {
        id: row.get(0)?,
        project_id: row.get(1)?,
        kind: row.get(2)?,
        name: row.get(3)?,
        status: row.get(4)?,
        first_seen_chapter_id: row.get(5)?,
        source_artifact_id: row.get(6)?,
        source_quote: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn map_story_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryEvent> {
    Ok(StoryEvent {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        kind: row.get(3)?,
        status: row.get(4)?,
        story_time: row.get(5)?,
        summary: row.get(6)?,
        narrative_chapter_id: row.get(7)?,
        source_artifact_id: row.get(8)?,
        source_quote: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn map_story_event_participant(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryEventParticipant> {
    Ok(StoryEventParticipant {
        event_id: row.get(0)?,
        entity_id: row.get(1)?,
        entity_name: row.get(2)?,
        role: row.get(3)?,
    })
}

fn map_story_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryFact> {
    Ok(StoryFact {
        id: row.get(0)?,
        project_id: row.get(1)?,
        entity_id: row.get(2)?,
        event_id: row.get(3)?,
        dimension: row.get(4)?,
        value: row.get(5)?,
        visibility: row.get(6)?,
        status: row.get(7)?,
        narrative_chapter_id: row.get(8)?,
        source_artifact_id: row.get(9)?,
        source_quote: row.get(10)?,
        supersedes_fact_id: row.get(11)?,
        created_at: row.get(12)?,
    })
}

fn map_story_index_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoryIndexSource> {
    Ok(StoryIndexSource {
        project_id: row.get(0)?,
        chapter_id: row.get(1)?,
        source_artifact_id: row.get(2)?,
        status: row.get(3)?,
        error: row.get(4)?,
        indexed_at: row.get(5)?,
    })
}

fn map_story_search_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<StorySearchSource> {
    Ok(StorySearchSource {
        project_id: row.get(0)?,
        source_kind: row.get(1)?,
        source_id: row.get(2)?,
        chapter_id: row.get(3)?,
        chapter_no_sort: row.get(4)?,
        stage: row.get(5)?,
        source_artifact_id: row.get(6)?,
        source_text_hash: row.get(7)?,
        normalization_version: row.get(8)?,
        status: row.get(9)?,
        error: row.get(10)?,
        indexed_at: row.get(11)?,
    })
}

fn map_derived_index_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<DerivedIndexJob> {
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

fn map_knowledge_card(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeCard> {
    Ok(KnowledgeCard {
        id: row.get(0)?,
        project_id: row.get(1)?,
        category: row.get(2)?,
        title: row.get(3)?,
        content: row.get(4)?,
        status: row.get(5)?,
        source_artifact_id: row.get(6)?,
        source_chapter_id: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn map_foreshadowing(row: &rusqlite::Row<'_>) -> rusqlite::Result<Foreshadowing> {
    Ok(Foreshadowing {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        content: row.get(3)?,
        status: row.get(4)?,
        planted_chapter_id: row.get(5)?,
        planned_payoff_chapter_id: row.get(6)?,
        planned_payoff_note: row.get(7)?,
        source_artifact_id: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn map_writing_skill(row: &rusqlite::Row<'_>) -> rusqlite::Result<WritingSkill> {
    let enabled: i64 = row.get(6)?;
    Ok(WritingSkill {
        id: row.get(0)?,
        skill_key: row.get(1)?,
        name: row.get(2)?,
        category: row.get(3)?,
        description: row.get(4)?,
        content: row.get(5)?,
        enabled: enabled != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> AppResult<Vec<T>> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn stage_label(stage: &str) -> &'static str {
    match stage {
        "setting" => "设定",
        "outline" => "大纲",
        "characters" => "角色",
        "draft" => "章节草稿",
        "review" => "试读报告",
        "revision" => "修订稿",
        _ => "产物",
    }
}

fn dedupe_approvals(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        DELETE FROM approvals
        WHERE id NOT IN (
            SELECT MIN(id)
            FROM approvals
            GROUP BY artifact_id
        );

        DELETE FROM messages
        WHERE role = 'approval_note'
          AND id NOT IN (
              SELECT MIN(id)
              FROM messages
              WHERE role = 'approval_note'
              GROUP BY project_id, chapter_id, content
          );

        DELETE FROM messages
        WHERE role = 'approval_note'
          AND id NOT IN (
              SELECT MIN(id)
              FROM messages
              WHERE role = 'approval_note'
              GROUP BY project_id, chapter_id,
                       substr(content, 1, instr(content || '。', '。'))
          );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_approvals_artifact_once
        ON approvals(artifact_id);
        "#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ToolProtocol;

    #[test]
    fn initializes_database_and_defaults() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let agents = state.list_agents().unwrap();
        let settings = state.get_ai_settings().unwrap();
        let skills = state.list_writing_skills().unwrap();

        assert!(agents.len() >= 14);
        assert!(agents.iter().any(|agent| agent.stage == "story_architect"));
        assert!(agents
            .iter()
            .any(|agent| agent.stage == "context_search_plan"));
        assert_eq!(
            agents
                .iter()
                .find(|agent| agent.stage == "context_search_rerank")
                .unwrap()
                .model,
            "deepseek-v4-flash"
        );
        assert!(!agents.iter().any(|agent| agent.stage == "setting"));
        assert_eq!(settings.base_url, "https://api.deepseek.com");
        assert_eq!(settings.model, "deepseek-v4-pro");
        let draft_agent = agents.iter().find(|agent| agent.stage == "draft").unwrap();
        assert_eq!(draft_agent.provider_base_url, settings.base_url);
        assert_eq!(draft_agent.model, settings.model);
        assert!(!draft_agent.thinking_enabled);
        state
            .save_ai_settings(SaveAiSettings {
                base_url: settings.base_url.clone(),
                model: settings.model.clone(),
                temperature: settings.temperature,
                thinking_enabled: true,
                thinking_level: "medium".to_string(),
                api_key: None,
            })
            .unwrap();
        assert!(
            state
                .list_agents()
                .unwrap()
                .into_iter()
                .find(|agent| agent.stage == "draft")
                .unwrap()
                .thinking_enabled
        );
        let saved_agent = state
            .save_agent_settings(SaveAgentSettings {
                agent_id: draft_agent.id,
                provider_base_url: "https://example.test/v1".to_string(),
                model: "example-model".to_string(),
                name: Some("自定义写作".to_string()),
                role: Some("只写章节正文".to_string()),
                system_prompt: Some("只输出正文，不输出解释。".to_string()),
                temperature: Some(0.9),
                thinking_enabled: true,
                thinking_level: Some("high".to_string()),
                uses_global_runtime_settings: Some(false),
                enabled_tool_keys: Some(vec![
                    agent_tools::CHAPTER_SPLIT.to_string(),
                    "unknown_tool".to_string(),
                ]),
                allowed_skill_keys: Some(vec![
                    "continuity_and_agency".to_string(),
                    "unknown_skill".to_string(),
                ]),
            })
            .unwrap();
        assert_eq!(saved_agent.provider_base_url, "https://example.test/v1");
        assert_eq!(saved_agent.model, "example-model");
        assert_eq!(saved_agent.name, "自定义写作");
        assert_eq!(saved_agent.editable_role, "只写章节正文");
        assert_eq!(
            saved_agent.editable_system_prompt,
            "只输出正文，不输出解释。"
        );
        assert_eq!(saved_agent.temperature, 0.9);
        assert!(saved_agent.thinking_enabled);
        assert_eq!(saved_agent.thinking_level, "high");
        assert_eq!(
            saved_agent.enabled_tool_keys,
            vec![agent_tools::CHAPTER_SPLIT.to_string()]
        );
        assert_eq!(
            saved_agent.allowed_skill_keys,
            vec![
                "continuity_and_agency".to_string(),
                "unknown_skill".to_string()
            ]
        );
        assert!(skills
            .iter()
            .any(|skill| skill.skill_key == "xianxia_power_fantasy"));
        assert!(skills
            .iter()
            .any(|skill| skill.skill_key == "continuity_and_agency"));
        assert!(skills
            .iter()
            .any(|skill| skill.skill_key == "urban_supernatural"));
        assert!(skills.iter().any(|skill| skill.skill_key == "mystery"));
    }

    #[test]
    fn preserves_an_explicitly_empty_agent_tool_allowlist_after_reopen() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        let agent = state
            .list_agents()
            .unwrap()
            .into_iter()
            .find(|item| item.stage == "draft")
            .unwrap();

        let saved = state
            .save_agent_settings(SaveAgentSettings {
                agent_id: agent.id,
                provider_base_url: String::new(),
                model: String::new(),
                name: None,
                role: None,
                system_prompt: None,
                temperature: None,
                thinking_enabled: false,
                thinking_level: Some("off".to_string()),
                uses_global_runtime_settings: Some(true),
                enabled_tool_keys: Some(Vec::new()),
                allowed_skill_keys: Some(Vec::new()),
            })
            .unwrap();
        assert!(saved.enabled_tool_keys.is_empty());

        drop(state);
        let reopened = AppState::from_path(path).unwrap();
        let reloaded = reopened
            .list_agents()
            .unwrap()
            .into_iter()
            .find(|item| item.stage == "draft")
            .unwrap();
        assert!(reloaded.enabled_tool_keys.is_empty());
    }

    #[test]
    fn persists_ai_provider_catalog_in_database() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();

        let defaults = state.list_ai_providers().unwrap();
        assert!(defaults.iter().any(|provider| {
            provider.base_url == "http://45.145.229.236:38001/v1" && provider.model == "kimi-k3"
        }));

        let saved = state
            .save_ai_provider(SaveAiProvider {
                id: None,
                label: "测试供应商".to_string(),
                base_url: "https://provider.example/v1".to_string(),
                model: "test-model".to_string(),
                temperature: 0.4,
                thinking_enabled: true,
                thinking_level: "medium".to_string(),
                tool_protocol: ToolProtocol::Auto,
            })
            .unwrap();
        assert_eq!(saved.label, "测试供应商");

        let updated = state
            .save_ai_provider(SaveAiProvider {
                id: Some(saved.id),
                label: "更新后的供应商".to_string(),
                base_url: saved.base_url.clone(),
                model: "updated-model".to_string(),
                temperature: 0.6,
                thinking_enabled: false,
                thinking_level: "off".to_string(),
                tool_protocol: ToolProtocol::Auto,
            })
            .unwrap();
        assert_eq!(updated.label, "更新后的供应商");
        assert_eq!(updated.model, "updated-model");

        state.delete_ai_provider(updated.id).unwrap();
        assert!(!state
            .list_ai_providers()
            .unwrap()
            .iter()
            .any(|provider| provider.id == updated.id));
    }

    #[test]
    fn persists_api_key_in_provider_database() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let base_url = "https://provider.example/v1";

        let settings = state
            .save_ai_settings(SaveAiSettings {
                base_url: base_url.to_string(),
                model: "test-model".to_string(),
                temperature: 0.5,
                thinking_enabled: false,
                thinking_level: "off".to_string(),
                api_key: Some("database-key".to_string()),
            })
            .unwrap();

        assert!(settings.has_api_key);
        assert_eq!(
            state.get_api_key_for_base_url(base_url).unwrap().as_deref(),
            Some("database-key")
        );
        let stored_key = state
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT api_key FROM ai_providers WHERE base_url = ?1",
                    params![base_url],
                    |row| row.get::<_, String>(0),
                )
                .map_err(AppError::from)
            })
            .unwrap();
        assert_eq!(stored_key, "database-key");
    }

    #[test]
    fn deleting_active_ai_provider_switches_global_settings_to_fallback() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let active = state
            .save_ai_provider(SaveAiProvider {
                id: None,
                label: "当前测试供应商".to_string(),
                base_url: "https://active-provider.example/v1".to_string(),
                model: "active-model".to_string(),
                temperature: 0.4,
                thinking_enabled: true,
                thinking_level: "medium".to_string(),
                tool_protocol: ToolProtocol::Auto,
            })
            .unwrap();
        state
            .save_ai_settings(SaveAiSettings {
                base_url: active.base_url.clone(),
                model: active.model.clone(),
                temperature: active.temperature,
                thinking_enabled: active.thinking_enabled,
                thinking_level: "medium".to_string(),
                api_key: None,
            })
            .unwrap();

        state.delete_ai_provider(active.id).unwrap();

        let fallback = state
            .list_ai_providers()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let settings = state.get_ai_settings().unwrap();
        assert_eq!(settings.base_url, fallback.base_url);
        assert_eq!(settings.model, fallback.model);
        assert_eq!(settings.temperature, fallback.temperature);
        assert_eq!(settings.thinking_enabled, fallback.thinking_enabled);
    }

    #[test]
    fn project_keeps_its_assigned_genre_agent_when_metadata_changes() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "专属 Agent 绑定".to_string(),
                genre: "悬疑".to_string(),
                target_words: 120000,
                premise: "测试项目绑定".to_string(),
            })
            .unwrap();

        assert_eq!(
            state
                .get_genre_agent_for_project(project.id)
                .unwrap()
                .agent_key,
            "mystery"
        );

        state
            .update_project(ProjectUpdate {
                id: project.id,
                title: project.title,
                genre: "男频修仙".to_string(),
                target_words: project.target_words,
                premise: project.premise,
                status: project.status,
            })
            .unwrap();

        assert_eq!(
            state
                .get_genre_agent_for_project(project.id)
                .unwrap()
                .agent_key,
            "mystery"
        );
    }

    #[test]
    fn migrates_the_legacy_combined_agent_by_project_genre() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        let urban_project = state
            .create_project(NewProject {
                title: "都市异能旧绑定".to_string(),
                genre: "都市异能".to_string(),
                target_words: 120000,
                premise: "测试迁移".to_string(),
            })
            .unwrap();
        let mystery_project = state
            .create_project(NewProject {
                title: "悬疑旧绑定".to_string(),
                genre: "悬疑".to_string(),
                target_words: 120000,
                premise: "测试迁移".to_string(),
            })
            .unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE project_genre_agents SET agent_key = 'urban_mystery'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        state
            .save_writing_skill(SaveWritingSkill {
                id: None,
                skill_key: "urban_mystery".to_string(),
                name: "都市异能/悬疑".to_string(),
                category: "genre".to_string(),
                description: "旧合并技能".to_string(),
                content: "## Always\n旧合并规则".to_string(),
                enabled: true,
            })
            .unwrap();
        drop(state);

        let migrated = AppState::from_path(path).unwrap();
        assert_eq!(
            migrated
                .get_genre_agent_for_project(urban_project.id)
                .unwrap()
                .agent_key,
            "urban_supernatural"
        );
        assert_eq!(
            migrated
                .get_genre_agent_for_project(mystery_project.id)
                .unwrap()
                .agent_key,
            "mystery"
        );
        let legacy = migrated
            .get_writing_skill_by_key("urban_mystery")
            .unwrap()
            .unwrap();
        assert_eq!(legacy.category, "legacy");
        assert!(!legacy.enabled);
        assert!(!migrated
            .list_writing_skills()
            .unwrap()
            .iter()
            .any(|skill| skill.skill_key == "urban_mystery"));
    }

    #[test]
    fn saves_writing_skill_to_app_library() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let saved = state
            .save_writing_skill(SaveWritingSkill {
                id: None,
                skill_key: "xianxia_power_fantasy".to_string(),
                name: "男频修仙爽文".to_string(),
                category: "genre".to_string(),
                description: "测试覆盖".to_string(),
                content: "# Xianxia Power Fantasy Skill\n\n## Always\n- 测试规则\n".to_string(),
                enabled: true,
            })
            .unwrap();
        let loaded = state
            .get_writing_skill_by_key("xianxia_power_fantasy")
            .unwrap()
            .unwrap();

        assert_eq!(saved.description, "测试覆盖");
        assert!(loaded.content.contains("测试规则"));
    }

    #[test]
    fn chapter_update_cannot_cross_project_boundary() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let first = state
            .create_project(NewProject {
                title: "第一本书".to_string(),
                genre: "悬疑".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let second = state
            .create_project(NewProject {
                title: "第二本书".to_string(),
                genre: "奇幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(first.id).unwrap().remove(0);

        let error = state
            .update_chapter(ChapterUpdate {
                project_id: second.id,
                id: chapter.id,
                title: "越权改名".to_string(),
                status: chapter.status.clone(),
            })
            .unwrap_err();

        assert!(matches!(error, AppError::Validation(message) if message == "章节不存在"));
        assert_eq!(
            state
                .ensure_chapter(first.id, Some(chapter.id))
                .unwrap()
                .unwrap()
                .title,
            chapter.title
        );
    }

    #[test]
    fn writing_skill_key_cannot_be_changed_when_editing() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let saved = state
            .save_writing_skill(SaveWritingSkill {
                id: None,
                skill_key: "stable_skill_key".to_string(),
                name: "稳定标识测试".to_string(),
                category: "craft".to_string(),
                description: "测试技能标识不可变".to_string(),
                content: "## 规则\n- 保留标识".to_string(),
                enabled: true,
            })
            .unwrap();

        let error = state
            .save_writing_skill(SaveWritingSkill {
                id: Some(saved.id),
                skill_key: "renamed_skill_key".to_string(),
                name: saved.name.clone(),
                category: saved.category.clone(),
                description: saved.description.clone(),
                content: saved.content.clone(),
                enabled: saved.enabled,
            })
            .unwrap_err();

        assert!(matches!(error, AppError::Validation(message) if message == "技能标识不能修改"));
        assert!(state
            .get_writing_skill_by_key("stable_skill_key")
            .unwrap()
            .is_some());
        assert!(state
            .get_writing_skill_by_key("renamed_skill_key")
            .unwrap()
            .is_none());
    }

    #[test]
    fn saves_dynamic_knowledge_and_foreshadowing_with_chapter_targets() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "资料账本测试".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let foundation = state
            .insert_artifact(
                project.id,
                None,
                "setting",
                "基础设定",
                "这是资料账本回归测试使用的基础设定。",
                None,
            )
            .unwrap();
        state
            .upsert_story_bible_from_artifact(project.id, &foundation, "confirmed")
            .unwrap();
        let card = state
            .save_knowledge_card(SaveKnowledgeCard {
                id: None,
                project_id: project.id,
                category: "world".to_string(),
                title: "月蚀规则".to_string(),
                content: "月蚀夜所有传送阵失效。".to_string(),
                status: "approved".to_string(),
                source_artifact_id: None,
                source_chapter_id: None,
            })
            .unwrap();
        let thread = state
            .save_foreshadowing(SaveForeshadowing {
                id: None,
                project_id: project.id,
                title: "裂纹铜镜".to_string(),
                content: "主角在第一章看见铜镜裂纹。".to_string(),
                status: "active".to_string(),
                planted_chapter_id: Some(chapter.id),
                planned_payoff_chapter_id: Some(chapter.id),
                planned_payoff_note: "第一卷中段确认镜中人身份。".to_string(),
                source_artifact_id: None,
            })
            .unwrap();
        let detail = state.get_detail(project.id).unwrap();

        assert_ne!(
            state.get_project(project.id).unwrap().updated_at,
            project.updated_at
        );
        assert_eq!(
            state.get_story_bible(project.id).unwrap().unwrap().status,
            "needs_review"
        );
        assert_eq!(detail.knowledge_cards.len(), 1);
        assert_eq!(detail.knowledge_cards[0].id, card.id);
        assert_eq!(detail.knowledge_cards[0].status, "approved");
        assert_eq!(detail.foreshadowings.len(), 1);
        assert_eq!(detail.foreshadowings[0].id, thread.id);
        assert_eq!(
            detail.foreshadowings[0].planned_payoff_chapter_id,
            Some(chapter.id)
        );
    }

    #[test]
    fn migrates_legacy_xianxia_patch_into_generic_skills() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        state
            .save_writing_skill(SaveWritingSkill {
                id: None,
                skill_key: "xianxia_power_fantasy".to_string(),
                name: "男频修仙爽文".to_string(),
                category: "genre".to_string(),
                description: "用户版本".to_string(),
                content: "# Xianxia Power Fantasy Skill\n\n## Always\n- 用户自定义规则\n\n## App Library Patch: continuity-and-agency\n- 正文阶段：同一个悬念最多用两件证据确认。不要用黑牌、碎片、布片、刻字、梦境、旁人口供连续确认同一个结论；第三个证据必须换成新的行动压力或直接删除。\n".to_string(),
                enabled: true,
            })
            .unwrap();

        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let xianxia = state
            .get_writing_skill_by_key("xianxia_power_fantasy")
            .unwrap()
            .unwrap();
        let craft = state
            .get_writing_skill_by_key("continuity_and_agency")
            .unwrap()
            .unwrap();

        assert!(!xianxia
            .content
            .contains("App Library Patch: continuity-and-agency"));
        assert!(xianxia.content.contains("每章要有可感知的进度增量"));
        assert!(craft
            .content
            .contains("同一个悬念或结论，最多用两件证据确认"));
    }

    #[test]
    fn migrates_rigid_builtin_serialized_skill_to_flexible_contract() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        state
            .save_writing_skill(SaveWritingSkill {
                id: None,
                skill_key: "general_serialized".to_string(),
                name: "通用连载写作".to_string(),
                category: "genre".to_string(),
                description: "旧内置模板".to_string(),
                content: "# General Serialized Fiction Skill\n\n- 上一章抛出的最强钩子必须立即回应。\n- 章末必须留下更具体的下一步。\n- 大纲阶段：每章必须写清章节功能。\n"
                    .to_string(),
                enabled: true,
            })
            .unwrap();
        drop(state);

        let reopened = AppState::from_path(path).unwrap();
        let skill = reopened
            .get_writing_skill_by_key("general_serialized")
            .unwrap()
            .unwrap();

        assert!(skill.content.contains("进入状态、章节模式、目标"));
        assert!(skill.content.contains("不强制危险、反转或悬念句"));
        assert!(!skill.content.contains("章末必须留下更具体的下一步"));
    }

    #[test]
    fn migrates_only_the_rigid_xianxia_ending_clause() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        state
            .save_writing_skill(SaveWritingSkill {
                id: None,
                skill_key: "xianxia_power_fantasy".to_string(),
                name: "男频修仙爽文".to_string(),
                category: "genre".to_string(),
                description: "用户扩展过的模板".to_string(),
                content: "# Xianxia Power Fantasy Skill\n\n- 用户自定义：保留慢修章。\n- 修订阶段：若章末钩子不足，优先补新的行动压力、外部变化、回收代价或更高层次的追查，不要只补一句“他决定继续变强”。\n"
                    .to_string(),
                enabled: true,
            })
            .unwrap();
        drop(state);

        let reopened = AppState::from_path(path).unwrap();
        let skill = reopened
            .get_writing_skill_by_key("xianxia_power_fantasy")
            .unwrap()
            .unwrap();

        assert!(skill.content.contains("用户自定义：保留慢修章"));
        assert!(skill.content.contains("若结尾功能不足"));
        assert!(skill.content.contains("不凭空补新危险"));
        assert!(!skill.content.contains("若章末钩子不足"));
    }

    #[test]
    fn startup_does_not_rewrite_custom_agent_prompt_fragments() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE agents SET system_prompt = ?1 WHERE stage = 'draft'",
                    params!["用户保留段落。开篇不要空转，应尽早给出异常、威胁、任务、瓶颈或正在发生的行动；结尾应落在具体新信息、变化、威胁或选择上，不能只写主角准备去做什么。"],
                )?;
                Ok(())
            })
            .unwrap();
        drop(state);

        let reopened = AppState::from_path(path).unwrap();
        let draft = reopened
            .list_agents()
            .unwrap()
            .into_iter()
            .find(|agent| agent.stage == "draft")
            .unwrap();

        assert_eq!(
            draft.system_prompt,
            "用户保留段落。开篇不要空转，应尽早给出异常、威胁、任务、瓶颈或正在发生的行动；结尾应落在具体新信息、变化、威胁或选择上，不能只写主角准备去做什么。"
        );
    }

    #[test]
    fn creates_project_with_first_chapter() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "测试小说".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "主角在废墟中重建文明".to_string(),
            })
            .unwrap();
        let detail = state.get_detail(project.id).unwrap();

        assert_eq!(detail.project.title, "测试小说");
        assert_eq!(detail.chapters.len(), 1);
    }

    #[test]
    fn creates_next_chapter_with_generated_title_when_title_is_empty() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "自动章节标题".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();

        let chapter = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("   ".to_string()),
            })
            .unwrap();

        assert_eq!(chapter.chapter_no, 2);
        assert_eq!(chapter.title, "第 2 章");
    }

    #[test]
    fn updates_streaming_workflow_run_in_place() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "流式运行".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let run = state
            .insert_workflow_run(
                project.id,
                None,
                "setting",
                "输入",
                "",
                "streaming",
                None,
                0,
            )
            .unwrap();
        let updated = state
            .update_workflow_run(run.id, "实时输出", "success", None, 123)
            .unwrap();

        assert_eq!(updated.id, run.id);
        assert_eq!(updated.status, "success");
        assert_eq!(updated.output, "实时输出");
        assert_eq!(updated.elapsed_ms, 123);
    }

    #[test]
    fn marks_streaming_runs_failed_when_database_reopens() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "恢复遗留运行".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        state
            .insert_workflow_run(
                project.id,
                None,
                "revision",
                "输入",
                "部分输出",
                "streaming",
                None,
                0,
            )
            .unwrap();
        drop(state);

        let reopened = AppState::from_path(path).unwrap();
        let runs = reopened.list_workflow_runs(project.id).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "failed");
        assert!(runs[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("应用重启前未完成"));
        assert_eq!(runs[0].output, "部分输出");
    }

    #[test]
    fn deletes_chapter_content_and_renumbers_following_chapters() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "删除章节".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let first = state.list_chapters(project.id).unwrap().remove(0);
        let second = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第二章".to_string()),
            })
            .unwrap();
        let third = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第三章".to_string()),
            })
            .unwrap();
        let artifact = state
            .insert_artifact(
                project.id,
                Some(second.id),
                "draft",
                "正文",
                "第二章正文",
                None,
            )
            .unwrap();

        state.delete_chapter(project.id, second.id).unwrap();

        let chapters = state.list_chapters(project.id).unwrap();
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].id, first.id);
        assert_eq!(chapters[0].chapter_no, 1);
        assert_eq!(chapters[1].id, third.id);
        assert_eq!(chapters[1].chapter_no, 2);
        assert!(state.get_artifact(artifact.id).is_err());
    }

    #[test]
    fn refuses_to_delete_current_chapter_body() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "删除保护".to_string(),
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
                "正文内容",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", artifact.id, "通过")
            .unwrap();

        let err = state
            .delete_artifact(project.id, artifact.id)
            .unwrap_err()
            .to_string();
        assert!(err.contains("当前正式正文"));
    }

    #[test]
    fn clears_chapter_history_but_keeps_current_body() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "清历史".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let current = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "正文",
                "当前正文",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", current.id, "通过")
            .unwrap();
        let old_review = state
            .insert_artifact(project.id, Some(chapter.id), "review", "试读", "[]", None)
            .unwrap();
        let old_revision = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "revision",
                "修订",
                "历史修订稿",
                Some(current.id),
            )
            .unwrap();

        let result = state
            .clear_chapter_history(project.id, chapter.id, &[old_review.id])
            .unwrap();

        assert!(result.deleted_artifact_ids.contains(&old_revision.id));
        assert!(result.kept_artifact_ids.contains(&current.id));
        assert!(result.kept_artifact_ids.contains(&old_review.id));
        assert!(state.get_artifact(current.id).is_ok());
        assert!(state.get_artifact(old_review.id).is_ok());
        assert!(state.get_artifact(old_revision.id).is_err());
    }

    #[test]
    fn chapter_memory_cas_rejects_a_replaced_official_body() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "交接记忆 CAS".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let first = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "正文 v1",
                "陆烬收起黑牌。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", first.id, "通过")
            .unwrap();
        let first_hash = crate::chapter_memory::source_text_hash(&first.content);
        state
            .upsert_chapter_memory_cas(
                project.id,
                chapter.id,
                first.id,
                &first_hash,
                crate::chapter_memory::NORMALIZATION_VERSION,
                "{}",
            )
            .unwrap();

        let second = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "revision",
                "正文 v2",
                "陆烬把黑牌交给苏清寒。",
                Some(first.id),
            )
            .unwrap();
        state
            .approve_stage(project.id, "revision", second.id, "通过")
            .unwrap();

        let error = state
            .upsert_chapter_memory_cas(
                project.id,
                chapter.id,
                first.id,
                &first_hash,
                crate::chapter_memory::NORMALIZATION_VERSION,
                "{}",
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("正式正文已切换"));
    }

    #[test]
    fn replacing_official_body_invalidates_derived_continuity_ledger() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "账本失效".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let first = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "正文 v1",
                "陆烬将黑牌投入火中，黑牌已经耗尽灵光。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", first.id, "通过")
            .unwrap();
        let first_hash = crate::chapter_memory::source_text_hash(&first.content);
        state
            .replace_continuity_ledger_chapter_cas(
                project.id,
                chapter.id,
                first.id,
                &first_hash,
                crate::continuity_ledger::NORMALIZATION_VERSION,
                &[ContinuityLedgerEntry {
                    id: 0,
                    project_id: project.id,
                    chapter_id: chapter.id,
                    source_artifact_id: first.id,
                    source_text_hash: first_hash.clone(),
                    normalization_version: crate::continuity_ledger::NORMALIZATION_VERSION
                        .to_string(),
                    entity_kind: "item".to_string(),
                    entity_key: "黑牌".to_string(),
                    entity_label: "黑牌".to_string(),
                    state_kind: "availability".to_string(),
                    state_value: "depleted".to_string(),
                    evidence_quote: "陆烬将黑牌投入火中，黑牌已经耗尽灵光。".to_string(),
                    created_at: String::new(),
                }],
            )
            .unwrap();
        assert_eq!(
            state
                .list_continuity_ledger_entries(project.id)
                .unwrap()
                .len(),
            1
        );

        let replacement = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "revision",
                "正文 v2",
                "陆烬将黑牌收入袖中。",
                Some(first.id),
            )
            .unwrap();
        state
            .approve_stage(project.id, "revision", replacement.id, "替换正文")
            .unwrap();
        assert!(state
            .list_continuity_ledger_entries(project.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn replacing_official_body_invalidates_story_index_and_prunes_orphan_entities() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "资料索引失效".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let first = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "正文 v1",
                "陆烬将裂纹黑牌收入怀中。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", first.id, "通过")
            .unwrap();

        let source_hash = crate::chapter_memory::source_text_hash(&first.content);
        let indexed = crate::story_index::IndexedChapter {
            entities: vec![crate::story_index::IndexedEntity {
                kind: "character".to_string(),
                name: "陆烬".to_string(),
                evidence_quote: "陆烬将裂纹黑牌收入怀中。".to_string(),
            }],
            events: vec![crate::story_index::IndexedEvent {
                title: "收起黑牌".to_string(),
                kind: "discovery".to_string(),
                status: "occurred".to_string(),
                story_time: "当夜".to_string(),
                summary: "陆烬收起裂纹黑牌。".to_string(),
                evidence_quote: "陆烬将裂纹黑牌收入怀中。".to_string(),
                participants: vec![crate::story_index::IndexedParticipant {
                    kind: "character".to_string(),
                    name: "陆烬".to_string(),
                    role: "行动者".to_string(),
                }],
            }],
            facts: vec![crate::story_index::IndexedFact {
                entity_kind: "character".to_string(),
                entity_name: "陆烬".to_string(),
                event_title: Some("收起黑牌".to_string()),
                dimension: "possession".to_string(),
                value: "将裂纹黑牌收入怀中".to_string(),
                visibility: "world".to_string(),
                evidence_quote: "陆烬将裂纹黑牌收入怀中。".to_string(),
            }],
        };
        state
            .replace_story_index_chapter_cas(
                project.id,
                chapter.id,
                first.id,
                &source_hash,
                crate::story_index::NORMALIZATION_VERSION,
                &indexed,
            )
            .unwrap();
        let indexed_detail = state.get_detail(project.id).unwrap();
        assert_eq!(indexed_detail.story_index_sources.len(), 1);
        assert_eq!(indexed_detail.story_entities.len(), 1);
        assert_eq!(indexed_detail.story_events.len(), 1);
        assert_eq!(indexed_detail.story_facts.len(), 1);

        let second = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "revision",
                "正文 v2",
                "陆烬把裂纹黑牌留在石台上。",
                Some(first.id),
            )
            .unwrap();
        state
            .approve_stage(project.id, "revision", second.id, "替换正式稿")
            .unwrap();

        let detail = state.get_detail(project.id).unwrap();
        assert!(detail.story_index_sources.is_empty());
        assert!(detail.story_entities.is_empty());
        assert!(detail.story_events.is_empty());
        assert!(detail.story_facts.is_empty());
    }

    #[test]
    fn approval_enqueues_deduplicated_jobs_and_chapter_update_keeps_pointer() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "审批任务队列".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let first = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "正文 v1",
                "第一版正文",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", first.id, "通过")
            .unwrap();

        let jobs = state.list_derived_index_jobs(project.id).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().all(|job| job.status == "pending"));
        assert!(jobs
            .iter()
            .all(|job| job.source_artifact_id == Some(first.id)));

        state
            .approve_stage(project.id, "draft", first.id, "重复通过")
            .unwrap();
        assert_eq!(state.list_derived_index_jobs(project.id).unwrap().len(), 2);

        let second = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "revision",
                "正文 v2",
                "第二版正文",
                Some(first.id),
            )
            .unwrap();
        state
            .approve_stage(project.id, "revision", second.id, "替换正文")
            .unwrap();
        let jobs = state.list_derived_index_jobs(project.id).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs
            .iter()
            .all(|job| job.source_artifact_id == Some(second.id)));

        let updated = state
            .update_chapter(ChapterUpdate {
                project_id: project.id,
                id: chapter.id,
                title: "改名后的章节".to_string(),
                status: "approved".to_string(),
            })
            .unwrap();
        assert_eq!(updated.current_artifact_id, Some(second.id));
    }

    #[test]
    fn running_index_jobs_are_recovered_on_database_reopen() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "恢复索引任务".to_string(),
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
                "正文内容",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", artifact.id, "通过")
            .unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE derived_index_jobs SET status = 'running', started_at = 'old' WHERE project_id = ?1",
                    params![project.id],
                )?;
                Ok(())
            })
            .unwrap();
        drop(state);

        let reopened = AppState::from_path(path).unwrap();
        let jobs = reopened.list_derived_index_jobs(project.id).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().all(|job| job.status == "pending"));
        assert!(jobs.iter().all(|job| job.started_at.is_none()));
    }

    #[test]
    fn artifact_and_approval_contracts_reject_cross_project_references() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let first_project = state
            .create_project(NewProject {
                title: "项目一".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let second_project = state
            .create_project(NewProject {
                title: "项目二".to_string(),
                genre: "悬疑".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let first_chapter = state.list_chapters(first_project.id).unwrap().remove(0);
        let second_chapter = state.list_chapters(second_project.id).unwrap().remove(0);
        let parent = state
            .insert_artifact(
                second_project.id,
                None,
                "setting",
                "项目二设定",
                "设定",
                None,
            )
            .unwrap();

        let cross_chapter = state
            .insert_artifact(
                first_project.id,
                Some(second_chapter.id),
                "draft",
                "非法正文",
                "正文",
                None,
            )
            .unwrap_err()
            .to_string();
        assert!(cross_chapter.contains("章节") && cross_chapter.contains("不存在"));

        let cross_parent = state
            .insert_artifact(
                first_project.id,
                Some(first_chapter.id),
                "draft",
                "非法父产物",
                "正文",
                Some(parent.id),
            )
            .unwrap_err()
            .to_string();
        assert!(cross_parent.contains("父产物不属于当前项目"));
    }

    #[test]
    fn deleting_project_clears_search_documents_fts_vectors_and_jobs() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "删除索引项目".to_string(),
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
                "孤立搜索内容",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", artifact.id, "通过")
            .unwrap();
        state
            .with_conn(|conn| {
                let embedding_sql = conn
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE name = 'story_search_embeddings'",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                if embedding_sql.is_none() {
                    conn.execute_batch("CREATE TABLE story_search_embeddings (embedding TEXT)")?;
                }
                conn.execute(
                    "INSERT INTO story_search_documents
                        (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, title,
                         content, search_text, chunk_no, chunk_start, chunk_end,
                         visibility_cutoff_chapter_no, source_text_hash, normalization_version, updated_at)
                     VALUES (?1, 'chapter', ?2, ?2, 1, 'draft', '正文', '孤立搜索内容',
                             '孤立搜索内容', 0, 0, 6, 1, 'hash', 'test', 'now')",
                    params![project.id, chapter.id],
                )?;
                let document_id = conn.last_insert_rowid();
                if embedding_sql
                    .as_deref()
                    .is_some_and(|sql| sql.to_ascii_lowercase().contains("vec0"))
                {
                    let vector = serde_json::to_string(&vec![0.0_f32; 512])?;
                    conn.execute(
                        "INSERT INTO story_search_embeddings(rowid, embedding) VALUES (?1, ?2)",
                        params![document_id, vector],
                    )?;
                } else {
                    conn.execute(
                        "INSERT INTO story_search_embeddings(rowid, embedding) VALUES (?1, 'vector')",
                        params![document_id],
                    )?;
                }
                conn.execute(
                    "INSERT INTO story_search_sources
                        (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage,
                         source_artifact_id, source_text_hash, normalization_version, status, error, indexed_at)
                     VALUES (?1, 'chapter', ?2, ?2, 1, 'draft', ?3, 'hash', 'test', 'success', NULL, 'now')",
                    params![project.id, chapter.id, artifact.id],
                )?;
                Ok(())
            })
            .unwrap();

        state.delete_project(project.id).unwrap();
        let counts = state
            .with_conn(|conn| {
                let documents: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM story_search_documents WHERE project_id = ?1",
                    params![project.id],
                    |row| row.get(0),
                )?;
                let sources: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM story_search_sources WHERE project_id = ?1",
                    params![project.id],
                    |row| row.get(0),
                )?;
                let vectors: i64 =
                    conn.query_row("SELECT COUNT(*) FROM story_search_embeddings", [], |row| {
                        row.get(0)
                    })?;
                let jobs: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM derived_index_jobs WHERE project_id = ?1",
                    params![project.id],
                    |row| row.get(0),
                )?;
                let fts: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM story_search_documents_fts
                     WHERE story_search_documents_fts MATCH '孤立搜索内容'",
                    [],
                    |row| row.get(0),
                )?;
                Ok((documents, sources, vectors, jobs, fts))
            })
            .unwrap();
        assert_eq!(counts, (0, 0, 0, 0, 0));
        assert!(state.get_project(project.id).is_err());
    }

    #[test]
    fn derived_schema_migration_removes_orphans_preserves_ids_and_adds_foreign_keys() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "迁移派生数据".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        drop(state);

        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS story_search_documents_ai;
             DROP TRIGGER IF EXISTS story_search_documents_ad;
             DROP TRIGGER IF EXISTS story_search_documents_au;
             DROP TABLE IF EXISTS story_search_documents_fts;
             DROP TABLE story_search_documents;
             DROP TABLE story_search_sources;
             CREATE TABLE story_search_documents (
                 id INTEGER PRIMARY KEY,
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
                 updated_at TEXT NOT NULL
             );
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
                 PRIMARY KEY(project_id, source_kind, source_id)
             );
             CREATE TABLE story_search_embeddings (embedding TEXT);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO story_search_documents
                (id, project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, title,
                 content, search_text, chunk_no, chunk_start, chunk_end, visibility_cutoff_chapter_no,
                 source_text_hash, normalization_version, updated_at)
             VALUES (101, ?1, 'chapter', 101, ?2, 1, 'draft', '有效正文', '有效正文',
                     '有效正文', 0, 0, 4, 1, 'hash', 'test', 'now')",
            params![project.id, chapter.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO story_search_documents
                (id, project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, title,
                 content, search_text, chunk_no, chunk_start, chunk_end, visibility_cutoff_chapter_no,
                 source_text_hash, normalization_version, updated_at)
             VALUES (102, 999999, 'chapter', 102, NULL, 2, 'draft', '孤立正文', '孤立正文',
                     '孤立正文', 0, 0, 4, 2, 'hash', 'test', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO story_search_sources
                (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, source_artifact_id,
                 source_text_hash, normalization_version, status, error, indexed_at)
             VALUES (?1, 'chapter', 101, ?2, 1, 'draft', NULL, 'hash', 'test', 'success', NULL, 'now')",
            params![project.id, chapter.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO story_search_sources
                (project_id, source_kind, source_id, chapter_id, chapter_no_sort, stage, source_artifact_id,
                 source_text_hash, normalization_version, status, error, indexed_at)
             VALUES (999999, 'chapter', 102, NULL, 2, 'draft', NULL, 'hash', 'test', 'success', NULL, 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO story_search_embeddings(rowid, embedding) VALUES (101, 'valid')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO story_search_embeddings(rowid, embedding) VALUES (102, 'orphan document')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        drop(conn);

        let migrated = AppState::from_path(path).unwrap();
        let (documents, sources, vectors, version, document_fks, source_fks) = migrated
            .with_conn(|conn| {
                let documents: i64 =
                    conn.query_row("SELECT COUNT(*) FROM story_search_documents", [], |row| {
                        row.get(0)
                    })?;
                let sources: i64 =
                    conn.query_row("SELECT COUNT(*) FROM story_search_sources", [], |row| {
                        row.get(0)
                    })?;
                let vectors: i64 =
                    conn.query_row("SELECT COUNT(*) FROM story_search_embeddings", [], |row| {
                        row.get(0)
                    })?;
                let version: i64 =
                    conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
                let document_fks: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM pragma_foreign_key_list('story_search_documents')",
                    [],
                    |row| row.get(0),
                )?;
                let source_fks: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM pragma_foreign_key_list('story_search_sources')",
                    [],
                    |row| row.get(0),
                )?;
                Ok((
                    documents,
                    sources,
                    vectors,
                    version,
                    document_fks,
                    source_fks,
                ))
            })
            .unwrap();
        assert_eq!((documents, sources, vectors, version), (1, 1, 1, 4));
        assert_eq!(document_fks, 2);
        assert_eq!(source_fks, 3);
        let migrated_source_status: String = migrated
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT status FROM story_search_sources WHERE project_id = ?1",
                    params![project.id],
                    |row| row.get(0),
                )
                .map_err(AppError::from)
            })
            .unwrap();
        assert_eq!(migrated_source_status, "fts_only");
        let preserved_id: i64 = migrated
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT id FROM story_search_documents WHERE project_id = ?1",
                    params![project.id],
                    |row| row.get(0),
                )
                .map_err(AppError::from)
            })
            .unwrap();
        assert_eq!(preserved_id, 101);
    }

    #[test]
    fn concurrent_artifact_inserts_get_unique_contiguous_versions() {
        use std::sync::Arc;

        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "并发版本".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let other_connection = AppState::from_path(path).unwrap();
        let states = [Arc::new(state), Arc::new(other_connection)];
        let mut handles = Vec::new();
        for index in 0..8 {
            let state = states[index % states.len()].clone();
            handles.push(std::thread::spawn(move || {
                state.insert_artifact(
                    project.id,
                    Some(chapter.id),
                    "draft",
                    &format!("并发版本 {index}"),
                    "正文",
                    None,
                )
            }));
        }
        let artifacts = handles
            .into_iter()
            .map(|handle| handle.join().unwrap().unwrap())
            .collect::<Vec<_>>();
        let mut versions = artifacts
            .into_iter()
            .map(|artifact| artifact.version)
            .collect::<Vec<_>>();
        versions.sort_unstable();
        assert_eq!(versions, (1..=8).collect::<Vec<_>>());
    }
}
