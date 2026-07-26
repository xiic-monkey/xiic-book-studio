use std::{env, net::SocketAddr, path::PathBuf};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use crate::{
    adoption, ai, chapter_memory, continuity_ledger,
    db::AppState,
    error::{AppError, AppResult},
    gate,
    models::{
        AiSpanRevisionRequest, ChapterGateRequest, ChapterSplitPlanRequest,
        ClearChapterHistoryRequest, ConfirmStoryBibleRequest, ConfirmStoryBibleReviewRequest,
        ContinuityReviewRequest, DecideAdoptionProposalsRequest, DeleteArtifactRequest,
        LedgerContinuityCheckRequest, ListAdoptionProposalsRequest, ListModelsInput,
        PrepareArtifactAdoptionsRequest, RebuildChapterMemoryRequest, RebuildStoryIndexRequest,
        RebuildStorySearchIndexRequest, RetryIndexJobsRequest, RevisionRequest, RunAgentRequest,
        RunStoryArchitectRequest, SaveAiSettings, SaveForeshadowing, SaveKnowledgeCard,
        SaveWritingSkill, SpanReplacementRequest, StoryBibleReviewRequest, StoryContextSearchInput,
        TestAiConnectionInput, UpdateAdoptionProposalRequest,
    },
    quality, story_architecture, story_index, story_search, workflow,
};

#[derive(Clone)]
struct DevServerState {
    app: AppState,
    db_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct DevMeta {
    mode: &'static str,
    db_path: String,
}

#[derive(Debug, Serialize)]
struct CommandEnvelope<T>
where
    T: Serialize,
{
    ok: bool,
    data: T,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    ok: bool,
    error: String,
}

pub async fn run() -> AppResult<()> {
    let db_path = resolve_dev_db_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let app = AppState::from_path(db_path.clone())?;
    app.start_index_worker();
    let state = DevServerState { app, db_path };

    let port = env::var("BOOK_STUDIO_DEV_API_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(4141);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let router = Router::new()
        .route("/health", get(health))
        .route("/meta", get(meta))
        .route("/commands/{command}", post(run_command))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| AppError::Validation(format!("dev api bind failed: {err}")))?;
    println!("Book Studio dev API listening on http://{}", addr);
    axum::serve(listener, router)
        .await
        .map_err(|err| AppError::Validation(format!("dev api server failed: {err}")))?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn meta(State(state): State<DevServerState>) -> impl IntoResponse {
    Json(DevMeta {
        mode: "web-dev-api",
        db_path: state.db_path.display().to_string(),
    })
}

async fn run_command(
    Path(command): Path<String>,
    State(state): State<DevServerState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    match dispatch_command(&state.app, &command, payload).await {
        Ok(value) => (
            StatusCode::OK,
            Json(CommandEnvelope {
                ok: true,
                data: value,
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorEnvelope {
                ok: false,
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn dispatch_command(app: &AppState, command: &str, payload: Value) -> AppResult<Value> {
    match command {
        "list_projects" => Ok(serde_json::to_value(app.list_projects()?)?),
        "create_project" => Ok(serde_json::to_value(
            app.create_project(read_required(&payload, "input")?)?,
        )?),
        "update_project" => Ok(serde_json::to_value(
            app.update_project(read_required(&payload, "input")?)?,
        )?),
        "delete_project" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            app.delete_project(project_id)?;
            Ok(Value::Null)
        }
        "create_chapter" => Ok(serde_json::to_value(
            app.create_chapter(read_required(&payload, "input")?)?,
        )?),
        "delete_chapter" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let chapter_id = read_i64(&payload, &["chapterId", "chapter_id"])?;
            app.delete_chapter(project_id, chapter_id)?;
            Ok(Value::Null)
        }
        "update_chapter" => {
            let chapter = app.update_chapter(read_required(&payload, "input")?)?;
            story_search::refresh_chapter_metadata(app, chapter.project_id, chapter.id)?;
            Ok(serde_json::to_value(chapter)?)
        }
        "get_project" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(app.get_detail(project_id)?)?)
        }
        "get_settings" => Ok(serde_json::to_value(app.get_ai_settings()?)?),
        "save_ai_settings" => {
            let input: SaveAiSettings = read_required(&payload, "input")?;
            Ok(serde_json::to_value(app.save_ai_settings(input)?)?)
        }
        "list_writing_skills" => Ok(serde_json::to_value(app.list_writing_skills()?)?),
        "save_writing_skill" => {
            let input: SaveWritingSkill = read_required(&payload, "input")?;
            Ok(serde_json::to_value(app.save_writing_skill(input)?)?)
        }
        "save_knowledge_card" => {
            let input: SaveKnowledgeCard = read_required(&payload, "input")?;
            let card = app.save_knowledge_card(input)?;
            let _ = story_search::refresh_knowledge_card(app, card.project_id, card.id).await;
            Ok(serde_json::to_value(card)?)
        }
        "save_foreshadowing" => {
            let input: SaveForeshadowing = read_required(&payload, "input")?;
            let item = app.save_foreshadowing(input)?;
            let _ = story_search::refresh_foreshadowing(app, item.project_id, item.id).await;
            Ok(serde_json::to_value(item)?)
        }
        "prepare_artifact_adoptions" => {
            let input: PrepareArtifactAdoptionsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                adoption::prepare_artifact_adoptions(app, input.project_id, input.artifact_id)
                    .await?,
            )?)
        }
        "list_adoption_proposals" => {
            let input: ListAdoptionProposalsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(adoption::list_adoption_proposals(
                app,
                input.project_id,
                input.artifact_id,
            )?)?)
        }
        "update_adoption_proposal" => {
            let input: UpdateAdoptionProposalRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(adoption::update_adoption_proposal(
                app, input,
            )?)?)
        }
        "apply_adoption_proposals" => {
            let input: DecideAdoptionProposalsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(adoption::apply_adoption_proposals(
                app, input,
            )?)?)
        }
        "reject_adoption_proposals" => {
            let input: DecideAdoptionProposalsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(adoption::reject_adoption_proposals(
                app, input,
            )?)?)
        }
        "test_ai_connection" => {
            let input: Option<TestAiConnectionInput> = read_optional(&payload, "input")?;
            let result = test_ai_connection_impl(app, input).await?;
            Ok(Value::String(result))
        }
        "list_models" => {
            let input: Option<ListModelsInput> = read_optional(&payload, "input")?;
            Ok(serde_json::to_value(list_models_impl(app, input).await?)?)
        }
        "run_agent_step" => {
            let input: RunAgentRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                workflow::run_agent_step(app, input).await?,
            )?)
        }
        "rebuild_chapter_memory" => {
            let input: RebuildChapterMemoryRequest = read_required(&payload, "input")?;
            let settings = app.get_ai_settings()?;
            let api_key = app
                .get_api_key_for_base_url(&settings.base_url)?
                .ok_or_else(|| {
                    AppError::Validation("请先在设置里为当前供应商保存 AI API Key".to_string())
                })?;
            Ok(serde_json::to_value(
                chapter_memory::rebuild_chapter_memory(
                    app,
                    input.project_id,
                    input.chapter_id,
                    &settings,
                    &api_key,
                    None,
                )
                .await?,
            )?)
        }
        "run_story_architect" => {
            let input: RunStoryArchitectRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                story_architecture::run_story_architect(app, input).await?,
            )?)
        }
        "create_targeted_rework" => {
            let input: RunStoryArchitectRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                story_architecture::create_targeted_rework(app, input).await?,
            )?)
        }
        "confirm_story_bible" => {
            let input: ConfirmStoryBibleRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                story_architecture::confirm_story_bible(app, input)?,
            )?)
        }
        "review_story_bible" => {
            let input: StoryBibleReviewRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                story_architecture::review_story_bible(app, input).await?,
            )?)
        }
        "confirm_story_bible_review" => {
            let input: ConfirmStoryBibleReviewRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                story_architecture::confirm_story_bible_review(app, input)?,
            )?)
        }
        "list_story_arcs" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(app.list_story_arcs(project_id)?)?)
        }
        "preview_agent_context" => {
            let input: RunAgentRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(workflow::preview_agent_context(
                app, input,
            )?)?)
        }
        "approve_stage" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let stage = read_string(&payload, &["stage"])?;
            let artifact_id = read_i64(&payload, &["artifactId", "artifact_id"])?;
            let note = read_optional_string(&payload, &["note"])?;
            let approval = app.approve_stage(
                project_id,
                &stage,
                artifact_id,
                note.as_deref().unwrap_or(""),
            )?;
            app.wake_index_worker();
            Ok(serde_json::to_value(approval)?)
        }
        "retry_index_jobs" => {
            let input: RetryIndexJobsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(crate::index_jobs::retry_index_jobs(
                app, input,
            )?)?)
        }
        "rebuild_story_index" => {
            let input: RebuildStoryIndexRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                story_index::rebuild_story_index(app, input).await?,
            )?)
        }
        "rebuild_story_search_index" => {
            let input: RebuildStorySearchIndexRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                story_search::rebuild_story_search_index(app, input).await?,
            )?)
        }
        "get_story_search_status" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(
                story_search::get_story_search_status(app, project_id)?,
            )?)
        }
        "request_revision" => {
            let input: RevisionRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                workflow::request_revision(app, input).await?,
            )?)
        }
        "replace_artifact_span" => {
            let input: SpanReplacementRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(workflow::replace_artifact_span(
                app, input,
            )?)?)
        }
        "revise_artifact_span_with_ai" => {
            let input: AiSpanRevisionRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                workflow::revise_artifact_span_with_ai(app, input).await?,
            )?)
        }
        "delete_artifact" => {
            let input: DeleteArtifactRequest = read_required(&payload, "input")?;
            app.delete_artifact(input.project_id, input.artifact_id)?;
            Ok(Value::Null)
        }
        "clear_chapter_history" => {
            let input: ClearChapterHistoryRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(app.clear_chapter_history(
                input.project_id,
                input.chapter_id,
                input.keep_artifact_ids.as_deref().unwrap_or(&[]),
            )?)?)
        }
        "list_artifacts" => Ok(serde_json::to_value(
            app.list_artifacts(read_required(&payload, "filters")?)?,
        )?),
        "export_project" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let format = read_string(&payload, &["format"])?;
            match format.as_str() {
                "markdown" | "md" => Ok(Value::String(workflow::export_markdown(app, project_id)?)),
                _ => Err(AppError::Validation(
                    "第一版只支持 Markdown 导出".to_string(),
                )),
            }
        }
        "analyze_artifact_quality" => {
            let artifact_id = read_i64(&payload, &["artifactId", "artifact_id"])?;
            let artifact = app.get_artifact(artifact_id)?;
            Ok(serde_json::to_value(quality::analyze_artifact(&artifact))?)
        }
        "review_project_continuity" => {
            let input: ContinuityReviewRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                workflow::review_project_continuity(app, input).await?,
            )?)
        }
        "check_artifact_ledger_continuity" => {
            let input: LedgerContinuityCheckRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                continuity_ledger::check_artifact_continuity(app, input).await?,
            )?)
        }
        "analyze_chapter_gate" => {
            let input: ChapterGateRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gate::analyze_chapter_gate(app, input).await?,
            )?)
        }
        "generate_chapter_split_plan" => {
            let input: ChapterSplitPlanRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                workflow::generate_chapter_split_plan(app, input).await?,
            )?)
        }
        "search_story_context" => {
            let input: StoryContextSearchInput = read_required(&payload, "input")?;
            Ok(serde_json::to_value(workflow::search_story_context(
                app, input,
            )?)?)
        }
        _ => Err(AppError::Validation(format!(
            "unknown dev command: {command}"
        ))),
    }
}

async fn test_ai_connection_impl(
    app: &AppState,
    input: Option<TestAiConnectionInput>,
) -> AppResult<String> {
    let mut settings = app.get_ai_settings()?;
    let input = input.unwrap_or(TestAiConnectionInput {
        base_url: None,
        model: None,
        temperature: None,
        thinking_enabled: None,
        api_key: None,
    });

    if let Some(base_url) = input.base_url.filter(|value| !value.trim().is_empty()) {
        settings.base_url = base_url;
    }
    if let Some(model) = input.model.filter(|value| !value.trim().is_empty()) {
        settings.model = model;
    }
    if let Some(temperature) = input.temperature {
        settings.temperature = temperature;
    }
    if let Some(thinking_enabled) = input.thinking_enabled {
        settings.thinking_enabled = thinking_enabled;
    }

    if settings.model.trim().is_empty() {
        return Err(AppError::Validation(
            "请先填写模型名称，再测试连接".to_string(),
        ));
    }

    let api_key = input
        .api_key
        .filter(|value| !value.trim().is_empty())
        .or(app.get_api_key_for_base_url(&settings.base_url)?)
        .ok_or_else(|| AppError::Validation("请先为当前供应商保存 API Key".to_string()))?;
    ai::complete_chat(
        &settings,
        &api_key,
        "你是连接测试助手，只回复 OK。",
        "请回复 OK。",
        0.0,
    )
    .await
}

async fn list_models_impl(
    app: &AppState,
    input: Option<ListModelsInput>,
) -> AppResult<Vec<crate::models::ModelInfo>> {
    let mut settings = app.get_ai_settings()?;
    let input = input.unwrap_or(ListModelsInput {
        base_url: None,
        api_key: None,
    });

    if let Some(base_url) = input.base_url.filter(|value| !value.trim().is_empty()) {
        settings.base_url = base_url;
    }

    let api_key = input
        .api_key
        .filter(|value| !value.trim().is_empty())
        .or(app.get_api_key_for_base_url(&settings.base_url)?)
        .ok_or_else(|| AppError::Validation("请先为当前供应商保存 API Key".to_string()))?;

    ai::list_models(&settings, &api_key).await
}

fn read_required<T>(payload: &Value, key: &str) -> AppResult<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(
        payload
            .get(key)
            .cloned()
            .ok_or_else(|| AppError::Validation(format!("missing payload key: {key}")))?,
    )
    .map_err(|err| AppError::Validation(format!("invalid payload for {key}: {err}")))
}

fn read_optional<T>(payload: &Value, key: &str) -> AppResult<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    match payload.get(key).cloned() {
        Some(Value::Null) | None => Ok(None),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(|err| AppError::Validation(format!("invalid payload for {key}: {err}"))),
    }
}

fn read_i64(payload: &Value, keys: &[&str]) -> AppResult<i64> {
    keys.iter()
        .find_map(|key| payload.get(*key))
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::Validation(format!("missing integer field: {}", keys.join(" / "))))
}

fn read_string(payload: &Value, keys: &[&str]) -> AppResult<String> {
    keys.iter()
        .find_map(|key| payload.get(*key))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AppError::Validation(format!("missing string field: {}", keys.join(" / "))))
}

fn read_optional_string(payload: &Value, keys: &[&str]) -> AppResult<Option<String>> {
    match keys.iter().find_map(|key| payload.get(*key)) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(AppError::Validation(format!(
            "invalid string field: {}",
            keys.join(" / ")
        ))),
    }
}

fn resolve_dev_db_path() -> AppResult<PathBuf> {
    if let Ok(path) = env::var("BOOK_STUDIO_DEV_DB_PATH") {
        return Ok(PathBuf::from(path));
    }

    #[cfg(target_os = "macos")]
    {
        let home = env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| AppError::Validation("cannot resolve HOME for dev api".to_string()))?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("com.xiic.book-studio")
            .join("book-studio.sqlite3"))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(env::current_dir()?.join(".book-studio-dev.sqlite3"))
    }
}
