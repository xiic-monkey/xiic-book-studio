use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    db::AppState,
    error::{AppError, AppResult},
    models::{
        AgentRunRequest, AgentRunSummary, Artifact, ContextSegment, PreparedContext,
        RevisionRequest, RunAgentRequest, RunStoryArchitectRequest, Stage, WorkflowRun,
    },
    tool_runtime::{self, ToolExecutionContext},
    workflow,
};

pub async fn preview_agent_run(
    state: &AppState,
    request: AgentRunRequest,
) -> AppResult<PreparedContext> {
    prepare_context(state, &request, true).await
}

pub async fn start_agent_run(
    state: &AppState,
    mut request: AgentRunRequest,
) -> AppResult<AgentRunSummary> {
    let prepared = if let Some(id) = request.prepared_context_id {
        let prepared = state.get_prepared_context(id)?;
        validate_prepared_context(state, &request, &prepared)?;
        prepared
    } else {
        prepare_context(state, &request, true).await?
    };
    request.prepared_context_id = Some(prepared.id);
    if state.get_active_agent_run(request.project_id)?.is_some() {
        return Err(AppError::Validation(
            "当前项目已有 Agent 任务正在运行".to_string(),
        ));
    }
    let run = state.insert_workflow_run(
        request.project_id,
        request.chapter_id,
        request.stage.as_str(),
        &prepared.prompt,
        "",
        "running",
        None,
        0,
    )?;
    state.link_run_prepared_context(run.id, prepared.id)?;
    state.insert_run_event(
        run.id,
        request.project_id,
        request.chapter_id,
        "started",
        "",
        "running",
        None,
    )?;

    let worker_state = state.clone();
    let worker_request = RunAgentRequest::from(request.clone());
    let worker_run = run.clone();
    tokio::spawn(async move {
        match workflow::run_agent_step_from_run(
            &worker_state,
            worker_request.clone(),
            worker_run.clone(),
        )
        .await
        {
            Ok(result) => {
                let proposals_error =
                    prepare_proposals_after_run(&worker_state, &worker_request, &result)
                        .await
                        .err();
                if let Some(error) = proposals_error {
                    if let Err(event_error) = worker_state.insert_run_event(
                        result.run.id,
                        worker_request.project_id,
                        worker_request.chapter_id,
                        "proposal_warning",
                        "",
                        "success",
                        Some(&error.to_string()),
                    ) {
                        eprintln!("记录 proposal_warning 运行事件失败: {event_error}");
                    }
                }
                if let Err(event_error) = worker_state.insert_run_event(
                    result.run.id,
                    worker_request.project_id,
                    worker_request.chapter_id,
                    "completed",
                    "",
                    "success",
                    None,
                ) {
                    eprintln!("记录 completed 运行事件失败: {event_error}");
                }
            }
            Err(error) => {
                // The workflow normally persists and broadcasts terminal events itself.  Some
                // failures happen before it has entered its streaming section (for example a
                // missing approved prerequisite), so finalize the run here as well; otherwise
                // the UI would leave a run stuck in `running` forever.
                let message = error.to_string();
                let cancelled = worker_state
                    .run_cancellation_requested(worker_run.id)
                    .unwrap_or(false);
                let final_status = if cancelled { "cancelled" } else { "failed" };
                if let Ok(current) = worker_state.get_workflow_run_v2(worker_run.id) {
                    if matches!(
                        current.status.as_str(),
                        "running" | "streaming" | "cancellation_requested"
                    ) {
                        let _ = worker_state.update_workflow_run(
                            worker_run.id,
                            &current.output,
                            final_status,
                            Some(&message),
                            current.elapsed_ms,
                        );
                        let _ = worker_state.insert_run_event(
                            worker_run.id,
                            worker_request.project_id,
                            worker_request.chapter_id,
                            final_status,
                            "",
                            final_status,
                            Some(&message),
                        );
                    }
                }
                eprintln!("Agent run {} ended with an error: {error}", worker_run.id);
            }
        }
    });

    let tool_invocations = tool_invocations_for_run(state, run.id, Some(prepared.id))?;
    Ok(AgentRunSummary {
        run,
        artifact: None,
        prepared_context_id: Some(prepared.id),
        tool_invocations,
        proposals: Vec::new(),
    })
}

async fn prepare_proposals_after_run(
    state: &AppState,
    request: &RunAgentRequest,
    result: &crate::models::AgentStepResult,
) -> AppResult<()> {
    let agent = state.get_agent_for_project_stage(request.project_id, request.stage.as_str())?;
    let mut proposal_agent = agent.clone();
    let foundation_mode = matches!(
        request.stage,
        Stage::Setting | Stage::Outline | Stage::Characters
    );
    proposal_agent.enabled_tool_keys.retain(|key| {
        crate::agent_tools::get(key).is_some_and(|definition| {
            definition.kind == crate::models::ToolKind::Proposal
                || (foundation_mode
                    && matches!(
                        key.as_str(),
                        crate::agent_tools::PROPOSE_KNOWLEDGE_CARD
                            | crate::agent_tools::PROPOSE_UPDATE_KNOWLEDGE_CARD
                    ))
        })
    });
    if proposal_agent.enabled_tool_keys.is_empty() {
        return Ok(());
    }
    let foundation_mode = matches!(
        request.stage,
        Stage::Setting | Stage::Outline | Stage::Characters
    );
    let proposal_prompt = if foundation_mode {
        format!(
            "# 已完成的 Agent 产物\n阶段：{}\n标题：{}\n\n{}\n\n# 人工原始指令\n{}\n\n你现在是结构化资料沉淀子流程。不要输出 Markdown，也不要创建资料候选版本。请逐条调用提议知识卡工具，把产物中的世界观、规则、地点、势力、物件、角色或大纲任务创建/更新为独立知识卡。每张卡只表达一个稳定知识单元；已有卡片应优先更新而不是重复创建。所有卡片保持待人工确认状态。完成后停止工具调用。",
            request.stage.as_str(),
            result.artifact.title,
            result.artifact.content,
            request.user_instruction.as_deref().unwrap_or("未提供")
        )
    } else {
        format!(
            "# 已完成的 Agent 产物\n阶段：{}\n标题：{}\n\n{}\n\n# 人工原始指令\n{}\n\n只在产物明确需要创建章节、重命名章节、生成资料候选、更新或删除知识卡、生成伏笔候选时创建写入提案。不得提议删除章节或正文、批准或直接应用正文。",
            request.stage.as_str(),
            result.artifact.title,
            result.artifact.content,
            request.user_instruction.as_deref().unwrap_or("未提供")
        )
    };
    tool_runtime::prepare_tools(
        ToolExecutionContext {
            state,
            agent: &proposal_agent,
            project_id: request.project_id,
            chapter_id: request.chapter_id,
            stage: &request.stage,
            source_artifact_id: Some(result.artifact.id),
            user_instruction: request.user_instruction.as_deref(),
            reference_selection: request.reference_selection.as_ref(),
            run_id: Some(result.run.id),
            preview: false,
        },
        &proposal_prompt,
    )
    .await?;
    Ok(())
}

pub async fn start_story_architect_run(
    state: &AppState,
    request: RunStoryArchitectRequest,
) -> AppResult<AgentRunSummary> {
    let request = crate::story_architecture::build_agent_run_request(state, request)?;
    start_agent_run(state, request).await
}

pub async fn start_revision_run(
    state: &AppState,
    request: RevisionRequest,
) -> AppResult<AgentRunSummary> {
    let source = state.get_artifact(request.artifact_id)?;
    if source.project_id != request.project_id {
        return Err(AppError::Validation("修订目标不属于当前项目".to_string()));
    }
    if !matches!(source.stage.as_str(), "draft" | "revision" | "review") {
        return Err(AppError::Validation(
            "只能对章节草稿、试读报告或修订稿发起修订".to_string(),
        ));
    }
    if request.feedback.trim().is_empty() {
        return Err(AppError::Validation("请填写修订反馈".to_string()));
    }
    start_agent_run(
        state,
        AgentRunRequest {
            project_id: request.project_id,
            stage: Stage::Revision,
            chapter_id: source.chapter_id,
            user_instruction: Some(request.feedback),
            source_artifact_id: Some(source.id),
            reference_selection: request.reference_selection,
            prepared_context_id: None,
        },
    )
    .await
}

pub fn get_agent_run(state: &AppState, run_id: i64) -> AppResult<AgentRunSummary> {
    let run = workflow_run(state, run_id)?;
    let prepared_context_id = state.prepared_context_id_for_run(run_id)?;
    let artifact = state
        .artifact_id_for_run(run_id)?
        .map(|artifact_id| state.get_artifact(artifact_id))
        .transpose()?;
    let proposals = state
        .list_action_proposals(run.project_id, None)?
        .into_iter()
        .filter(|proposal| proposal.source_run_id == Some(run_id))
        .collect();
    Ok(AgentRunSummary {
        run,
        artifact,
        prepared_context_id,
        tool_invocations: tool_invocations_for_run(state, run_id, prepared_context_id)?,
        proposals,
    })
}

pub fn cancel_agent_run(state: &AppState, run_id: i64) -> AppResult<AgentRunSummary> {
    let run = workflow_run(state, run_id)?;
    if !matches!(run.status.as_str(), "streaming" | "running") {
        return Err(AppError::Validation(
            "只有正在运行的 Agent 任务可以取消".to_string(),
        ));
    }
    let updated = state.update_workflow_run(
        run_id,
        &run.output,
        "cancellation_requested",
        Some("用户请求取消"),
        run.elapsed_ms,
    )?;
    state.insert_run_event(
        run_id,
        run.project_id,
        run.chapter_id,
        "cancellation_requested",
        "",
        "cancellation_requested",
        None,
    )?;
    let prepared_context_id = state.prepared_context_id_for_run(run_id)?;
    Ok(AgentRunSummary {
        run: updated,
        artifact: None,
        prepared_context_id,
        tool_invocations: tool_invocations_for_run(state, run_id, prepared_context_id)?,
        proposals: state
            .list_action_proposals(run.project_id, None)?
            .into_iter()
            .filter(|proposal| proposal.source_run_id == Some(run_id))
            .collect(),
    })
}

async fn prepare_context(
    state: &AppState,
    request: &AgentRunRequest,
    preview: bool,
) -> AppResult<PreparedContext> {
    state.purge_expired_prepared_contexts()?;
    let source = validate_request(state, request)?;
    let agent = state.get_agent_for_project_stage(request.project_id, request.stage.as_str())?;
    let mut prompt = workflow::build_prompt_for_agent(
        state,
        request.project_id,
        &request.stage,
        request.chapter_id,
        request.user_instruction.as_deref(),
        source.as_ref(),
        None,
        &agent,
    )?;
    let preparation = tool_runtime::prepare_tools(
        ToolExecutionContext {
            state,
            agent: &agent,
            project_id: request.project_id,
            chapter_id: request.chapter_id,
            stage: &request.stage,
            source_artifact_id: request.source_artifact_id,
            user_instruction: request.user_instruction.as_deref(),
            reference_selection: request.reference_selection.as_ref(),
            run_id: None,
            preview,
        },
        &prompt,
    )
    .await?;
    if let Some(tool_context) = preparation.rendered_context.as_deref() {
        prompt.push_str("\n\n");
        prompt.push_str(tool_context);
    }
    let fingerprint = context_fingerprint(state, request, &agent, source.as_ref())?;
    let segments = split_segments(&prompt);
    state.insert_prepared_context(
        request.project_id,
        request.chapter_id,
        request.stage.as_str(),
        &fingerprint,
        &agent.system_prompt,
        &prompt,
        &segments,
        &preparation.invocation_ids,
    )
}

fn validate_request(state: &AppState, request: &AgentRunRequest) -> AppResult<Option<Artifact>> {
    state.get_project(request.project_id)?;
    match request.stage {
        Stage::Setting | Stage::Outline | Stage::Characters if request.chapter_id.is_some() => {
            return Err(AppError::Validation(
                "设定、大纲和角色阶段不能绑定章节".to_string(),
            ));
        }
        Stage::Draft | Stage::Review | Stage::Revision if request.chapter_id.is_none() => {
            return Err(AppError::Validation(
                "写作、试读和修订阶段必须选择章节".to_string(),
            ));
        }
        _ => {}
    }
    if request.chapter_id.is_some() {
        state
            .ensure_chapter(request.project_id, request.chapter_id)?
            .ok_or_else(|| AppError::Validation("章节不属于当前项目".to_string()))?;
    }
    let source = request
        .source_artifact_id
        .map(|id| state.get_artifact(id))
        .transpose()?;
    if source
        .as_ref()
        .is_some_and(|artifact| artifact.project_id != request.project_id)
    {
        return Err(AppError::Validation("候选产物不属于当前项目".to_string()));
    }
    if let Some(source) = source.as_ref() {
        let valid_source = match request.stage {
            Stage::Setting | Stage::Outline | Stage::Characters => {
                source.chapter_id.is_none() && source.stage == request.stage.as_str()
            }
            Stage::Review => {
                source.chapter_id == request.chapter_id
                    && matches!(source.stage.as_str(), "draft" | "revision")
            }
            Stage::Revision => {
                source.chapter_id == request.chapter_id
                    && matches!(source.stage.as_str(), "draft" | "revision" | "review")
            }
            Stage::Draft => false,
        };
        if !valid_source {
            return Err(AppError::Validation(
                "当前阶段不支持把这个产物作为上下文来源".to_string(),
            ));
        }
    }
    Ok(source)
}

fn validate_prepared_context(
    state: &AppState,
    request: &AgentRunRequest,
    prepared: &PreparedContext,
) -> AppResult<()> {
    if prepared.project_id != request.project_id
        || prepared.chapter_id != request.chapter_id
        || prepared.stage != request.stage.as_str()
    {
        return Err(AppError::Validation(
            "准备上下文与当前请求不匹配".to_string(),
        ));
    }
    let expires_at = DateTime::parse_from_rfc3339(&prepared.expires_at)
        .map_err(|_| AppError::Validation("准备上下文过期时间损坏".to_string()))?;
    if expires_at <= Utc::now() {
        return Err(AppError::Validation(
            "准备上下文已过期，请重新预览".to_string(),
        ));
    }
    let source = validate_request(state, request)?;
    let agent = state.get_agent_for_project_stage(request.project_id, request.stage.as_str())?;
    let current = context_fingerprint(state, request, &agent, source.as_ref())?;
    if current != prepared.fingerprint {
        return Err(AppError::Validation(
            "项目资料、章节、候选稿、Prompt 或工具配置已变化，请重新预览".to_string(),
        ));
    }
    Ok(())
}

fn context_fingerprint(
    state: &AppState,
    request: &AgentRunRequest,
    agent: &crate::models::Agent,
    source: Option<&Artifact>,
) -> AppResult<String> {
    #[derive(Serialize)]
    struct Fingerprint<'a> {
        project_updated_at: &'a str,
        canonical_fingerprint: String,
        chapter_id: Option<i64>,
        chapter_updated_at: Option<String>,
        stage: &'a str,
        source_id: Option<i64>,
        source_hash: Option<String>,
        instruction: Option<&'a str>,
        reference_selection: &'a Option<crate::models::ReferenceSelection>,
        agent_id: i64,
        system_prompt: &'a str,
        enabled_tool_keys: &'a [String],
        allowed_skill_keys: &'a [String],
        provider_base_url: &'a str,
        model: &'a str,
        tool_protocol: &'a str,
        reference_fingerprint: String,
    }
    let project = state.get_project(request.project_id)?;
    let chapter_updated_at = request.chapter_id.and_then(|id| {
        state
            .ensure_chapter(request.project_id, Some(id))
            .ok()
            .flatten()
            .map(|chapter| chapter.updated_at)
    });
    let value = Fingerprint {
        project_updated_at: &project.updated_at,
        canonical_fingerprint: crate::story_architecture::canonical_fingerprint(
            state,
            request.project_id,
        )?,
        chapter_id: request.chapter_id,
        chapter_updated_at,
        stage: request.stage.as_str(),
        source_id: source.map(|artifact| artifact.id),
        source_hash: source.map(|artifact| chapter_memory_hash(&artifact.content)),
        instruction: request.user_instruction.as_deref(),
        reference_selection: &request.reference_selection,
        agent_id: agent.id,
        system_prompt: &agent.system_prompt,
        enabled_tool_keys: &agent.enabled_tool_keys,
        allowed_skill_keys: &agent.allowed_skill_keys,
        provider_base_url: &agent.provider_base_url,
        model: &agent.model,
        tool_protocol: state
            .provider_capabilities(&agent.provider_base_url)?
            .configured_protocol
            .as_str(),
        reference_fingerprint: crate::reference::selection_fingerprint(
            state,
            request.project_id,
            request.reference_selection.as_ref(),
        )?,
    };
    let encoded = serde_json::to_vec(&value)?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn chapter_memory_hash(content: &str) -> String {
    crate::chapter_memory::source_text_hash(content)
}

fn split_segments(prompt: &str) -> Vec<ContextSegment> {
    const MAX_PREVIEW_CHARS: usize = 2_400;
    let mut segments: Vec<(String, String)> = Vec::new();
    for line in prompt.lines() {
        if line.starts_with("# ") {
            segments.push((
                line.trim_start_matches("# ").trim().to_string(),
                String::new(),
            ));
        } else if let Some((_, content)) = segments.last_mut() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(line);
        }
    }
    if segments.is_empty() {
        segments.push(("生成上下文".to_string(), prompt.to_string()));
    }
    segments
        .into_iter()
        .filter(|(_, content)| !content.trim().is_empty())
        .map(|(label, content)| {
            let chars = content.chars().count();
            let truncated = chars > MAX_PREVIEW_CHARS;
            let preview = if truncated {
                content.chars().take(MAX_PREVIEW_CHARS).collect::<String>()
                    + "\n…（预览已截断，正式运行使用完整内容）"
            } else {
                content
            };
            ContextSegment {
                kind: segment_kind(&label).to_string(),
                label,
                source: "application_context_pipeline".to_string(),
                content: preview,
                chars,
                truncated,
            }
        })
        .collect()
}

fn segment_kind(label: &str) -> &'static str {
    if label.contains("工具") || label.contains("检索") || label.contains("账本") {
        "tool_result"
    } else if label.contains("人工") {
        "human_instruction"
    } else if label.contains("任务") || label.contains("输出") {
        "task"
    } else {
        "static_context"
    }
}

fn workflow_run(state: &AppState, run_id: i64) -> AppResult<WorkflowRun> {
    state.get_workflow_run_v2(run_id)
}

fn tool_invocations_for_run(
    state: &AppState,
    run_id: i64,
    prepared_context_id: Option<i64>,
) -> AppResult<Vec<crate::models::ToolInvocation>> {
    let mut invocations = if let Some(prepared_context_id) = prepared_context_id {
        state.list_tool_invocations_for_context(prepared_context_id)?
    } else {
        Vec::new()
    };
    invocations.extend(state.list_tool_invocations_for_run(run_id)?);
    invocations.sort_by_key(|invocation| invocation.id);
    Ok(invocations)
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, time::Duration};

    use axum::{
        extract::State,
        response::{
            sse::{Event, Sse},
            IntoResponse, Response,
        },
        routing::post,
        Json, Router,
    };
    use serde_json::{json, Value};
    use tempfile::TempDir;
    use tokio::{net::TcpListener, sync::mpsc, time::sleep};
    use tokio_stream::wrappers::ReceiverStream;

    use super::*;
    use crate::models::{NewProject, SaveAgentSettings, SaveAiSettings, Stage};

    #[derive(Clone)]
    struct MockAiState {
        chunk_delay: Duration,
    }

    async fn mock_chat_completions(
        State(state): State<MockAiState>,
        Json(request): Json<Value>,
    ) -> Response {
        if request.get("stream").and_then(Value::as_bool) != Some(true) {
            return Json(json!({
                "choices": [{"message": {"role": "assistant", "content": "Mock completion"}}]
            }))
            .into_response();
        }

        let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(4);
        tokio::spawn(async move {
            for delta in ["Mock ", "streamed setting"] {
                if sender
                    .send(Ok(Event::default().data(
                        json!({
                            "choices": [{"delta": {"content": delta}}]
                        })
                        .to_string(),
                    )))
                    .await
                    .is_err()
                {
                    return;
                }
                sleep(state.chunk_delay).await;
            }
            let _ = sender.send(Ok(Event::default().data("[DONE]"))).await;
        });
        Sse::new(ReceiverStream::new(receiver)).into_response()
    }

    async fn start_mock_ai_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .route("/v1/chat/completions", post(mock_chat_completions))
            .with_state(MockAiState {
                chunk_delay: Duration::from_millis(120),
            });
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{address}/v1")
    }

    fn test_state(base_url: &str) -> (TempDir, AppState, crate::models::Project) {
        let temp_dir = tempfile::tempdir().unwrap();
        let state = AppState::from_path(temp_dir.path().join("mock-agent.sqlite3")).unwrap();
        state
            .save_ai_settings(SaveAiSettings {
                base_url: base_url.to_string(),
                model: "mock-model".to_string(),
                temperature: 0.2,
                thinking_enabled: false,
                thinking_level: "off".to_string(),
                api_key: Some("mock-key".to_string()),
            })
            .unwrap();
        let architect = state.get_agent("story_architect").unwrap();
        state
            .save_agent_settings(SaveAgentSettings {
                agent_id: architect.id,
                provider_base_url: String::new(),
                model: String::new(),
                name: None,
                role: None,
                system_prompt: None,
                temperature: None,
                thinking_enabled: false,
                thinking_level: None,
                uses_global_runtime_settings: Some(true),
                enabled_tool_keys: Some(Vec::new()),
                allowed_skill_keys: Some(Vec::new()),
            })
            .unwrap();
        let project = state
            .create_project(NewProject {
                title: "Mock Agent Run".to_string(),
                genre: "奇幻".to_string(),
                target_words: 100_000,
                premise: "验证后台任务生命周期".to_string(),
            })
            .unwrap();
        (temp_dir, state, project)
    }

    fn setting_request(project_id: i64) -> AgentRunRequest {
        AgentRunRequest {
            project_id,
            stage: Stage::Setting,
            chapter_id: None,
            user_instruction: Some("请生成简短设定".to_string()),
            source_artifact_id: None,
            reference_selection: None,
            prepared_context_id: None,
        }
    }

    async fn wait_for_terminal_run(state: &AppState, run_id: i64) -> AgentRunSummary {
        for _ in 0..100 {
            let summary = get_agent_run(state, run_id).unwrap();
            if matches!(
                summary.run.status.as_str(),
                "success" | "failed" | "cancelled"
            ) {
                return summary;
            }
            sleep(Duration::from_millis(20)).await;
        }
        panic!("Agent run {run_id} did not reach a terminal status");
    }

    async fn wait_for_event(state: &AppState, run_id: i64, event_type: &str) {
        for _ in 0..100 {
            if state
                .list_run_events(run_id, 0)
                .unwrap()
                .iter()
                .any(|event| event.kind == event_type)
            {
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }
        panic!("Agent run {run_id} did not emit {event_type}");
    }

    #[test]
    fn prompt_segments_keep_heading_boundaries() {
        let segments = split_segments("# 项目\nA\n# Agent 工具执行结果\nB");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].kind, "tool_result");
    }

    #[tokio::test]
    async fn mock_streaming_ai_runs_in_background_and_can_be_cancelled() {
        let base_url = start_mock_ai_server().await;
        let (_temp_dir, state, project) = test_state(&base_url);

        let started = start_agent_run(&state, setting_request(project.id))
            .await
            .unwrap();
        assert_eq!(started.run.status, "running");
        assert!(started.artifact.is_none());
        assert!(state.get_active_agent_run(project.id).unwrap().is_some());

        let completed = wait_for_terminal_run(&state, started.run.id).await;
        assert_eq!(completed.run.status, "success");
        assert_eq!(
            completed
                .artifact
                .as_ref()
                .map(|artifact| artifact.content.as_str()),
            Some("Mock streamed setting")
        );
        let completed_events = state.list_run_events(started.run.id, 0).unwrap();
        assert!(completed_events
            .iter()
            .any(|event| event.kind == "output_delta"));
        assert!(completed_events
            .iter()
            .any(|event| event.kind == "completed"));

        let cancelling = start_agent_run(&state, setting_request(project.id))
            .await
            .unwrap();
        wait_for_event(&state, cancelling.run.id, "output_delta").await;
        let cancellation = cancel_agent_run(&state, cancelling.run.id).unwrap();
        assert_eq!(cancellation.run.status, "cancellation_requested");

        let cancelled = wait_for_terminal_run(&state, cancelling.run.id).await;
        assert_eq!(cancelled.run.status, "cancelled");
        assert!(cancelled.artifact.is_none());
        assert!(state
            .list_run_events(cancelling.run.id, 0)
            .unwrap()
            .iter()
            .any(|event| event.kind == "cancelled"));
    }
}
