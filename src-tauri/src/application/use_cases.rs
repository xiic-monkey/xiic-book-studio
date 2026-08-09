use super::ApplicationGateway;
use crate::{
    adoption, ai, chapter_memory, context_search, continuity_ledger,
    error::{AppError, AppResult},
    gate, index_jobs,
    models::*,
    quality, story_architecture, story_index, story_search, workflow,
};

impl ApplicationGateway {
    pub fn create_project(&self, input: NewProject) -> AppResult<Project> {
        self.state.create_project(input)
    }

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        self.state.list_projects()
    }

    pub fn get_project_detail(&self, project_id: i64) -> AppResult<ProjectDetail> {
        self.state.get_detail(project_id)
    }

    pub fn update_project(&self, input: ProjectUpdate) -> AppResult<Project> {
        self.state.update_project(input)
    }

    pub fn delete_project(&self, project_id: i64) -> AppResult<()> {
        self.state.delete_project(project_id)
    }

    pub fn import_reference_text(
        &self,
        input: ImportReferenceTextRequest,
    ) -> AppResult<ReferenceMaterial> {
        self.state.import_reference_text(input)
    }

    pub fn list_reference_materials(&self, project_id: i64) -> AppResult<Vec<ReferenceMaterial>> {
        self.state.list_reference_materials(project_id)
    }

    pub fn update_reference_material(
        &self,
        input: UpdateReferenceMaterialRequest,
    ) -> AppResult<ReferenceMaterial> {
        self.state.update_reference_material(input)
    }

    pub fn remove_reference_material(&self, project_id: i64, reference_id: u64) -> AppResult<()> {
        self.state
            .remove_reference_material(project_id, reference_id)
    }

    pub fn create_chapter(&self, input: NewChapter) -> AppResult<Chapter> {
        self.state.create_chapter(input)
    }

    pub fn delete_chapter(&self, project_id: i64, chapter_id: i64) -> AppResult<()> {
        self.state.delete_chapter(project_id, chapter_id)
    }

    pub fn update_chapter(&self, input: ChapterUpdate) -> AppResult<Chapter> {
        let chapter = self.state.update_chapter(input)?;
        if let Err(error) =
            story_search::refresh_chapter_metadata(&self.state, chapter.project_id, chapter.id)
        {
            eprintln!(
                "chapter search metadata refresh unavailable; queueing project rebuild: {error}"
            );
            if let Err(queue_error) =
                index_jobs::enqueue_project_search_job(&self.state, chapter.project_id)
            {
                eprintln!("unable to queue chapter search rebuild: {queue_error}");
            }
        }
        Ok(chapter)
    }

    pub fn get_settings(&self) -> AppResult<AiSettings> {
        self.state.get_ai_settings()
    }

    pub fn save_ai_settings(&self, input: SaveAiSettings) -> AppResult<AiSettings> {
        self.state.save_ai_settings(input)
    }

    pub fn list_ai_providers(&self) -> AppResult<Vec<AiProvider>> {
        self.state.list_ai_providers()
    }

    pub fn save_ai_provider(&self, input: SaveAiProvider) -> AppResult<AiProvider> {
        self.state.save_ai_provider(input)
    }

    pub fn delete_ai_provider(&self, provider_id: i64) -> AppResult<()> {
        self.state.delete_ai_provider(provider_id)
    }

    pub fn list_agents(&self) -> AppResult<Vec<Agent>> {
        self.state.list_agents()
    }

    pub fn list_agent_tools(&self) -> Vec<AgentToolDefinition> {
        crate::agent_tools::definitions()
    }

    pub fn save_agent_settings(&self, input: SaveAgentSettings) -> AppResult<Agent> {
        self.state.save_agent_settings(input)
    }

    pub fn list_writing_skills(&self) -> AppResult<Vec<WritingSkill>> {
        self.state.list_writing_skills()
    }

    pub fn save_writing_skill(&self, input: SaveWritingSkill) -> AppResult<WritingSkill> {
        self.state.save_writing_skill(input)
    }

    pub fn save_knowledge_card(&self, input: SaveKnowledgeCard) -> AppResult<KnowledgeCard> {
        let card = self.state.save_knowledge_card(input)?;
        if let Err(error) = index_jobs::enqueue_project_search_job(&self.state, card.project_id) {
            eprintln!(
                "knowledge card search refresh unavailable; queueing project rebuild: {error}"
            );
        }
        Ok(card)
    }

    pub fn save_foreshadowing(&self, input: SaveForeshadowing) -> AppResult<Foreshadowing> {
        let item = self.state.save_foreshadowing(input)?;
        if let Err(error) = index_jobs::enqueue_project_search_job(&self.state, item.project_id) {
            eprintln!(
                "foreshadowing search refresh unavailable; queueing project rebuild: {error}"
            );
        }
        Ok(item)
    }

    pub async fn prepare_artifact_adoptions(
        &self,
        input: PrepareArtifactAdoptionsRequest,
    ) -> AppResult<Vec<AdoptionProposal>> {
        adoption::prepare_artifact_adoptions(&self.state, input.project_id, input.artifact_id).await
    }

    pub fn list_adoption_proposals(
        &self,
        input: ListAdoptionProposalsRequest,
    ) -> AppResult<Vec<AdoptionProposal>> {
        adoption::list_adoption_proposals(&self.state, input.project_id, input.artifact_id)
    }

    pub fn update_adoption_proposal(
        &self,
        input: UpdateAdoptionProposalRequest,
    ) -> AppResult<AdoptionProposal> {
        adoption::update_adoption_proposal(&self.state, input)
    }

    pub fn apply_adoption_proposals(
        &self,
        input: DecideAdoptionProposalsRequest,
    ) -> AppResult<AdoptionBatchResult> {
        adoption::apply_adoption_proposals(&self.state, input)
    }

    pub fn reject_adoption_proposals(
        &self,
        input: DecideAdoptionProposalsRequest,
    ) -> AppResult<AdoptionBatchResult> {
        adoption::reject_adoption_proposals(&self.state, input)
    }

    pub async fn test_ai_connection(
        &self,
        input: Option<TestAiConnectionInput>,
    ) -> AppResult<String> {
        let mut settings = self.state.get_ai_settings()?;
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
        settings.thinking_level =
            normalize_thinking_level(settings.thinking_enabled, &settings.thinking_level)
                .map_err(AppError::Validation)?;
        if settings.model.trim().is_empty() {
            return Err(AppError::Validation(
                "请先填写模型名称，再测试连接".to_string(),
            ));
        }
        let api_key = input
            .api_key
            .filter(|value| !value.trim().is_empty())
            .or(self.state.get_api_key_for_base_url(&settings.base_url)?)
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

    pub async fn list_models(&self, input: Option<ListModelsInput>) -> AppResult<Vec<ModelInfo>> {
        let mut settings = self.state.get_ai_settings()?;
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
            .or(self.state.get_api_key_for_base_url(&settings.base_url)?)
            .ok_or_else(|| AppError::Validation("请先为当前供应商保存 API Key".to_string()))?;
        ai::list_models(&settings, &api_key).await
    }

    pub async fn run_agent_step(&self, input: RunAgentRequest) -> AppResult<AgentStepResult> {
        workflow::run_agent_step(&self.state, input).await
    }

    pub async fn rebuild_chapter_memory(
        &self,
        input: RebuildChapterMemoryRequest,
    ) -> AppResult<ChapterMemoryRecord> {
        let settings = self.state.get_ai_settings_for_agent("chapter_memory")?;
        let api_key = self
            .state
            .get_api_key_for_base_url(&settings.base_url)?
            .ok_or_else(|| {
                AppError::Validation("请先在设置里为当前供应商保存 AI API Key".to_string())
            })?;
        chapter_memory::rebuild_chapter_memory(
            &self.state,
            input.project_id,
            input.chapter_id,
            &settings,
            &api_key,
            None,
        )
        .await
    }

    pub async fn run_story_architect(
        &self,
        input: RunStoryArchitectRequest,
    ) -> AppResult<AgentStepResult> {
        story_architecture::run_story_architect(&self.state, input).await
    }

    pub async fn create_targeted_rework(
        &self,
        input: RunStoryArchitectRequest,
    ) -> AppResult<AgentStepResult> {
        story_architecture::create_targeted_rework(&self.state, input).await
    }

    pub fn confirm_story_bible(&self, input: ConfirmStoryBibleRequest) -> AppResult<StoryBible> {
        story_architecture::confirm_story_bible(&self.state, input)
    }

    pub async fn review_story_bible(
        &self,
        input: StoryBibleReviewRequest,
    ) -> AppResult<StoryBibleReview> {
        story_architecture::review_story_bible(&self.state, input).await
    }

    pub fn confirm_story_bible_review(
        &self,
        input: ConfirmStoryBibleReviewRequest,
    ) -> AppResult<StoryBibleReview> {
        story_architecture::confirm_story_bible_review(&self.state, input)
    }

    pub fn list_story_arcs(&self, project_id: i64) -> AppResult<Vec<StoryArc>> {
        self.state.list_story_arcs(project_id)
    }

    pub fn preview_agent_context(&self, input: RunAgentRequest) -> AppResult<ContextPreview> {
        workflow::preview_agent_context(&self.state, input)
    }

    pub fn approve_stage(
        &self,
        project_id: i64,
        stage: &str,
        artifact_id: i64,
        note: Option<&str>,
    ) -> AppResult<Approval> {
        let approval =
            self.state
                .approve_stage(project_id, stage, artifact_id, note.unwrap_or(""))?;
        self.state.wake_index_worker();
        Ok(approval)
    }

    pub fn retry_index_jobs(
        &self,
        input: RetryIndexJobsRequest,
    ) -> AppResult<Vec<DerivedIndexJob>> {
        index_jobs::retry_index_jobs(&self.state, input)
    }

    pub async fn rebuild_story_index(
        &self,
        input: RebuildStoryIndexRequest,
    ) -> AppResult<Vec<StoryIndexSummary>> {
        story_index::rebuild_story_index(&self.state, input).await
    }

    pub async fn rebuild_story_search_index(
        &self,
        input: RebuildStorySearchIndexRequest,
    ) -> AppResult<StorySearchStatus> {
        story_search::rebuild_story_search_index(&self.state, input).await
    }

    pub fn get_story_search_status(&self, project_id: i64) -> AppResult<StorySearchStatus> {
        story_search::get_story_search_status(&self.state, project_id)
    }

    pub async fn request_revision(&self, input: RevisionRequest) -> AppResult<AgentStepResult> {
        workflow::request_revision(&self.state, input).await
    }

    pub fn replace_artifact_span(
        &self,
        input: SpanReplacementRequest,
    ) -> AppResult<AgentStepResult> {
        workflow::replace_artifact_span(&self.state, input)
    }

    pub async fn revise_artifact_span_with_ai(
        &self,
        input: AiSpanRevisionRequest,
    ) -> AppResult<AgentStepResult> {
        workflow::revise_artifact_span_with_ai(&self.state, input).await
    }

    pub fn delete_artifact(&self, input: DeleteArtifactRequest) -> AppResult<()> {
        self.state
            .delete_artifact(input.project_id, input.artifact_id)
    }

    pub fn clear_chapter_history(
        &self,
        input: ClearChapterHistoryRequest,
    ) -> AppResult<HistoryCleanupResult> {
        self.state.clear_chapter_history(
            input.project_id,
            input.chapter_id,
            input.keep_artifact_ids.as_deref().unwrap_or(&[]),
        )
    }

    pub fn list_artifacts(&self, filters: ArtifactFilters) -> AppResult<Vec<Artifact>> {
        self.state.list_artifacts(filters)
    }

    pub fn export_project(&self, project_id: i64, format: &str) -> AppResult<String> {
        match format {
            "markdown" | "md" => workflow::export_markdown(&self.state, project_id),
            _ => Err(AppError::Validation(
                "第一版只支持 Markdown 导出".to_string(),
            )),
        }
    }

    pub fn analyze_artifact_quality(
        &self,
        project_id: i64,
        artifact_id: i64,
    ) -> AppResult<QualityReport> {
        let artifact = self.get_artifact(project_id, artifact_id)?;
        Ok(quality::analyze_artifact(&artifact))
    }

    pub async fn review_project_continuity(
        &self,
        input: ContinuityReviewRequest,
    ) -> AppResult<ContinuityReport> {
        workflow::review_project_continuity(&self.state, input).await
    }

    pub async fn check_artifact_ledger_continuity(
        &self,
        input: LedgerContinuityCheckRequest,
    ) -> AppResult<LedgerContinuityReport> {
        continuity_ledger::check_artifact_continuity(&self.state, input).await
    }

    pub async fn analyze_chapter_gate(
        &self,
        input: ChapterGateRequest,
    ) -> AppResult<ChapterGateReport> {
        gate::analyze_chapter_gate(&self.state, input).await
    }

    pub async fn generate_chapter_split_plan(
        &self,
        input: ChapterSplitPlanRequest,
    ) -> AppResult<ChapterSplitPlan> {
        workflow::generate_chapter_split_plan(&self.state, input).await
    }

    pub fn search_story_context(
        &self,
        input: StoryContextSearchInput,
    ) -> AppResult<Vec<StoryContextSnippet>> {
        workflow::search_story_context(&self.state, input)
    }

    pub async fn rerank_story_context(
        &self,
        input: StoryContextRerankRequest,
    ) -> AppResult<StoryContextRerankResult> {
        context_search::rerank_story_context(&self.state, input).await
    }
}
