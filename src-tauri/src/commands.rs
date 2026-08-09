use tauri::State;

use crate::{
    adoption, ai,
    application::ApplicationGateway,
    chapter_memory, context_search, continuity_ledger,
    db::AppState,
    error::AppResult,
    gate, index_jobs,
    models::{
        ActionProposal, ActiveAgentRun, AdoptionBatchResult, AdoptionProposal, Agent,
        AgentRunRequest, AgentRunSummary, AgentStepResult, AgentToolDefinition, AiProvider,
        AiSettings, AiSpanRevisionRequest, Approval, Artifact, ArtifactFilters, ArtifactSummary,
        Chapter, ChapterGateReport, ChapterGateRequest, ChapterMemoryRecord, ChapterSplitPlan,
        ChapterSplitPlanRequest, ChapterUpdate, ClearChapterHistoryRequest,
        ConfirmStoryBibleRequest, ConfirmStoryBibleReviewRequest, ContextPreview, ContinuityReport,
        ContinuityReviewRequest, DecideActionProposalRequest, DecideAdoptionProposalsRequest,
        DeleteArtifactRequest, DerivedIndexJob, Foreshadowing, HistoryCleanupResult,
        ImportReferenceTextRequest, KnowledgeCard, LedgerContinuityCheckRequest,
        LedgerContinuityReport, LegacyAgentPrompt, ListActionProposalsRequest,
        ListAdoptionProposalsRequest, ListModelsInput, NewChapter, NewProject,
        PrepareArtifactAdoptionsRequest, PreparedContext, Project, ProjectDetail, ProjectUpdate,
        ProjectWorkspace, ProposalApplyResult, ProviderCapabilities, QualityReport,
        RebuildChapterMemoryRequest, RebuildStoryIndexRequest, RebuildStorySearchIndexRequest,
        ReferenceMaterial, RetryIndexJobsRequest, RevisionRequest, RunAgentRequest, RunEvent,
        RunStoryArchitectRequest, SaveAgentSettings, SaveAiProvider, SaveAiSettings,
        SaveForeshadowing, SaveKnowledgeCard, SaveWritingSkill, SpanReplacementRequest, StoryBible,
        StoryBibleReview, StoryBibleReviewRequest, StoryContextRerankRequest,
        StoryContextRerankResult, StoryContextSearchInput, StoryContextSnippet, StoryIndexSummary,
        TestAiConnectionInput, UpdateAdoptionProposalRequest, UpdateReferenceMaterialRequest,
        WritingSkill,
    },
    quality, story_architecture, story_index, story_search, workflow,
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
pub fn import_reference_text(
    state: State<'_, AppState>,
    input: ImportReferenceTextRequest,
) -> AppResult<ReferenceMaterial> {
    state.import_reference_text(input)
}

#[tauri::command]
pub fn list_reference_materials(
    state: State<'_, AppState>,
    project_id: i64,
) -> AppResult<Vec<ReferenceMaterial>> {
    state.list_reference_materials(project_id)
}

#[tauri::command]
pub fn update_reference_material(
    state: State<'_, AppState>,
    input: UpdateReferenceMaterialRequest,
) -> AppResult<ReferenceMaterial> {
    state.update_reference_material(input)
}

#[tauri::command]
pub fn remove_reference_material(
    state: State<'_, AppState>,
    project_id: i64,
    reference_id: u64,
) -> AppResult<()> {
    state.remove_reference_material(project_id, reference_id)
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
pub async fn update_chapter(
    state: State<'_, AppState>,
    input: ChapterUpdate,
) -> AppResult<Chapter> {
    let chapter = state.update_chapter(input)?;
    if let Err(error) =
        story_search::refresh_chapter_metadata(&state, chapter.project_id, chapter.id)
    {
        eprintln!("chapter search metadata refresh unavailable; queueing project rebuild: {error}");
        if let Err(queue_error) = index_jobs::enqueue_project_search_job(&state, chapter.project_id)
        {
            eprintln!("unable to queue chapter search rebuild: {queue_error}");
        }
    }
    Ok(chapter)
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
pub fn list_ai_providers(state: State<'_, AppState>) -> AppResult<Vec<AiProvider>> {
    state.list_ai_providers()
}

#[tauri::command]
pub fn save_ai_provider(
    state: State<'_, AppState>,
    input: SaveAiProvider,
) -> AppResult<AiProvider> {
    state.save_ai_provider(input)
}

#[tauri::command]
pub fn delete_ai_provider(state: State<'_, AppState>, provider_id: i64) -> AppResult<()> {
    state.delete_ai_provider(provider_id)
}

#[tauri::command]
pub fn list_agents(state: State<'_, AppState>) -> AppResult<Vec<Agent>> {
    state.list_agents()
}

#[tauri::command]
pub fn list_agent_tools() -> Vec<AgentToolDefinition> {
    crate::agent_tools::definitions()
}

#[tauri::command]
pub fn save_agent_settings(
    state: State<'_, AppState>,
    input: SaveAgentSettings,
) -> AppResult<crate::models::Agent> {
    state.save_agent_settings(input)
}

#[tauri::command]
pub fn reset_agent_prompt(
    gateway: State<'_, ApplicationGateway>,
    agent_id: i64,
) -> AppResult<Agent> {
    gateway.reset_agent_prompt(agent_id)
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
    let card = state.save_knowledge_card(input)?;
    if let Err(error) = index_jobs::enqueue_project_search_job(&state, card.project_id) {
        eprintln!("knowledge card search refresh unavailable; queueing project rebuild: {error}");
    }
    Ok(card)
}

#[tauri::command]
pub fn save_foreshadowing(
    state: State<'_, AppState>,
    input: SaveForeshadowing,
) -> AppResult<Foreshadowing> {
    let item = state.save_foreshadowing(input)?;
    if let Err(error) = index_jobs::enqueue_project_search_job(&state, item.project_id) {
        eprintln!("foreshadowing search refresh unavailable; queueing project rebuild: {error}");
    }
    Ok(item)
}

#[tauri::command]
pub async fn prepare_artifact_adoptions(
    state: State<'_, AppState>,
    input: PrepareArtifactAdoptionsRequest,
) -> AppResult<Vec<AdoptionProposal>> {
    adoption::prepare_artifact_adoptions(&state, input.project_id, input.artifact_id).await
}

#[tauri::command]
pub fn list_adoption_proposals(
    state: State<'_, AppState>,
    input: ListAdoptionProposalsRequest,
) -> AppResult<Vec<AdoptionProposal>> {
    adoption::list_adoption_proposals(&state, input.project_id, input.artifact_id)
}

#[tauri::command]
pub fn update_adoption_proposal(
    state: State<'_, AppState>,
    input: UpdateAdoptionProposalRequest,
) -> AppResult<AdoptionProposal> {
    adoption::update_adoption_proposal(&state, input)
}

#[tauri::command]
pub fn apply_adoption_proposals(
    state: State<'_, AppState>,
    input: DecideAdoptionProposalsRequest,
) -> AppResult<AdoptionBatchResult> {
    adoption::apply_adoption_proposals(&state, input)
}

#[tauri::command]
pub fn reject_adoption_proposals(
    state: State<'_, AppState>,
    input: DecideAdoptionProposalsRequest,
) -> AppResult<AdoptionBatchResult> {
    adoption::reject_adoption_proposals(&state, input)
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
        thinking_level: None,
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
    if let Some(thinking_level) = input.thinking_level.as_deref() {
        settings.thinking_level = thinking_level.to_string();
    }
    settings.thinking_level = crate::models::normalize_thinking_level(
        settings.thinking_enabled,
        &settings.thinking_level,
    )
    .map_err(crate::error::AppError::Validation)?;

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
pub async fn rebuild_chapter_memory(
    state: State<'_, AppState>,
    input: RebuildChapterMemoryRequest,
) -> AppResult<ChapterMemoryRecord> {
    let memory_agent = state.get_agent("chapter_memory")?;
    let settings = memory_agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| {
            crate::error::AppError::Validation(
                "请先在设置里为当前供应商保存 AI API Key".to_string(),
            )
        })?;
    chapter_memory::rebuild_chapter_memory(
        &state,
        input.project_id,
        input.chapter_id,
        &settings,
        &api_key,
        None,
    )
    .await
}

#[tauri::command]
pub async fn run_story_architect(
    state: State<'_, AppState>,
    input: RunStoryArchitectRequest,
) -> AppResult<AgentStepResult> {
    story_architecture::run_story_architect(&state, input).await
}

#[tauri::command]
pub async fn create_targeted_rework(
    state: State<'_, AppState>,
    input: RunStoryArchitectRequest,
) -> AppResult<AgentStepResult> {
    story_architecture::create_targeted_rework(&state, input).await
}

#[tauri::command]
pub fn confirm_story_bible(
    state: State<'_, AppState>,
    input: ConfirmStoryBibleRequest,
) -> AppResult<StoryBible> {
    story_architecture::confirm_story_bible(&state, input)
}

#[tauri::command]
pub async fn review_story_bible(
    state: State<'_, AppState>,
    input: StoryBibleReviewRequest,
) -> AppResult<StoryBibleReview> {
    story_architecture::review_story_bible(&state, input).await
}

#[tauri::command]
pub fn confirm_story_bible_review(
    state: State<'_, AppState>,
    input: ConfirmStoryBibleReviewRequest,
) -> AppResult<StoryBibleReview> {
    story_architecture::confirm_story_bible_review(&state, input)
}

#[tauri::command]
pub fn list_story_arcs(
    state: State<'_, AppState>,
    project_id: i64,
) -> AppResult<Vec<crate::models::StoryArc>> {
    state.list_story_arcs(project_id)
}

#[tauri::command]
pub fn preview_agent_context(
    state: State<'_, AppState>,
    input: RunAgentRequest,
) -> AppResult<ContextPreview> {
    workflow::preview_agent_context(&state, input)
}

#[tauri::command]
pub async fn approve_stage(
    state: State<'_, AppState>,
    project_id: i64,
    stage: String,
    artifact_id: i64,
    note: Option<String>,
) -> AppResult<Approval> {
    let approval = state.approve_stage(
        project_id,
        &stage,
        artifact_id,
        note.as_deref().unwrap_or(""),
    )?;
    state.wake_index_worker();
    Ok(approval)
}

#[tauri::command]
pub fn retry_index_jobs(
    state: State<'_, AppState>,
    input: RetryIndexJobsRequest,
) -> AppResult<Vec<crate::models::DerivedIndexJob>> {
    index_jobs::retry_index_jobs(&state, input)
}

#[tauri::command]
pub async fn rebuild_story_index(
    state: State<'_, AppState>,
    input: RebuildStoryIndexRequest,
) -> AppResult<Vec<StoryIndexSummary>> {
    story_index::rebuild_story_index(&state, input).await
}

#[tauri::command]
pub async fn rebuild_story_search_index(
    state: State<'_, AppState>,
    input: RebuildStorySearchIndexRequest,
) -> AppResult<crate::models::StorySearchStatus> {
    story_search::rebuild_story_search_index(&state, input).await
}

#[tauri::command]
pub fn get_story_search_status(
    state: State<'_, AppState>,
    project_id: i64,
) -> AppResult<crate::models::StorySearchStatus> {
    story_search::get_story_search_status(&state, project_id)
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
    project_id: i64,
    artifact_id: i64,
) -> AppResult<QualityReport> {
    let artifact = state.get_artifact(artifact_id)?;
    if artifact.project_id != project_id {
        return Err(crate::error::AppError::Validation(
            "产物不属于当前项目".to_string(),
        ));
    }
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
pub async fn check_artifact_ledger_continuity(
    state: State<'_, AppState>,
    input: LedgerContinuityCheckRequest,
) -> AppResult<LedgerContinuityReport> {
    continuity_ledger::check_artifact_continuity(&state, input).await
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

#[tauri::command]
pub async fn rerank_story_context(
    state: State<'_, AppState>,
    input: StoryContextRerankRequest,
) -> AppResult<StoryContextRerankResult> {
    context_search::rerank_story_context(&state, input).await
}

#[tauri::command]
pub fn list_tool_definitions() -> Vec<AgentToolDefinition> {
    crate::agent_tools::definitions()
}

#[tauri::command]
pub async fn preview_agent_run(
    gateway: State<'_, ApplicationGateway>,
    input: AgentRunRequest,
) -> AppResult<PreparedContext> {
    gateway.preview_agent_run(input).await
}

#[tauri::command]
pub async fn start_agent_run(
    gateway: State<'_, ApplicationGateway>,
    input: AgentRunRequest,
) -> AppResult<AgentRunSummary> {
    gateway.start_agent_run(input).await
}

#[tauri::command]
pub async fn start_story_architect_run(
    gateway: State<'_, ApplicationGateway>,
    input: RunStoryArchitectRequest,
) -> AppResult<AgentRunSummary> {
    gateway.start_story_architect_run(input).await
}

#[tauri::command]
pub async fn start_revision_run(
    gateway: State<'_, ApplicationGateway>,
    input: RevisionRequest,
) -> AppResult<AgentRunSummary> {
    gateway.start_revision_run(input).await
}

#[tauri::command]
pub fn cancel_agent_run(
    gateway: State<'_, ApplicationGateway>,
    run_id: i64,
) -> AppResult<AgentRunSummary> {
    gateway.cancel_agent_run(run_id)
}

#[tauri::command]
pub fn get_agent_run(
    gateway: State<'_, ApplicationGateway>,
    run_id: i64,
) -> AppResult<AgentRunSummary> {
    gateway.get_agent_run(run_id)
}

#[tauri::command]
pub fn list_run_events(
    gateway: State<'_, ApplicationGateway>,
    run_id: i64,
    after_sequence: Option<i64>,
) -> AppResult<Vec<RunEvent>> {
    gateway.list_run_events(run_id, after_sequence.unwrap_or(0))
}

#[tauri::command]
pub fn get_active_agent_run(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
) -> AppResult<Option<ActiveAgentRun>> {
    gateway.get_active_agent_run(project_id)
}

#[tauri::command]
pub fn get_project_workspace(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
) -> AppResult<ProjectWorkspace> {
    gateway.get_project_workspace(project_id)
}

#[tauri::command]
pub fn get_artifact_v2(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
    artifact_id: i64,
) -> AppResult<Artifact> {
    gateway.get_artifact(project_id, artifact_id)
}

#[tauri::command]
pub fn list_artifact_summaries(
    gateway: State<'_, ApplicationGateway>,
    filters: ArtifactFilters,
) -> AppResult<Vec<ArtifactSummary>> {
    gateway.list_artifact_summaries(filters)
}

#[tauri::command]
pub fn list_index_jobs(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
) -> AppResult<Vec<DerivedIndexJob>> {
    gateway.list_index_jobs(project_id)
}

#[tauri::command]
pub fn list_legacy_agent_prompts(
    gateway: State<'_, ApplicationGateway>,
) -> AppResult<Vec<LegacyAgentPrompt>> {
    gateway.list_legacy_agent_prompts()
}

#[tauri::command]
pub fn get_provider_capabilities(
    gateway: State<'_, ApplicationGateway>,
    provider_base_url: String,
) -> AppResult<ProviderCapabilities> {
    gateway.get_provider_capabilities(&provider_base_url)
}

#[tauri::command]
pub fn list_action_proposals_v2(
    gateway: State<'_, ApplicationGateway>,
    input: ListActionProposalsRequest,
) -> AppResult<Vec<ActionProposal>> {
    gateway.list_action_proposals(input)
}

#[tauri::command]
pub fn apply_action_proposal(
    gateway: State<'_, ApplicationGateway>,
    input: DecideActionProposalRequest,
) -> AppResult<ProposalApplyResult> {
    gateway.apply_action_proposal(input)
}

#[tauri::command]
pub fn reject_action_proposal(
    gateway: State<'_, ApplicationGateway>,
    input: DecideActionProposalRequest,
) -> AppResult<ActionProposal> {
    gateway.reject_action_proposal(input)
}
