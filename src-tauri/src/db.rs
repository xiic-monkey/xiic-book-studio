use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use tauri::{AppHandle, Manager};

use crate::{
    error::{AppError, AppResult},
    genre_skill,
    models::{
        Agent, AiSettings, Approval, Artifact, ArtifactFilters, Chapter, ChapterUpdate,
        Foreshadowing, KnowledgeCard, Message, NewChapter, NewProject, Project, ProjectDetail,
        ProjectUpdate, SaveAiSettings, SaveForeshadowing, SaveKnowledgeCard, SaveWritingSkill,
        StoryThread, WorkflowRun, WritingSkill,
    },
    secrets, workflow,
};

#[derive(Clone)]
pub struct AppState {
    conn: Arc<Mutex<Connection>>,
}

impl AppState {
    pub fn new(app: &AppHandle) -> AppResult<Self> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|err| AppError::Validation(format!("cannot resolve app data dir: {err}")))?;
        fs::create_dir_all(&data_dir)?;
        Self::from_path(data_dir.join("book-studio.sqlite3"))
    }

    pub fn from_path(path: PathBuf) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let state = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        state.migrate()?;
        state.recover_stale_workflow_runs()?;
        workflow::rebuild_story_threads(&state)?;
        Ok(state)
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Validation("database connection lock poisoned".to_string()))?;
        f(&conn)
    }

    fn migrate(&self) -> AppResult<()> {
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
                    temperature REAL NOT NULL DEFAULT 0.75
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
                "#,
            )?;

            for (stage, name, role, prompt, temperature) in default_agents() {
                conn.execute(
                    "INSERT INTO agents (stage, name, role, system_prompt, temperature)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(stage) DO UPDATE SET
                        name = excluded.name,
                        role = excluded.role,
                        system_prompt = excluded.system_prompt,
                        temperature = excluded.temperature",
                    params![stage, name, role, prompt, temperature],
                )?;
            }

            set_default_setting(conn, "ai.base_url", "https://api.deepseek.com")?;
            set_default_setting(conn, "ai.model", "deepseek-v4-pro")?;
            set_default_setting(conn, "ai.temperature", "0.75")?;
            set_default_setting(conn, "ai.thinking_enabled", "false")?;

            let now = now();
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

    fn recover_stale_workflow_runs(&self) -> AppResult<()> {
        self.with_conn(|conn| {
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

    pub fn create_project(&self, input: NewProject) -> AppResult<Project> {
        if input.title.trim().is_empty() {
            return Err(AppError::Validation("项目标题不能为空".to_string()));
        }
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO projects (title, genre, target_words, premise, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)",
                params![
                    input.title.trim(),
                    input.genre.trim(),
                    input.target_words.max(1),
                    input.premise.trim(),
                    now
                ],
            )?;
            let project_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO chapters (project_id, chapter_no, title, status, created_at, updated_at)
                 VALUES (?1, 1, '第 1 章', 'planning', ?2, ?2)",
                params![project_id, now],
            )?;
            query_project_by_id(conn, project_id)
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
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE projects
                 SET title = ?1, genre = ?2, target_words = ?3, premise = ?4, status = ?5, updated_at = ?6
                 WHERE id = ?7",
                params![
                    input.title.trim(),
                    input.genre.trim(),
                    input.target_words.max(1),
                    input.premise.trim(),
                    input.status,
                    now,
                    input.id
                ],
            )?;
            query_project_by_id(conn, input.id)
        })
    }

    pub fn delete_project(&self, id: i64) -> AppResult<()> {
        self.with_conn(|conn| {
            let deleted = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
            if deleted == 0 {
                return Err(AppError::Validation("项目不存在".to_string()));
            }
            Ok(())
        })
    }

    pub fn get_detail(&self, project_id: i64) -> AppResult<ProjectDetail> {
        Ok(ProjectDetail {
            project: self.get_project(project_id)?,
            chapters: self.list_chapters(project_id)?,
            agents: self.list_agents()?,
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
            settings: self.get_ai_settings()?,
        })
    }

    pub fn list_agents(&self) -> AppResult<Vec<Agent>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, stage, name, role, system_prompt, temperature FROM agents ORDER BY id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    stage: row.get(1)?,
                    name: row.get(2)?,
                    role: row.get(3)?,
                    system_prompt: row.get(4)?,
                    temperature: row.get(5)?,
                })
            })?;
            collect_rows(rows)
        })
    }

    pub fn get_agent(&self, stage: &str) -> AppResult<Agent> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, stage, name, role, system_prompt, temperature FROM agents WHERE stage = ?1",
                params![stage],
                |row| {
                    Ok(Agent {
                        id: row.get(0)?,
                        stage: row.get(1)?,
                        name: row.get(2)?,
                        role: row.get(3)?,
                        system_prompt: row.get(4)?,
                        temperature: row.get(5)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("Agent 不存在".to_string()))
        })
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
        let now = now();
        self.with_conn(|conn| {
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
                params![input.project_id, next_no, title, now],
            )?;
            let id = conn.last_insert_rowid();
            conn.query_row(
                "SELECT id, project_id, chapter_no, title, status, current_artifact_id, created_at, updated_at
                 FROM chapters WHERE id = ?1",
                params![id],
                map_chapter,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("章节创建失败".to_string()))
        })
    }

    pub fn delete_chapter(&self, project_id: i64, chapter_id: i64) -> AppResult<()> {
        if project_id <= 0 || chapter_id <= 0 {
            return Err(AppError::Validation("项目或章节不存在".to_string()));
        }

        self.with_conn(|conn| {
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

                conn.execute(
                    "DELETE FROM chapters WHERE id = ?1 AND project_id = ?2",
                    params![chapter_id, project_id],
                )?;
                conn.execute(
                    "UPDATE chapters
                     SET chapter_no = chapter_no - 1, updated_at = ?1
                     WHERE project_id = ?2 AND chapter_no > ?3",
                    params![now(), project_id, chapter_no],
                )?;
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
        if input.title.trim().is_empty() {
            return Err(AppError::Validation("章节标题不能为空".to_string()));
        }
        let now = now();
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE chapters
                 SET title = ?1, status = ?2, current_artifact_id = ?3, updated_at = ?4
                 WHERE id = ?5",
                params![input.title.trim(), input.status, input.current_artifact_id, now, input.id],
            )?;
            conn.query_row(
                "SELECT id, project_id, chapter_no, title, status, current_artifact_id, created_at, updated_at
                 FROM chapters WHERE id = ?1",
                params![input.id],
                map_chapter,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("章节不存在".to_string()))
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

    pub fn insert_artifact(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
        stage: &str,
        title: &str,
        content: &str,
        parent_artifact_id: Option<i64>,
    ) -> AppResult<Artifact> {
        let next_version = self.next_version(project_id, chapter_id, stage)?;
        let now = now();
        self.with_conn(|conn| {
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
                    now
                ],
            )?;
            let id = conn.last_insert_rowid();
            query_artifact_by_id(conn, id)
        })
    }

    pub fn delete_artifact(&self, project_id: i64, artifact_id: i64) -> AppResult<()> {
        let artifact = self.get_artifact(artifact_id)?;
        if artifact.project_id != project_id {
            return Err(AppError::Validation("产物不属于当前项目".to_string()));
        }
        if let Some(reason) = self.protected_artifact_reason(&artifact)? {
            return Err(AppError::Validation(reason));
        }
        self.with_conn(|conn| {
            conn.execute("DELETE FROM artifacts WHERE id = ?1", params![artifact_id])?;
            Ok(())
        })?;
        self.insert_message(
            project_id,
            artifact.chapter_id,
            "human_instruction",
            &format!(
                "删除历史版本：{} v{}",
                stage_label(&artifact.stage),
                artifact.version
            ),
        )?;
        Ok(())
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
        let mut keep_set: HashSet<i64> = keep_artifact_ids.iter().copied().collect();
        if let Some(current_id) = chapter.current_artifact_id {
            keep_set.insert(current_id);
        }
        let artifacts = self.list_artifacts(ArtifactFilters {
            project_id,
            stage: None,
            chapter_id: Some(chapter_id),
        })?;
        let mut deleted = Vec::new();
        for artifact in artifacts {
            if keep_set.contains(&artifact.id) {
                continue;
            }
            if self.protected_artifact_reason(&artifact)?.is_some() {
                keep_set.insert(artifact.id);
                continue;
            }
            self.with_conn(|conn| {
                conn.execute("DELETE FROM artifacts WHERE id = ?1", params![artifact.id])?;
                Ok(())
            })?;
            deleted.push(artifact.id);
        }
        self.insert_message(
            project_id,
            Some(chapter_id),
            "human_instruction",
            &format!(
                "清理章节历史版本：删除 {} 个，保留 {} 个。",
                deleted.len(),
                keep_set.len()
            ),
        )?;
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

    fn next_version(
        &self,
        project_id: i64,
        chapter_id: Option<i64>,
        stage: &str,
    ) -> AppResult<i64> {
        self.with_conn(|conn| {
            let version: Option<i64> = if let Some(chapter_id) = chapter_id {
                conn.query_row(
                    "SELECT MAX(version) FROM artifacts WHERE project_id = ?1 AND chapter_id = ?2 AND stage = ?3",
                    params![project_id, chapter_id, stage],
                    |row| row.get(0),
                )?
            } else {
                conn.query_row(
                    "SELECT MAX(version) FROM artifacts WHERE project_id = ?1 AND chapter_id IS NULL AND stage = ?2",
                    params![project_id, stage],
                    |row| row.get(0),
                )?
            };
            Ok(version.unwrap_or(0) + 1)
        })
    }

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
        let category = input.category.trim();
        let title = input.title.trim();
        let content = input.content.trim();
        let status = normalize_library_status(&input.status)?;
        if category.is_empty() || title.is_empty() || content.is_empty() {
            return Err(AppError::Validation(
                "资料分类、标题和内容不能为空".to_string(),
            ));
        }
        self.get_project(input.project_id)?;
        let now = now();
        self.with_conn(|conn| {
            let id = if let Some(id) = input.id {
                let updated = conn.execute(
                    "UPDATE knowledge_cards
                     SET category = ?1, title = ?2, content = ?3, status = ?4, source_artifact_id = ?5,
                         source_chapter_id = ?6, updated_at = ?7
                     WHERE id = ?8 AND project_id = ?9",
                    params![category, title, content, status, input.source_artifact_id, input.source_chapter_id, now, id, input.project_id],
                )?;
                if updated == 0 {
                    return Err(AppError::Validation("资料卡不存在".to_string()));
                }
                id
            } else {
                conn.execute(
                    "INSERT INTO knowledge_cards (project_id, category, title, content, status, source_artifact_id, source_chapter_id, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    params![input.project_id, category, title, content, status, input.source_artifact_id, input.source_chapter_id, now],
                )?;
                conn.last_insert_rowid()
            };
            conn.query_row(
                "SELECT id, project_id, category, title, content, status, source_artifact_id, source_chapter_id, created_at, updated_at
                 FROM knowledge_cards WHERE id = ?1",
                params![id],
                map_knowledge_card,
            )
            .map_err(AppError::from)
        })
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
        let title = input.title.trim();
        let content = input.content.trim();
        let status = normalize_foreshadowing_status(&input.status)?;
        if title.is_empty() || content.is_empty() {
            return Err(AppError::Validation("伏笔标题和内容不能为空".to_string()));
        }
        self.get_project(input.project_id)?;
        let now = now();
        self.with_conn(|conn| {
            let id = if let Some(id) = input.id {
                let updated = conn.execute(
                    "UPDATE foreshadowings
                     SET title = ?1, content = ?2, status = ?3, planted_chapter_id = ?4,
                         planned_payoff_chapter_id = ?5, planned_payoff_note = ?6, source_artifact_id = ?7, updated_at = ?8
                     WHERE id = ?9 AND project_id = ?10",
                    params![title, content, status, input.planted_chapter_id, input.planned_payoff_chapter_id, input.planned_payoff_note.trim(), input.source_artifact_id, now, id, input.project_id],
                )?;
                if updated == 0 {
                    return Err(AppError::Validation("伏笔不存在".to_string()));
                }
                id
            } else {
                conn.execute(
                    "INSERT INTO foreshadowings (project_id, title, content, status, planted_chapter_id, planned_payoff_chapter_id, planned_payoff_note, source_artifact_id, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                    params![input.project_id, title, content, status, input.planted_chapter_id, input.planned_payoff_chapter_id, input.planned_payoff_note.trim(), input.source_artifact_id, now],
                )?;
                conn.last_insert_rowid()
            };
            conn.query_row(
                "SELECT id, project_id, title, content, status, planted_chapter_id, planned_payoff_chapter_id,
                        planned_payoff_note, source_artifact_id, created_at, updated_at
                 FROM foreshadowings WHERE id = ?1",
                params![id],
                map_foreshadowing,
            )
            .map_err(AppError::from)
        })
    }

    pub fn list_writing_skills(&self) -> AppResult<Vec<WritingSkill>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, skill_key, name, category, description, content, enabled, created_at, updated_at
                 FROM writing_skills ORDER BY category ASC, id ASC",
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
        })
    }

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
        let now = now();
        let approval = self.with_conn(|conn| {
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
                params![project_id, artifact.chapter_id, stage, artifact_id, note, now],
            )?;
            conn.execute(
                "UPDATE artifacts SET status = 'approved' WHERE id = ?1",
                params![artifact_id],
            )?;
            if stage == "draft" || stage == "revision" {
                if let Some(chapter_id) = artifact.chapter_id {
                    conn.execute(
                        "UPDATE chapters SET status = 'approved', current_artifact_id = ?1, updated_at = ?2 WHERE id = ?3",
                        params![artifact_id, now, chapter_id],
                    )?;
                }
            }
            conn.execute(
                "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
                params![now, project_id],
            )?;
            let id = conn.last_insert_rowid();
            let approval = conn.query_row(
                "SELECT id, project_id, chapter_id, stage, artifact_id, note, created_at
                 FROM approvals WHERE id = ?1",
                params![id],
                map_approval,
            )?;
            conn.execute(
                "INSERT INTO messages (project_id, chapter_id, role, content, created_at)
                 VALUES (?1, ?2, 'approval_note', ?3, ?4)",
                params![
                    project_id,
                    artifact.chapter_id,
                    format!("{} 已人工通过。{}", stage, note.trim()),
                    now
                ],
            )?;
            Ok(approval)
        })?;
        if stage == "draft" || stage == "revision" {
            workflow::sync_story_threads_from_artifact(self, &artifact)?;
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
        if input.base_url.trim().is_empty() {
            return Err(AppError::Validation("AI 地址不能为空".to_string()));
        }
        self.with_conn(|conn| {
            upsert_setting(conn, "ai.base_url", input.base_url.trim())?;
            upsert_setting(conn, "ai.model", input.model.trim())?;
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
            Ok(())
        })?;
        if let Some(api_key) = input.api_key {
            if !api_key.trim().is_empty() {
                secrets::set_api_key_for_scope(input.base_url.trim(), api_key.trim())?;
            }
        }
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
                has_api_key: false,
            })
        })?;
        settings.has_api_key = self.get_api_key_for_base_url(&settings.base_url)?.is_some();
        Ok(settings)
    }

    pub fn get_api_key(&self) -> AppResult<Option<String>> {
        secrets::get_api_key()
    }

    pub fn get_api_key_for_base_url(&self, base_url: &str) -> AppResult<Option<String>> {
        secrets::get_api_key_for_scope(base_url)
    }
}

fn default_agents() -> Vec<(&'static str, &'static str, &'static str, &'static str, f64)> {
    vec![
        (
            "setting",
            "设定 Agent",
            "负责小说核心设定",
            "你是类型小说开发编辑。目标不是写百科，而是给后续写作提供可执行约束。输出必须克制、具体、可引用：核心卖点、世界规则、叙事边界、读者爽点、禁忌清单。避免空泛形容和过长背景史。",
            0.62,
        ),
        (
            "outline",
            "大纲 Agent",
            "负责整书结构与章节路线",
            "你是商业连载主编。请把大纲写成可执行的章节生产计划：每章只保留目标、冲突、信息释放、结尾钩子。不要写散文式说明，不要扩写正文，不要过度解释设定。",
            0.64,
        ),
        (
            "characters",
            "角色 Agent",
            "负责角色卡与关系网",
            "你是小说角色编辑。请产出能直接指导正文的角色卡：欲望、恐惧、底线、说话方式、与主角的利益冲突、首次登场功能。减少履历堆砌，多给可复用的动作和对白特征。",
            0.66,
        ),
        (
            "draft",
            "写作 Agent",
            "负责章节正文草稿",
            "你是商业类型小说写手。只输出章节正文，不写创作说明。优先场景、动作、对白、细节和悬念推进；少解释设定，少内心总结，少使用泛化比喻。每段都要推动人物、线索或气氛。严格使用已批准设定、大纲和角色资料，不越过未批准事实。开篇不要空转，应尽早给出异常、威胁、任务、瓶颈或正在发生的行动；结尾应落在具体新信息、变化、威胁或选择上，不能只写主角准备去做什么。章节长度由场景、人物关系、情绪承接和冲突解决的实际需要决定，不设上限。每章只解决一个明确动作目标，最多回收1到2条旧线索，最多新增1个关键事实；若素材太多，宁可延后，不要塞爆单章。",
            0.78,
        ),
        (
            "review",
            "试读 Agent",
            "负责挑出问题",
            "你是苛刻的连载试读编辑。只挑会影响读者继续读的问题，从开篇抓力、节奏、场景可信度、对白质感、人物一致性、悬念强度、解释感和爽点兑现检查。重点检查开篇是否尽早进入人物正在面对的事情，以及结尾是否留下具体可追读的变化；如果结尾只有情绪、决定、出发、回头或沉默，不算有效钩子。审校与建议都只能使用候选稿、已批准资料和前章已有事实；候选稿凭空加入过去事件、隐藏证据、人物、地点、规则或角色已知信息时，必须标 major 的事实越界，不能用新设定替它修补。每条建议必须同时提供候选稿或已批准资料中的 evidence_quote 和 action_evidence_quote 原文；后者必须证明建议只是删减、重排、强化或继续使用已有动作。无法提供动作原文时，建议留空。不要因章节本身较长而扣分，只有重复、信息堆叠或失去章节功能时才指出篇幅问题。每条建议必须能直接指导修稿。",
            0.35,
        ),
        (
            "revision",
            "修订 Agent",
            "负责根据反馈改稿",
            "你是小说修订编辑。只输出修订后的完整正文。优先解决试读问题和人工反馈，保留有效段落，删掉解释感和模板化句子，增强开篇钩子、动作推进、对白潜台词和章末悬念。只允许使用源稿、已批准资料、试读报告和人工指令中已明确的事实；不得为制造钩子编造过去事件、隐藏证据、人物、地点、制度、交易习惯或角色已知信息。需要更强结尾时，推进已经出现的威胁、时限、资源、伤势或主角已开始的行动。不要为了压缩篇幅删掉必要场景、对白或情绪承接；只有重复或失焦时才主动减法。若原稿信息过载，删除次要机制解释、次要空间层级和次要反转，把章节收束成一个更清晰的主动作目标。",
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

fn normalize_library_status(status: &str) -> AppResult<&str> {
    match status.trim() {
        "pending_human_approval" | "approved" | "archived" => Ok(status.trim()),
        _ => Err(AppError::Validation("资料卡状态无效".to_string())),
    }
}

fn normalize_foreshadowing_status(status: &str) -> AppResult<&str> {
    match status.trim() {
        "pending_human_approval" | "active" | "ready_for_payoff" | "resolved" | "archived" => {
            Ok(status.trim())
        }
        _ => Err(AppError::Validation("伏笔状态无效".to_string())),
    }
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

    #[test]
    fn initializes_database_and_defaults() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let agents = state.list_agents().unwrap();
        let settings = state.get_ai_settings().unwrap();
        let skills = state.list_writing_skills().unwrap();

        assert_eq!(agents.len(), 6);
        assert_eq!(settings.base_url, "https://api.deepseek.com");
        assert_eq!(settings.model, "deepseek-v4-pro");
        assert!(skills
            .iter()
            .any(|skill| skill.skill_key == "xianxia_power_fantasy"));
        assert!(skills
            .iter()
            .any(|skill| skill.skill_key == "continuity_and_agency"));
    }

    #[test]
    fn saves_writing_skill_to_app_library() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let saved = state
            .save_writing_skill(SaveWritingSkill {
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
}
