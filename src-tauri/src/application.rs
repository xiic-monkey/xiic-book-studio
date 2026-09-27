use crate::{
    agent_run_service,
    db::AppState,
    error::{AppError, AppResult},
    index_jobs,
    models::{
        ActionProposal, ActiveAgentRun, Agent, AgentRunRequest, AgentRunSummary, Artifact,
        ArtifactFilters, ArtifactSummary, ConfirmCurrentPlanRequest, CurrentPlanConfirmationResult,
        DecideActionProposalRequest, DerivedIndexJob, ListActionProposalsRequest,
        OrchestratorTurnRequest, OrchestratorTurnResponse, PreparedContext, ProjectWorkspace,
        ProposalApplyResult, ProviderCapabilities, RevisionRequest, RunEvent,
        RunStoryArchitectRequest,
    },
    story_architecture,
};

mod use_cases;

#[derive(Clone)]
pub struct ApplicationGateway {
    state: AppState,
}

impl ApplicationGateway {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn start_background_workers(&self) {
        self.state.start_index_worker();
    }

    pub fn subscribe_run_events(&self) -> tokio::sync::broadcast::Receiver<RunEvent> {
        self.state.subscribe_run_events()
    }

    pub async fn preview_agent_run(&self, input: AgentRunRequest) -> AppResult<PreparedContext> {
        agent_run_service::preview_agent_run(&self.state, input).await
    }

    pub async fn start_agent_run(&self, input: AgentRunRequest) -> AppResult<AgentRunSummary> {
        agent_run_service::start_agent_run(&self.state, input).await
    }

    pub async fn start_orchestrator_turn(
        &self,
        input: OrchestratorTurnRequest,
    ) -> AppResult<OrchestratorTurnResponse> {
        agent_run_service::start_orchestrator_turn(&self.state, input).await
    }

    pub async fn start_story_architect_run(
        &self,
        input: RunStoryArchitectRequest,
    ) -> AppResult<AgentRunSummary> {
        agent_run_service::start_story_architect_run(&self.state, input).await
    }

    pub async fn confirm_current_plan(
        &self,
        input: ConfirmCurrentPlanRequest,
    ) -> AppResult<CurrentPlanConfirmationResult> {
        story_architecture::confirm_current_plan(&self.state, input).await
    }

    pub async fn start_revision_run(&self, input: RevisionRequest) -> AppResult<AgentRunSummary> {
        agent_run_service::start_revision_run(&self.state, input).await
    }

    pub fn cancel_agent_run(&self, run_id: i64) -> AppResult<AgentRunSummary> {
        agent_run_service::cancel_agent_run(&self.state, run_id)
    }

    pub fn get_agent_run(&self, run_id: i64) -> AppResult<AgentRunSummary> {
        agent_run_service::get_agent_run(&self.state, run_id)
    }

    pub fn list_run_events(&self, run_id: i64, after_sequence: i64) -> AppResult<Vec<RunEvent>> {
        self.state.list_run_events(run_id, after_sequence)
    }

    pub fn get_active_agent_run(&self, project_id: i64) -> AppResult<Option<ActiveAgentRun>> {
        self.state.get_active_agent_run(project_id)
    }

    pub fn list_artifact_summaries(
        &self,
        filters: ArtifactFilters,
    ) -> AppResult<Vec<ArtifactSummary>> {
        self.state.list_artifact_summaries(filters)
    }

    pub fn get_project_workspace(&self, project_id: i64) -> AppResult<ProjectWorkspace> {
        let project = self.state.get_project(project_id)?;
        Ok(ProjectWorkspace {
            project,
            genre_agent: self.state.get_genre_agent_for_project(project_id)?,
            chapters: self.state.list_chapters(project_id)?,
            chapter_plans: self.state.list_chapter_plans(project_id)?,
            formal_char_count: self.state.formal_char_count(project_id)?,
            artifacts: self.state.list_artifact_summaries(ArtifactFilters {
                project_id,
                stage: None,
                chapter_id: None,
            })?,
            approvals: self.state.list_approvals(project_id)?,
            messages: self.state.list_messages(project_id)?,
            workflow_runs: self.state.list_workflow_run_summaries(project_id)?,
            story_threads: self.state.list_story_threads(project_id)?,
            canon_entries: self.state.list_canon_entries(project_id)?,
            foreshadowings: self.state.list_foreshadowings(project_id)?,
            story_entities: self.state.list_story_entities(project_id)?,
            story_events: self.state.list_story_events(project_id)?,
            story_event_participants: self.state.list_story_event_participants(project_id)?,
            story_facts: self.state.list_story_facts(project_id)?,
            story_index_sources: self.state.list_story_index_sources(project_id)?,
            story_search_sources: self.state.list_story_search_sources(project_id)?,
            index_jobs: self.state.list_derived_index_jobs(project_id)?,
            adoption_proposals: self.state.list_adoption_proposals(project_id, None)?,
            story_bible: self.state.get_story_bible(project_id)?,
            story_arcs: self.state.list_story_arcs(project_id)?,
            story_bible_review: self.state.latest_story_bible_review(project_id)?,
            canonical_fingerprint: crate::story_architecture::canonical_fingerprint(
                &self.state,
                project_id,
            )?,
            settings: self.state.get_ai_settings()?,
        })
    }

    pub fn get_artifact(&self, project_id: i64, artifact_id: i64) -> AppResult<Artifact> {
        let artifact = self.state.get_artifact(artifact_id)?;
        if artifact.project_id != project_id {
            return Err(AppError::Validation("产物不属于当前项目".to_string()));
        }
        Ok(artifact)
    }

    pub fn list_index_jobs(&self, project_id: i64) -> AppResult<Vec<DerivedIndexJob>> {
        self.state.list_derived_index_jobs(project_id)
    }

    pub fn get_provider_capabilities(
        &self,
        provider_base_url: &str,
    ) -> AppResult<ProviderCapabilities> {
        self.state.provider_capabilities(provider_base_url)
    }

    pub fn list_action_proposals(
        &self,
        input: ListActionProposalsRequest,
    ) -> AppResult<Vec<ActionProposal>> {
        self.state
            .list_action_proposals(input.project_id, input.status.as_deref())
    }

    pub fn apply_action_proposal(
        &self,
        input: DecideActionProposalRequest,
    ) -> AppResult<ProposalApplyResult> {
        let result =
            self.state
                .apply_action_proposal(input.project_id, input.proposal_id, &input.note)?;
        if matches!(
            result.proposal.proposal_type.as_str(),
            "canon_entry" | "canon_entry_update" | "canon_entry_delete"
        ) {
            if let Err(error) =
                index_jobs::enqueue_project_search_job(&self.state, input.project_id)
            {
                eprintln!("knowledge card search refresh unavailable: {error}");
            }
        }
        Ok(result)
    }

    pub fn reject_action_proposal(
        &self,
        input: DecideActionProposalRequest,
    ) -> AppResult<ActionProposal> {
        self.state
            .reject_action_proposal(input.project_id, input.proposal_id, &input.note)
    }

    pub fn reset_agent_prompt(&self, agent_id: i64) -> AppResult<Agent> {
        crate::prompt_templates::reset_agent_prompt(&self.state, agent_id)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;
    use crate::{index_jobs, models::NewProject};

    /// 门禁硬阻断：正文质量不过关时审批必须被拒绝，且不得污染章节指针。
    ///
    /// 用的是第一章，跨章连续性评审会被跳过（见 `gate.rs`），因此本用例
    /// 不依赖任何 AI 调用即可端到端验证"门禁确实挡在审批前面"。
    #[tokio::test]
    async fn body_approval_is_blocked_when_quality_gate_fails() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "门禁拦截".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let artifact = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "带标题的劣质正文",
                "# 第1章 开端\n\n他拿到了赤髓。",
                None,
            )
            .unwrap();

        let gateway = ApplicationGateway::new(state.clone());
        let error = gateway
            .approve_stage(project.id, "draft", artifact.id, None)
            .await
            .expect_err("质量门禁未通过时审批必须被拒绝");
        assert!(
            error.to_string().contains("质量门禁未通过"),
            "实际错误信息：{error}"
        );

        let stored = state.get_artifact(artifact.id).unwrap();
        assert_ne!(stored.status, "approved", "被拦下的产物不应被标记为已通过");
        assert!(
            state.list_chapters(project.id).unwrap()[0]
                .current_artifact_id
                .is_none(),
            "被拦下的产物不应成为章节正式稿"
        );
    }

    /// 门禁作用域：只拦正文阶段的转正，项目级资料阶段照常审批。
    ///
    /// 此处刻意复用上一用例里"含 Markdown 标题"的劣质内容——同样的内容
    /// 在 `setting` 阶段必须放行，证明拦截是"按阶段"而不是"按内容"。
    #[tokio::test]
    async fn foundation_approval_skips_the_body_quality_gate() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "资料审批".to_string(),
                genre: "玄幻".to_string(),
                target_words: 100_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let artifact = state
            .insert_artifact(
                project.id,
                None,
                "setting",
                "世界观",
                "# 第1章 开端\n\n他拿到了赤髓。",
                None,
            )
            .unwrap();

        let gateway = ApplicationGateway::new(state.clone());
        let approval = gateway
            .approve_stage(project.id, "setting", artifact.id, None)
            .await
            .expect("项目级资料阶段不应被正文门禁拦截");
        assert_eq!(approval.artifact_id, artifact.id);
    }

    #[test]
    fn applying_a_canon_entry_proposal_queues_search_rebuild() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "资料卡索引测试".to_string(),
                genre: "测试".to_string(),
                target_words: 10_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        assert!(state
            .list_derived_index_jobs(project.id)
            .unwrap()
            .iter()
            .all(|job| job.job_type != index_jobs::SEARCH_PROJECT_JOB));
        let proposal = state
            .create_action_proposal(
                project.id,
                None,
                None,
                "canon_entry",
                "创建资料卡",
                &serde_json::json!({
                    "category": "world",
                    "title": "新资料",
                    "content": "等待建立检索索引"
                }),
                None,
            )
            .unwrap();

        ApplicationGateway::new(state.clone())
            .apply_action_proposal(DecideActionProposalRequest {
                project_id: project.id,
                proposal_id: proposal.id,
                note: "确认".to_string(),
            })
            .unwrap();

        assert!(state
            .list_derived_index_jobs(project.id)
            .unwrap()
            .iter()
            .any(|job| job.job_type == index_jobs::SEARCH_PROJECT_JOB));
    }

    #[test]
    fn project_workspace_omits_large_bodies_and_run_prompts() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "大型项目".to_string(),
                genre: "测试".to_string(),
                target_words: 1_000_000,
                premise: "验证工作区摘要大小".to_string(),
            })
            .unwrap();
        state
            .with_conn(|conn| {
                let timestamp = "2026-08-09T00:00:00Z";
                for chapter_no in 2..=100_i64 {
                    conn.execute(
                        "INSERT INTO chapters
                         (project_id, chapter_no, title, status, created_at, updated_at)
                         VALUES (?1, ?2, ?3, 'planning', ?4, ?4)",
                        params![
                            project.id,
                            chapter_no,
                            format!("第{chapter_no}章"),
                            timestamp
                        ],
                    )?;
                }
                let chapters = state.list_chapters(project.id)?;
                for artifact_no in 0..1000_i64 {
                    let chapter = &chapters[(artifact_no as usize) % chapters.len()];
                    let version = artifact_no / chapters.len() as i64 + 1;
                    conn.execute(
                        "INSERT INTO artifacts
                         (project_id, chapter_id, stage, title, content, version, status,
                          parent_artifact_id, created_at)
                         VALUES (?1, ?2, 'draft', ?3, ?4, ?5, 'pending', NULL, ?6)",
                        params![
                            project.id,
                            chapter.id,
                            format!("v{version}"),
                            "ARTIFACT_BODY_SENTINEL".repeat(500),
                            version,
                            timestamp
                        ],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        state
            .insert_workflow_run(
                project.id,
                None,
                "draft",
                &"WORKFLOW_PROMPT_SENTINEL".repeat(500),
                &"WORKFLOW_OUTPUT_SENTINEL".repeat(500),
                "success",
                None,
                10,
            )
            .unwrap();

        let workspace = ApplicationGateway::new(state)
            .get_project_workspace(project.id)
            .unwrap();
        let payload = serde_json::to_vec(&workspace).unwrap();
        let text = String::from_utf8(payload.clone()).unwrap();
        assert_eq!(workspace.chapters.len(), 100);
        assert_eq!(workspace.artifacts.len(), 1000);
        assert!(
            payload.len() < 250 * 1024,
            "workspace payload was {} bytes",
            payload.len()
        );
        assert!(!text.contains("ARTIFACT_BODY_SENTINEL"));
        assert!(!text.contains("WORKFLOW_PROMPT_SENTINEL"));
        assert!(!text.contains("WORKFLOW_OUTPUT_SENTINEL"));
    }

    #[test]
    fn formal_char_count_uses_current_approved_artifacts_only() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "字数统计".into(),
                genre: "测试".into(),
                target_words: 10_000,
                premise: "统计".into(),
            })
            .unwrap();
        state.with_conn(|conn| {
            for chapter_no in 2..=3_i64 {
                conn.execute(
                    "INSERT INTO chapters (project_id, chapter_no, title, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'writing', 't', 't')",
                    params![project.id, chapter_no, format!("第{chapter_no}章")],
                )?;
            }
            let chapters = state.list_chapters(project.id)?;
            conn.execute(
                "INSERT INTO artifacts (project_id, chapter_id, stage, title, content, version, status, created_at)
                 VALUES (?1, ?2, 'draft', '正式', '甲 乙\n丙', 1, 'approved', 't')",
                params![project.id, chapters[0].id],
            )?;
            let approved_id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO artifacts (project_id, chapter_id, stage, title, content, version, status, created_at)
                 VALUES (?1, ?2, 'draft', '候选', '不应计入', 1, 'pending_review', 't')",
                params![project.id, chapters[1].id],
            )?;
            let pending_id = conn.last_insert_rowid();
            conn.execute("UPDATE chapters SET current_artifact_id = ?1 WHERE id = ?2", params![approved_id, chapters[0].id])?;
            conn.execute("UPDATE chapters SET current_artifact_id = ?1 WHERE id = ?2", params![pending_id, chapters[1].id])?;
            Ok(())
        }).unwrap();
        assert_eq!(state.formal_char_count(project.id).unwrap(), 3);
    }
}
