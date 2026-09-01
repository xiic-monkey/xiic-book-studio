use tauri::State;

use crate::{
    application::ApplicationGateway,
    error::AppResult,
    models::{
        ActionProposal, ActiveAgentRun, AdoptionBatchResult, AdoptionProposal, Agent,
        AgentRunRequest, AgentRunSummary, AgentStepResult, AgentToolDefinition, AiProvider,
        AiSettings, AiSpanRevisionRequest, Approval, Artifact, ArtifactFilters, ArtifactSummary,
        Chapter, ChapterGateReport, ChapterGateRequest, ChapterSplitPlan, ChapterSplitPlanRequest,
        ChapterUpdate, ClearChapterHistoryRequest, ConfirmStoryBibleRequest,
        ConfirmStoryBibleReviewRequest, ContinuityReport, ContinuityReviewRequest,
        DecideActionProposalRequest, DecideAdoptionProposalsRequest, DeleteArtifactRequest,
        DeleteKnowledgeCardRequest, DerivedIndexJob, Foreshadowing, HistoryCleanupResult,
        ImportReferenceTextRequest, KnowledgeCard, LedgerContinuityCheckRequest,
        LedgerContinuityReport, ListActionProposalsRequest, ListAdoptionProposalsRequest,
        ListModelsInput, NewChapter, NewProject, PrepareArtifactAdoptionsRequest, PreparedContext,
        Project, ProjectDetail, ProjectUpdate, ProjectWorkspace, ProposalApplyResult,
        ProviderCapabilities, QualityReport, RebuildStoryIndexRequest,
        RebuildStorySearchIndexRequest, ReferenceMaterial, RetryIndexJobsRequest, RevisionRequest,
        RunEvent, RunStoryArchitectRequest, SaveAgentSettings, SaveAiProvider, SaveAiSettings,
        SaveForeshadowing, SaveKnowledgeCard, SaveWritingSkill, SpanReplacementRequest, StoryBible,
        StoryBibleReview, StoryBibleReviewRequest, StoryContextRerankRequest,
        StoryContextRerankResult, StoryContextSearchInput, StoryContextSnippet,
        StoryFactSearchResult, StoryIndexSummary, TestAiConnectionInput,
        UpdateAdoptionProposalRequest, UpdateReferenceMaterialRequest, WritingSkill,
    },
};

#[tauri::command]
pub fn create_project(
    gateway: State<'_, ApplicationGateway>,
    input: NewProject,
) -> AppResult<Project> {
    gateway.create_project(input)
}

#[tauri::command]
pub fn list_projects(gateway: State<'_, ApplicationGateway>) -> AppResult<Vec<Project>> {
    gateway.list_projects()
}

#[tauri::command]
pub fn get_project(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
) -> AppResult<ProjectDetail> {
    gateway.get_project_detail(project_id)
}

#[tauri::command]
pub fn update_project(
    gateway: State<'_, ApplicationGateway>,
    input: ProjectUpdate,
) -> AppResult<Project> {
    gateway.update_project(input)
}

#[tauri::command]
pub fn delete_project(gateway: State<'_, ApplicationGateway>, project_id: i64) -> AppResult<()> {
    gateway.delete_project(project_id)
}

#[tauri::command]
pub fn import_reference_text(
    gateway: State<'_, ApplicationGateway>,
    input: ImportReferenceTextRequest,
) -> AppResult<ReferenceMaterial> {
    gateway.import_reference_text(input)
}

#[tauri::command]
pub fn list_reference_materials(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
) -> AppResult<Vec<ReferenceMaterial>> {
    gateway.list_reference_materials(project_id)
}

#[tauri::command]
pub fn update_reference_material(
    gateway: State<'_, ApplicationGateway>,
    input: UpdateReferenceMaterialRequest,
) -> AppResult<ReferenceMaterial> {
    gateway.update_reference_material(input)
}

#[tauri::command]
pub fn remove_reference_material(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
    reference_id: u64,
) -> AppResult<()> {
    gateway.remove_reference_material(project_id, reference_id)
}

#[tauri::command]
pub fn create_chapter(
    gateway: State<'_, ApplicationGateway>,
    input: NewChapter,
) -> AppResult<Chapter> {
    gateway.create_chapter(input)
}

#[tauri::command]
pub fn delete_chapter(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    gateway.delete_chapter(project_id, chapter_id)
}

#[tauri::command]
pub fn update_chapter(
    gateway: State<'_, ApplicationGateway>,
    input: ChapterUpdate,
) -> AppResult<Chapter> {
    gateway.update_chapter(input)
}

#[tauri::command]
pub fn get_settings(gateway: State<'_, ApplicationGateway>) -> AppResult<AiSettings> {
    gateway.get_settings()
}

#[tauri::command]
pub fn save_ai_settings(
    gateway: State<'_, ApplicationGateway>,
    input: SaveAiSettings,
) -> AppResult<AiSettings> {
    gateway.save_ai_settings(input)
}

#[tauri::command]
pub fn list_ai_providers(gateway: State<'_, ApplicationGateway>) -> AppResult<Vec<AiProvider>> {
    gateway.list_ai_providers()
}

#[tauri::command]
pub fn save_ai_provider(
    gateway: State<'_, ApplicationGateway>,
    input: SaveAiProvider,
) -> AppResult<AiProvider> {
    gateway.save_ai_provider(input)
}

#[tauri::command]
pub fn delete_ai_provider(
    gateway: State<'_, ApplicationGateway>,
    provider_id: i64,
) -> AppResult<()> {
    gateway.delete_ai_provider(provider_id)
}

#[tauri::command]
pub fn list_agents(gateway: State<'_, ApplicationGateway>) -> AppResult<Vec<Agent>> {
    gateway.list_agents()
}

#[tauri::command]
pub fn list_agent_tools(gateway: State<'_, ApplicationGateway>) -> Vec<AgentToolDefinition> {
    gateway.list_agent_tools()
}

#[tauri::command]
pub fn save_agent_settings(
    gateway: State<'_, ApplicationGateway>,
    input: SaveAgentSettings,
) -> AppResult<crate::models::Agent> {
    gateway.save_agent_settings(input)
}

#[tauri::command]
pub fn reset_agent_prompt(
    gateway: State<'_, ApplicationGateway>,
    agent_id: i64,
) -> AppResult<Agent> {
    gateway.reset_agent_prompt(agent_id)
}

#[tauri::command]
pub fn list_writing_skills(gateway: State<'_, ApplicationGateway>) -> AppResult<Vec<WritingSkill>> {
    gateway.list_writing_skills()
}

#[tauri::command]
pub fn save_writing_skill(
    gateway: State<'_, ApplicationGateway>,
    input: SaveWritingSkill,
) -> AppResult<WritingSkill> {
    gateway.save_writing_skill(input)
}

#[tauri::command]
pub fn save_knowledge_card(
    gateway: State<'_, ApplicationGateway>,
    input: SaveKnowledgeCard,
) -> AppResult<KnowledgeCard> {
    gateway.save_knowledge_card(input)
}

#[tauri::command]
pub fn delete_knowledge_card(
    gateway: State<'_, ApplicationGateway>,
    input: DeleteKnowledgeCardRequest,
) -> AppResult<()> {
    gateway.delete_knowledge_card(input)
}

#[tauri::command]
pub fn save_foreshadowing(
    gateway: State<'_, ApplicationGateway>,
    input: SaveForeshadowing,
) -> AppResult<Foreshadowing> {
    gateway.save_foreshadowing(input)
}

#[tauri::command]
pub async fn prepare_artifact_adoptions(
    gateway: State<'_, ApplicationGateway>,
    input: PrepareArtifactAdoptionsRequest,
) -> AppResult<Vec<AdoptionProposal>> {
    gateway.prepare_artifact_adoptions(input).await
}

#[tauri::command]
pub fn list_adoption_proposals(
    gateway: State<'_, ApplicationGateway>,
    input: ListAdoptionProposalsRequest,
) -> AppResult<Vec<AdoptionProposal>> {
    gateway.list_adoption_proposals(input)
}

#[tauri::command]
pub fn update_adoption_proposal(
    gateway: State<'_, ApplicationGateway>,
    input: UpdateAdoptionProposalRequest,
) -> AppResult<AdoptionProposal> {
    gateway.update_adoption_proposal(input)
}

#[tauri::command]
pub fn apply_adoption_proposals(
    gateway: State<'_, ApplicationGateway>,
    input: DecideAdoptionProposalsRequest,
) -> AppResult<AdoptionBatchResult> {
    gateway.apply_adoption_proposals(input)
}

#[tauri::command]
pub fn reject_adoption_proposals(
    gateway: State<'_, ApplicationGateway>,
    input: DecideAdoptionProposalsRequest,
) -> AppResult<AdoptionBatchResult> {
    gateway.reject_adoption_proposals(input)
}

#[tauri::command]
pub async fn test_ai_connection(
    gateway: State<'_, ApplicationGateway>,
    input: Option<TestAiConnectionInput>,
) -> AppResult<String> {
    gateway.test_ai_connection(input).await
}

#[tauri::command]
pub async fn list_models(
    gateway: State<'_, ApplicationGateway>,
    input: Option<ListModelsInput>,
) -> AppResult<Vec<crate::models::ModelInfo>> {
    gateway.list_models(input).await
}

#[tauri::command]
pub fn confirm_story_bible(
    gateway: State<'_, ApplicationGateway>,
    input: ConfirmStoryBibleRequest,
) -> AppResult<StoryBible> {
    gateway.confirm_story_bible(input)
}

#[tauri::command]
pub async fn review_story_bible(
    gateway: State<'_, ApplicationGateway>,
    input: StoryBibleReviewRequest,
) -> AppResult<StoryBibleReview> {
    gateway.review_story_bible(input).await
}

#[tauri::command]
pub fn confirm_story_bible_review(
    gateway: State<'_, ApplicationGateway>,
    input: ConfirmStoryBibleReviewRequest,
) -> AppResult<StoryBibleReview> {
    gateway.confirm_story_bible_review(input)
}

#[tauri::command]
pub fn list_story_arcs(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
) -> AppResult<Vec<crate::models::StoryArc>> {
    gateway.list_story_arcs(project_id)
}

#[tauri::command]
pub fn approve_stage(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
    stage: String,
    artifact_id: i64,
    note: Option<String>,
) -> AppResult<Approval> {
    gateway.approve_stage(project_id, &stage, artifact_id, note.as_deref())
}

#[tauri::command]
pub fn retry_index_jobs(
    gateway: State<'_, ApplicationGateway>,
    input: RetryIndexJobsRequest,
) -> AppResult<Vec<crate::models::DerivedIndexJob>> {
    gateway.retry_index_jobs(input)
}

#[tauri::command]
pub async fn rebuild_story_index(
    gateway: State<'_, ApplicationGateway>,
    input: RebuildStoryIndexRequest,
) -> AppResult<Vec<StoryIndexSummary>> {
    gateway.rebuild_story_index(input).await
}

#[tauri::command]
pub async fn rebuild_story_search_index(
    gateway: State<'_, ApplicationGateway>,
    input: RebuildStorySearchIndexRequest,
) -> AppResult<crate::models::StorySearchStatus> {
    gateway.rebuild_story_search_index(input).await
}

#[tauri::command]
pub fn get_story_search_status(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
) -> AppResult<crate::models::StorySearchStatus> {
    gateway.get_story_search_status(project_id)
}

#[tauri::command]
pub fn replace_artifact_span(
    gateway: State<'_, ApplicationGateway>,
    input: SpanReplacementRequest,
) -> AppResult<AgentStepResult> {
    gateway.replace_artifact_span(input)
}

#[tauri::command]
pub async fn revise_artifact_span_with_ai(
    gateway: State<'_, ApplicationGateway>,
    input: AiSpanRevisionRequest,
) -> AppResult<AgentStepResult> {
    gateway.revise_artifact_span_with_ai(input).await
}

#[tauri::command]
pub fn delete_artifact(
    gateway: State<'_, ApplicationGateway>,
    input: DeleteArtifactRequest,
) -> AppResult<()> {
    gateway.delete_artifact(input)
}

#[tauri::command]
pub fn clear_chapter_history(
    gateway: State<'_, ApplicationGateway>,
    input: ClearChapterHistoryRequest,
) -> AppResult<HistoryCleanupResult> {
    gateway.clear_chapter_history(input)
}

#[tauri::command]
pub fn export_project(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
    format: String,
) -> AppResult<String> {
    gateway.export_project(project_id, &format)
}

#[tauri::command]
pub fn analyze_artifact_quality(
    gateway: State<'_, ApplicationGateway>,
    project_id: i64,
    artifact_id: i64,
) -> AppResult<QualityReport> {
    gateway.analyze_artifact_quality(project_id, artifact_id)
}

#[tauri::command]
pub async fn review_project_continuity(
    gateway: State<'_, ApplicationGateway>,
    input: ContinuityReviewRequest,
) -> AppResult<ContinuityReport> {
    gateway.review_project_continuity(input).await
}

#[tauri::command]
pub async fn check_artifact_ledger_continuity(
    gateway: State<'_, ApplicationGateway>,
    input: LedgerContinuityCheckRequest,
) -> AppResult<LedgerContinuityReport> {
    gateway.check_artifact_ledger_continuity(input).await
}

#[tauri::command]
pub async fn analyze_chapter_gate(
    gateway: State<'_, ApplicationGateway>,
    input: ChapterGateRequest,
) -> AppResult<ChapterGateReport> {
    gateway.analyze_chapter_gate(input).await
}

#[tauri::command]
pub async fn generate_chapter_split_plan(
    gateway: State<'_, ApplicationGateway>,
    input: ChapterSplitPlanRequest,
) -> AppResult<ChapterSplitPlan> {
    gateway.generate_chapter_split_plan(input).await
}

#[tauri::command]
pub fn search_story_context(
    gateway: State<'_, ApplicationGateway>,
    input: StoryContextSearchInput,
) -> AppResult<Vec<StoryContextSnippet>> {
    gateway.search_story_context(input)
}

#[tauri::command]
pub fn search_story(
    gateway: State<'_, ApplicationGateway>,
    input: StoryContextSearchInput,
) -> AppResult<Vec<StoryContextSnippet>> {
    gateway.search_story(input)
}

#[tauri::command]
pub fn search_story_facts(
    gateway: State<'_, ApplicationGateway>,
    input: StoryContextSearchInput,
) -> AppResult<Vec<StoryFactSearchResult>> {
    gateway.search_story_facts(input)
}

#[tauri::command]
pub async fn rerank_story_context(
    gateway: State<'_, ApplicationGateway>,
    input: StoryContextRerankRequest,
) -> AppResult<StoryContextRerankResult> {
    gateway.rerank_story_context(input).await
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
