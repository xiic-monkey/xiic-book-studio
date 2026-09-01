use std::{convert::Infallible, env, net::SocketAddr, path::PathBuf, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use tower_http::cors::CorsLayer;

use crate::{
    application::ApplicationGateway,
    db::AppState,
    error::{AppError, AppResult},
    models::{
        AgentRunRequest, AiSpanRevisionRequest, ChapterGateRequest, ChapterSplitPlanRequest,
        ClearChapterHistoryRequest, ConfirmStoryBibleRequest, ConfirmStoryBibleReviewRequest,
        ContinuityReviewRequest, DecideActionProposalRequest, DecideAdoptionProposalsRequest,
        DeleteArtifactRequest, DeleteKnowledgeCardRequest, ImportReferenceTextRequest,
        LedgerContinuityCheckRequest, ListActionProposalsRequest, ListAdoptionProposalsRequest,
        ListModelsInput, PrepareArtifactAdoptionsRequest, RebuildStoryIndexRequest,
        RebuildStorySearchIndexRequest, RetryIndexJobsRequest, RevisionRequest,
        RunStoryArchitectRequest, SaveAgentSettings, SaveAiProvider, SaveAiSettings,
        SaveForeshadowing, SaveKnowledgeCard, SaveWritingSkill, SpanReplacementRequest,
        StoryBibleReviewRequest, StoryContextRerankRequest, StoryContextSearchInput,
        TestAiConnectionInput, UpdateAdoptionProposalRequest, UpdateReferenceMaterialRequest,
    },
};

#[derive(Clone)]
struct DevServerState {
    gateway: ApplicationGateway,
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

#[derive(Debug, Deserialize)]
struct RunEventQuery {
    project_id: Option<i64>,
    run_id: Option<i64>,
}

pub async fn run() -> AppResult<()> {
    let db_path = resolve_dev_db_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let app = AppState::from_path(db_path.clone())?;
    let gateway = ApplicationGateway::new(app);
    gateway.start_background_workers();
    let state = DevServerState { gateway, db_path };

    let port = env::var("BOOK_STUDIO_DEV_API_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(4141);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let router = Router::new()
        .route("/health", get(health))
        .route("/meta", get(meta))
        .route("/events/agent-runs", get(agent_run_events))
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

async fn agent_run_events(
    State(state): State<DevServerState>,
    Query(query): Query<RunEventQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream =
        BroadcastStream::new(state.gateway.subscribe_run_events()).filter_map(move |message| {
            let event = message.ok()?;
            if query
                .project_id
                .is_some_and(|project_id| event.project_id != project_id)
                || query.run_id.is_some_and(|run_id| event.run_id != run_id)
            {
                return None;
            }
            let data = serde_json::to_string(&event).ok()?;
            Some(Ok(Event::default().event("run_event").data(data)))
        });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn run_command(
    Path(command): Path<String>,
    State(state): State<DevServerState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    match dispatch_command(&state.gateway, &command, payload).await {
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

async fn dispatch_command(
    gateway: &ApplicationGateway,
    command: &str,
    payload: Value,
) -> AppResult<Value> {
    match command {
        "list_projects" => Ok(serde_json::to_value(gateway.list_projects()?)?),
        "create_project" => Ok(serde_json::to_value(
            gateway.create_project(read_required(&payload, "input")?)?,
        )?),
        "update_project" => Ok(serde_json::to_value(
            gateway.update_project(read_required(&payload, "input")?)?,
        )?),
        "delete_project" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            gateway.delete_project(project_id)?;
            Ok(Value::Null)
        }
        "import_reference_text" => {
            let input: ImportReferenceTextRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.import_reference_text(input)?)?)
        }
        "list_reference_materials" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(
                gateway.list_reference_materials(project_id)?,
            )?)
        }
        "update_reference_material" => {
            let input: UpdateReferenceMaterialRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.update_reference_material(input)?,
            )?)
        }
        "remove_reference_material" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let reference_id = payload
                .get("referenceId")
                .or_else(|| payload.get("reference_id"))
                .and_then(Value::as_u64)
                .ok_or_else(|| AppError::Validation("missing reference id".to_string()))?;
            gateway.remove_reference_material(project_id, reference_id)?;
            Ok(Value::Null)
        }
        "create_chapter" => Ok(serde_json::to_value(
            gateway.create_chapter(read_required(&payload, "input")?)?,
        )?),
        "delete_chapter" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let chapter_id = read_i64(&payload, &["chapterId", "chapter_id"])?;
            gateway.delete_chapter(project_id, chapter_id)?;
            Ok(Value::Null)
        }
        "update_chapter" => Ok(serde_json::to_value(
            gateway.update_chapter(read_required(&payload, "input")?)?,
        )?),
        "get_project" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(
                gateway.get_project_detail(project_id)?,
            )?)
        }
        "get_settings" => Ok(serde_json::to_value(gateway.get_settings()?)?),
        "save_ai_settings" => {
            let input: SaveAiSettings = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.save_ai_settings(input)?)?)
        }
        "list_ai_providers" => Ok(serde_json::to_value(gateway.list_ai_providers()?)?),
        "save_ai_provider" => {
            let input: SaveAiProvider = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.save_ai_provider(input)?)?)
        }
        "delete_ai_provider" => {
            let provider_id = read_i64(&payload, &["providerId", "provider_id"])?;
            gateway.delete_ai_provider(provider_id)?;
            Ok(Value::Null)
        }
        "list_agents" => Ok(serde_json::to_value(gateway.list_agents()?)?),
        "list_agent_tools" => Ok(serde_json::to_value(gateway.list_agent_tools())?),
        "save_agent_settings" => {
            let input: SaveAgentSettings = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.save_agent_settings(input)?)?)
        }
        "reset_agent_prompt" => {
            let agent_id = read_i64(&payload, &["agentId", "agent_id"])?;
            Ok(serde_json::to_value(gateway.reset_agent_prompt(agent_id)?)?)
        }
        "list_writing_skills" => Ok(serde_json::to_value(gateway.list_writing_skills()?)?),
        "save_writing_skill" => {
            let input: SaveWritingSkill = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.save_writing_skill(input)?)?)
        }
        "save_knowledge_card" => {
            let input: SaveKnowledgeCard = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.save_knowledge_card(input)?)?)
        }
        "delete_knowledge_card" => {
            let input: DeleteKnowledgeCardRequest = read_required(&payload, "input")?;
            gateway.delete_knowledge_card(input)?;
            Ok(serde_json::to_value(())?)
        }
        "save_foreshadowing" => {
            let input: SaveForeshadowing = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.save_foreshadowing(input)?)?)
        }
        "prepare_artifact_adoptions" => {
            let input: PrepareArtifactAdoptionsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.prepare_artifact_adoptions(input).await?,
            )?)
        }
        "list_adoption_proposals" => {
            let input: ListAdoptionProposalsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.list_adoption_proposals(input)?,
            )?)
        }
        "update_adoption_proposal" => {
            let input: UpdateAdoptionProposalRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.update_adoption_proposal(input)?,
            )?)
        }
        "apply_adoption_proposals" => {
            let input: DecideAdoptionProposalsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.apply_adoption_proposals(input)?,
            )?)
        }
        "reject_adoption_proposals" => {
            let input: DecideAdoptionProposalsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.reject_adoption_proposals(input)?,
            )?)
        }
        "test_ai_connection" => {
            let input: Option<TestAiConnectionInput> = read_optional(&payload, "input")?;
            let result = gateway.test_ai_connection(input).await?;
            Ok(Value::String(result))
        }
        "list_models" => {
            let input: Option<ListModelsInput> = read_optional(&payload, "input")?;
            Ok(serde_json::to_value(gateway.list_models(input).await?)?)
        }
        "confirm_story_bible" => {
            let input: ConfirmStoryBibleRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.confirm_story_bible(input)?)?)
        }
        "review_story_bible" => {
            let input: StoryBibleReviewRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.review_story_bible(input).await?,
            )?)
        }
        "confirm_story_bible_review" => {
            let input: ConfirmStoryBibleReviewRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.confirm_story_bible_review(input)?,
            )?)
        }
        "list_story_arcs" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(gateway.list_story_arcs(project_id)?)?)
        }
        "approve_stage" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let stage = read_string(&payload, &["stage"])?;
            let artifact_id = read_i64(&payload, &["artifactId", "artifact_id"])?;
            let note = read_optional_string(&payload, &["note"])?;
            Ok(serde_json::to_value(gateway.approve_stage(
                project_id,
                &stage,
                artifact_id,
                note.as_deref(),
            )?)?)
        }
        "retry_index_jobs" => {
            let input: RetryIndexJobsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.retry_index_jobs(input)?)?)
        }
        "rebuild_story_index" => {
            let input: RebuildStoryIndexRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.rebuild_story_index(input).await?,
            )?)
        }
        "rebuild_story_search_index" => {
            let input: RebuildStorySearchIndexRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.rebuild_story_search_index(input).await?,
            )?)
        }
        "get_story_search_status" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(
                gateway.get_story_search_status(project_id)?,
            )?)
        }
        "replace_artifact_span" => {
            let input: SpanReplacementRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.replace_artifact_span(input)?)?)
        }
        "revise_artifact_span_with_ai" => {
            let input: AiSpanRevisionRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.revise_artifact_span_with_ai(input).await?,
            )?)
        }
        "delete_artifact" => {
            let input: DeleteArtifactRequest = read_required(&payload, "input")?;
            gateway.delete_artifact(input)?;
            Ok(Value::Null)
        }
        "clear_chapter_history" => {
            let input: ClearChapterHistoryRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.clear_chapter_history(input)?)?)
        }
        "export_project" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let format = read_string(&payload, &["format"])?;
            Ok(Value::String(gateway.export_project(project_id, &format)?))
        }
        "analyze_artifact_quality" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let artifact_id = read_i64(&payload, &["artifactId", "artifact_id"])?;
            Ok(serde_json::to_value(
                gateway.analyze_artifact_quality(project_id, artifact_id)?,
            )?)
        }
        "review_project_continuity" => {
            let input: ContinuityReviewRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.review_project_continuity(input).await?,
            )?)
        }
        "check_artifact_ledger_continuity" => {
            let input: LedgerContinuityCheckRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.check_artifact_ledger_continuity(input).await?,
            )?)
        }
        "analyze_chapter_gate" => {
            let input: ChapterGateRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.analyze_chapter_gate(input).await?,
            )?)
        }
        "generate_chapter_split_plan" => {
            let input: ChapterSplitPlanRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.generate_chapter_split_plan(input).await?,
            )?)
        }
        "search_story_context" => {
            let input: StoryContextSearchInput = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.search_story_context(input)?)?)
        }
        "rerank_story_context" => {
            let input: StoryContextRerankRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.rerank_story_context(input).await?,
            )?)
        }
        "preview_agent_run" => {
            let input: AgentRunRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.preview_agent_run(input).await?,
            )?)
        }
        "start_agent_run" => {
            let input: AgentRunRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.start_agent_run(input).await?)?)
        }
        "start_story_architect_run" => {
            let input: RunStoryArchitectRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.start_story_architect_run(input).await?,
            )?)
        }
        "start_revision_run" => {
            let input: RevisionRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.start_revision_run(input).await?,
            )?)
        }
        "cancel_agent_run" => {
            let run_id = read_i64(&payload, &["runId", "run_id"])?;
            Ok(serde_json::to_value(gateway.cancel_agent_run(run_id)?)?)
        }
        "get_agent_run" => {
            let run_id = read_i64(&payload, &["runId", "run_id"])?;
            Ok(serde_json::to_value(gateway.get_agent_run(run_id)?)?)
        }
        "list_run_events" => {
            let run_id = read_i64(&payload, &["runId", "run_id"])?;
            let after_sequence = payload
                .get("afterSequence")
                .or_else(|| payload.get("after_sequence"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            Ok(serde_json::to_value(
                gateway.list_run_events(run_id, after_sequence)?,
            )?)
        }
        "get_active_agent_run" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(
                gateway.get_active_agent_run(project_id)?,
            )?)
        }
        "get_project_workspace" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(
                gateway.get_project_workspace(project_id)?,
            )?)
        }
        "get_artifact_v2" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            let artifact_id = read_i64(&payload, &["artifactId", "artifact_id"])?;
            Ok(serde_json::to_value(
                gateway.get_artifact(project_id, artifact_id)?,
            )?)
        }
        "list_artifact_summaries" => {
            let filters = read_required(&payload, "filters")?;
            Ok(serde_json::to_value(
                gateway.list_artifact_summaries(filters)?,
            )?)
        }
        "list_index_jobs" => {
            let project_id = read_i64(&payload, &["projectId", "project_id"])?;
            Ok(serde_json::to_value(gateway.list_index_jobs(project_id)?)?)
        }
        "get_provider_capabilities" => {
            let provider_base_url = payload
                .get("providerBaseUrl")
                .or_else(|| payload.get("provider_base_url"))
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::Validation("missing provider base URL".to_string()))?;
            Ok(serde_json::to_value(
                gateway.get_provider_capabilities(provider_base_url)?,
            )?)
        }
        "list_action_proposals_v2" => {
            let input: ListActionProposalsRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.list_action_proposals(input)?)?)
        }
        "apply_action_proposal" => {
            let input: DecideActionProposalRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(gateway.apply_action_proposal(input)?)?)
        }
        "reject_action_proposal" => {
            let input: DecideActionProposalRequest = read_required(&payload, "input")?;
            Ok(serde_json::to_value(
                gateway.reject_action_proposal(input)?,
            )?)
        }
        _ => Err(AppError::Validation(format!(
            "unknown dev command: {command}"
        ))),
    }
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
            .join("book-studio-v2.sqlite3"))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(env::current_dir()?.join(".book-studio-dev-v2.sqlite3"))
    }
}
