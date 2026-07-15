use tauri::State;

use crate::{
    ai,
    db::AppState,
    error::AppResult,
    gate,
    models::{
        AgentStepResult, AiSettings, AiSpanRevisionRequest, Approval, Artifact, ArtifactFilters,
        Chapter, ChapterGateReport, ChapterGateRequest, ChapterSplitPlan, ChapterSplitPlanRequest,
        ChapterUpdate, ClearChapterHistoryRequest, ContinuityReport, ContinuityReviewRequest,
        DeleteArtifactRequest, Foreshadowing, HistoryCleanupResult, KnowledgeCard, ListModelsInput,
        NewChapter, NewProject, Project, ProjectDetail, ProjectUpdate, QualityReport,
        RevisionRequest, RunAgentRequest, SaveAiSettings, SaveForeshadowing, SaveKnowledgeCard,
        SaveWritingSkill, SpanReplacementRequest, StoryContextSearchInput, StoryContextSnippet,
        TestAiConnectionInput, WritingSkill,
    },
    quality, workflow,
};

#[tauri::command]
pub fn create_project(state: State<'_, AppState>, input: NewProject) -> AppResult<Project> {
    state.create_project(input)
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    state.list_projects()
}

#[tauri::command]
pub fn get_project(state: State<'_, AppState>, project_id: i64) -> AppResult<ProjectDetail> {
    state.get_detail(project_id)
}

#[tauri::command]
pub fn update_project(state: State<'_, AppState>, input: ProjectUpdate) -> AppResult<Project> {
    state.update_project(input)
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, project_id: i64) -> AppResult<()> {
    state.delete_project(project_id)
}

#[tauri::command]
pub fn create_chapter(state: State<'_, AppState>, input: NewChapter) -> AppResult<Chapter> {
    state.create_chapter(input)
}

#[tauri::command]
pub fn delete_chapter(
    state: State<'_, AppState>,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    state.delete_chapter(project_id, chapter_id)
}

#[tauri::command]
pub fn update_chapter(state: State<'_, AppState>, input: ChapterUpdate) -> AppResult<Chapter> {
    state.update_chapter(input)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<AiSettings> {
    state.get_ai_settings()
}

#[tauri::command]
pub fn save_ai_settings(
    state: State<'_, AppState>,
    input: SaveAiSettings,
) -> AppResult<AiSettings> {
    state.save_ai_settings(input)
}

#[tauri::command]
pub fn list_writing_skills(state: State<'_, AppState>) -> AppResult<Vec<WritingSkill>> {
    state.list_writing_skills()
}

#[tauri::command]
pub fn save_writing_skill(
    state: State<'_, AppState>,
    input: SaveWritingSkill,
) -> AppResult<WritingSkill> {
    state.save_writing_skill(input)
}

#[tauri::command]
pub fn save_knowledge_card(
    state: State<'_, AppState>,
    input: SaveKnowledgeCard,
) -> AppResult<KnowledgeCard> {
    state.save_knowledge_card(input)
}

#[tauri::command]
pub fn save_foreshadowing(
    state: State<'_, AppState>,
    input: SaveForeshadowing,
) -> AppResult<Foreshadowing> {
    state.save_foreshadowing(input)
}

#[tauri::command]
pub async fn test_ai_connection(
    state: State<'_, AppState>,
    input: Option<TestAiConnectionInput>,
) -> AppResult<String> {
    let mut settings = state.get_ai_settings()?;
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
        return Err(crate::error::AppError::Validation(
            "请先填写模型名称，再测试连接".to_string(),
        ));
    }

    let api_key = input
        .api_key
        .filter(|value| !value.trim().is_empty())
        .or(state.get_api_key_for_base_url(&settings.base_url)?)
        .ok_or_else(|| {
            crate::error::AppError::Validation("请先为当前供应商保存 API Key".to_string())
        })?;
    ai::complete_chat(
        &settings,
        &api_key,
        "你是连接测试助手，只回复 OK。",
        "请回复 OK。",
        0.0,
    )
    .await
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    input: Option<ListModelsInput>,
) -> AppResult<Vec<crate::models::ModelInfo>> {
    let mut settings = state.get_ai_settings()?;
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
        .or(state.get_api_key_for_base_url(&settings.base_url)?)
        .ok_or_else(|| {
            crate::error::AppError::Validation("请先为当前供应商保存 API Key".to_string())
        })?;

    ai::list_models(&settings, &api_key).await
}

#[tauri::command]
pub async fn run_agent_step(
    state: State<'_, AppState>,
    input: RunAgentRequest,
) -> AppResult<AgentStepResult> {
    workflow::run_agent_step(&state, input).await
}

#[tauri::command]
pub fn approve_stage(
    state: State<'_, AppState>,
    project_id: i64,
    stage: String,
    artifact_id: i64,
    note: Option<String>,
) -> AppResult<Approval> {
    state.approve_stage(
        project_id,
        &stage,
        artifact_id,
        note.as_deref().unwrap_or(""),
    )
}

#[tauri::command]
pub async fn request_revision(
    state: State<'_, AppState>,
    input: RevisionRequest,
) -> AppResult<AgentStepResult> {
    workflow::request_revision(&state, input).await
}

#[tauri::command]
pub fn replace_artifact_span(
    state: State<'_, AppState>,
    input: SpanReplacementRequest,
) -> AppResult<AgentStepResult> {
    workflow::replace_artifact_span(&state, input)
}

#[tauri::command]
pub async fn revise_artifact_span_with_ai(
    state: State<'_, AppState>,
    input: AiSpanRevisionRequest,
) -> AppResult<AgentStepResult> {
    workflow::revise_artifact_span_with_ai(&state, input).await
}

#[tauri::command]
pub fn delete_artifact(state: State<'_, AppState>, input: DeleteArtifactRequest) -> AppResult<()> {
    state.delete_artifact(input.project_id, input.artifact_id)
}

#[tauri::command]
pub fn clear_chapter_history(
    state: State<'_, AppState>,
    input: ClearChapterHistoryRequest,
) -> AppResult<HistoryCleanupResult> {
    state.clear_chapter_history(
        input.project_id,
        input.chapter_id,
        input.keep_artifact_ids.as_deref().unwrap_or(&[]),
    )
}

#[tauri::command]
pub fn list_artifacts(
    state: State<'_, AppState>,
    filters: ArtifactFilters,
) -> AppResult<Vec<Artifact>> {
    state.list_artifacts(filters)
}

#[tauri::command]
pub fn export_project(
    state: State<'_, AppState>,
    project_id: i64,
    format: String,
) -> AppResult<String> {
    match format.as_str() {
        "markdown" | "md" => workflow::export_markdown(&state, project_id),
        _ => Err(crate::error::AppError::Validation(
            "第一版只支持 Markdown 导出".to_string(),
        )),
    }
}

#[tauri::command]
pub fn analyze_artifact_quality(
    state: State<'_, AppState>,
    artifact_id: i64,
) -> AppResult<QualityReport> {
    let artifact = state.get_artifact(artifact_id)?;
    Ok(quality::analyze_artifact(&artifact))
}

#[tauri::command]
pub async fn review_project_continuity(
    state: State<'_, AppState>,
    input: ContinuityReviewRequest,
) -> AppResult<ContinuityReport> {
    workflow::review_project_continuity(&state, input).await
}

#[tauri::command]
pub async fn analyze_chapter_gate(
    state: State<'_, AppState>,
    input: ChapterGateRequest,
) -> AppResult<ChapterGateReport> {
    gate::analyze_chapter_gate(&state, input).await
}

#[tauri::command]
pub async fn generate_chapter_split_plan(
    state: State<'_, AppState>,
    input: ChapterSplitPlanRequest,
) -> AppResult<ChapterSplitPlan> {
    workflow::generate_chapter_split_plan(&state, input).await
}

#[tauri::command]
pub fn search_story_context(
    state: State<'_, AppState>,
    input: StoryContextSearchInput,
) -> AppResult<Vec<StoryContextSnippet>> {
    workflow::search_story_context(&state, input)
}
