use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use serde_json::Value;

mod history_search;
mod markdown_export;
mod text;
use history_search::{build_history_query, extract_history_terms, retrieve_history_snippets};
use text::{excerpt_around, is_han, is_noise_term, split_query_tokens};

pub use history_search::{search_story, search_story_context, search_story_facts};
pub use markdown_export::export_markdown;

use crate::{
    agent_tools, ai, chapter_memory, context_search, continuity_ledger,
    db::AppState,
    error::{AppError, AppResult},
    genre_skill,
    models::{
        Agent, AgentStepResult, AiSpanRevisionRequest, Artifact, ChapterSplitPlan,
        ChapterSplitPlanRequest, ContinuityIssue, ContinuityReport, ContinuityReviewRequest,
        Project, ReviewIssue, RunAgentRequest, SpanReplacementRequest, Stage,
        StoryContextSearchInput, StoryContextSnippet, WorkflowRun,
    },
    quality, story_architecture,
};

#[cfg(test)]
use crate::models::RevisionRequest;

#[derive(Debug, Clone)]
struct ChapterTaskContract {
    chapter_mode: String,
    entry_state: String,
    objective: String,
    resistance: String,
    context_focus: String,
    required_change: String,
    ending_function: String,
    next_condition: String,
    payoff: Option<String>,
    hook_carryover: Option<String>,
    must_use_threads: Vec<String>,
    must_avoid_threads: Vec<String>,
}

const REVISION_FACT_BOUNDARY: &str = "这是最高优先级的修订约束。源稿、已批准设定/大纲/角色、已批准前章、试读报告和人工指令共同构成唯一事实来源。除非其中已经明确存在，否则不得新增过去发生过的事件、隐藏物件/证据、人物、地点、组织、规则、交易习惯、制度条款、关系或角色已知信息。不要为了制造更刺激的结尾补写‘原来早有’的物件、记录、目击、计划或外部安排。需要强化结尾时，只能落稳或推进已出现的结果、决定、压力、时限、伤势、资源、关系、场景或主角已经开始的动作。若事实来源不足以支持某项建议，宁可保留原有选择或删去该建议，不得编造补丁。";
const REVIEW_FACT_BOUNDARY: &str = "审校建议也受事实边界约束：只能引用候选稿、已批准资料和已批准前章已经明确的事实。若候选稿凭空加入过去事件、隐藏物件/证据、人物、地点、组织、规则、交易习惯、制度条款、关系或角色已知信息，必须标为“事实越界”，severity 为 major，并指出它不在何处已有来源。建议只能删减、重排、强化或继续使用已有事实；不得建议编造石片、旧记录、目击、临时交易窗口、既有计划、额外物件或新规则来修问题。";

fn review_evidence_corpus(
    state: &AppState,
    project_id: i64,
    chapter_id: Option<i64>,
    source_artifact: Option<&Artifact>,
) -> AppResult<String> {
    let mut parts = Vec::new();
    if let Some(source) = source_artifact {
        parts.push(source.content.clone());
    } else if let Some(chapter_id) = chapter_id {
        if let Some(source) = state.latest_approved_chapter_body(project_id, chapter_id)? {
            parts.push(source.content);
        }
    }
    for stage in ["setting", "outline", "characters"] {
        if let Some(artifact) = state.approved_artifact(project_id, stage, None)? {
            parts.push(artifact.content);
        }
    }
    if let Some(chapter_id) = chapter_id {
        if let Some(current) = state.ensure_chapter(project_id, Some(chapter_id))? {
            for chapter in state
                .list_chapters(project_id)?
                .into_iter()
                .filter(|chapter| chapter.chapter_no < current.chapter_no)
            {
                if let Some(artifact) =
                    state.latest_approved_chapter_body(project_id, chapter.id)?
                {
                    parts.push(artifact.content);
                }
            }
        }
    }
    Ok(parts.join("\n"))
}

fn constrain_review_issues(issues: Vec<ReviewIssue>, evidence_corpus: &str) -> Vec<ReviewIssue> {
    issues
        .into_iter()
        .filter_map(|mut issue| {
            // A review issue without a source quote is not actionable. Keeping it
            // around as a warning makes the UI look authoritative while giving the
            // revision agent nothing reliable to work from.
            if !evidence_quote_is_verifiable(&issue.evidence_quote, evidence_corpus) {
                return None;
            }

            // An action quote is optional: a safe revision can delete or reorder
            // existing prose without introducing a new factual action. When the
            // model supplied an unusable one, omit it instead of discarding an
            // otherwise grounded finding.
            if !issue.action_evidence_quote.trim().is_empty()
                && !evidence_quote_is_verifiable(&issue.action_evidence_quote, evidence_corpus)
            {
                issue.action_evidence_quote.clear();
            }

            // A grounded finding can still carry an invented repair, such as
            // adding a guard or a new object that never appears in the source.
            // Keep the finding visible, but quarantine that repair before it
            // reaches the revision prompt.
            if suggestion_contains_unbound_fact(&issue.suggestion, evidence_corpus) {
                issue.suggestion =
                    "原建议包含来源中未确认的新事实，已拦截。修订只能删减、重排或强化已有动作；请人工重新指定修法。"
                        .to_string();
                issue.action_evidence_quote.clear();
            }
            Some(issue)
        })
        .collect()
}

fn model_issue_duplicates_ledger_check(issue: &ReviewIssue) -> bool {
    ["物件状态", "资源状态", "伤势状态", "禁制状态", "状态账本"]
        .iter()
        .any(|kind| issue.issue_type.contains(kind))
}

fn ledger_review_issues(
    report: Option<&crate::models::LedgerContinuityReport>,
) -> Vec<ReviewIssue> {
    report
        .into_iter()
        .flat_map(|report| &report.issues)
        .map(|issue| ReviewIssue {
            issue_type: format!("状态账本冲突/{}", issue.state_kind),
            severity: issue.severity.clone(),
            location: issue.entity_label.clone(),
            reason: format!(
                "候选稿对“{}”的使用与{}的最后已确认状态冲突。{}",
                issue.entity_label, issue.source_chapter, issue.reason
            ),
            suggestion: issue.suggestion.clone(),
            evidence_quote: bounded_evidence_quote(&issue.candidate_quote),
            action_evidence_quote: bounded_evidence_quote(&issue.source_quote),
        })
        .collect()
}

fn bounded_evidence_quote(quote: &str) -> String {
    quote.trim().chars().take(80).collect()
}

fn suggestion_contains_unbound_fact(suggestion: &str, _evidence_corpus: &str) -> bool {
    const ADDITIVE_WORDS: &[&str] = &["加入", "增加", "补充", "插入", "安排", "添加", "让", "出现"];
    const SUBSTITUTION_EXAMPLES: &[&str] = &[
        "可以是",
        "可以改成",
        "改为其他",
        "换成",
        "例如：",
        "例如添加",
        "比如加入",
    ];

    ADDITIVE_WORDS.iter().any(|word| suggestion.contains(word))
        || SUBSTITUTION_EXAMPLES
            .iter()
            .any(|pattern| suggestion.contains(pattern))
}

fn evidence_quote_is_verifiable(quote: &str, evidence_corpus: &str) -> bool {
    let normalized_quote = normalize_evidence_text(quote);
    let quote_length = normalized_quote.chars().count();
    if !(8..=80).contains(&quote_length) {
        return false;
    }
    let normalized_corpus = normalize_evidence_text(evidence_corpus);
    normalized_corpus.contains(&normalized_quote)
}

fn normalize_evidence_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| match character {
            '“' | '”' | '‘' | '’' | '"' | '\'' => ' ',
            '，' => ',',
            '。' => '.',
            '：' => ':',
            '；' => ';',
            '！' => '!',
            '？' => '?',
            '（' => '(',
            '）' => ')',
            '【' => '[',
            '】' => ']',
            '—' | '–' => '-',
            other => other,
        })
        .filter(|character| *character != ' ')
        .collect()
}

#[cfg(test)]
pub async fn run_agent_step(
    state: &AppState,
    input: RunAgentRequest,
) -> AppResult<AgentStepResult> {
    run_agent_step_impl(state, input, None, true).await
}

pub(crate) async fn run_agent_step_from_run(
    state: &AppState,
    input: RunAgentRequest,
    existing_run: WorkflowRun,
) -> AppResult<AgentStepResult> {
    run_agent_step_impl(state, input, Some(existing_run), false).await
}

async fn run_agent_step_impl(
    state: &AppState,
    input: RunAgentRequest,
    existing_run: Option<WorkflowRun>,
    emit_completion: bool,
) -> AppResult<AgentStepResult> {
    state.get_project(input.project_id)?;
    let chapter = state.ensure_chapter(input.project_id, input.chapter_id)?;
    validate_stage_scope(&input.stage, chapter.is_some())?;
    let prepared_context = input
        .prepared_context_id
        .map(|id| state.get_prepared_context(id))
        .transpose()?;
    if let Some(prepared) = prepared_context.as_ref() {
        if prepared.project_id != input.project_id
            || prepared.chapter_id != input.chapter_id
            || prepared.stage != input.stage.as_str()
        {
            return Err(AppError::Validation(
                "准备上下文与当前项目、章节或阶段不匹配".to_string(),
            ));
        }
        let expires_at = chrono::DateTime::parse_from_rfc3339(&prepared.expires_at)
            .map_err(|_| AppError::Validation("准备上下文过期时间损坏".to_string()))?;
        if expires_at <= chrono::Utc::now() {
            return Err(AppError::Validation(
                "准备上下文已过期，请重新预览".to_string(),
            ));
        }
    }
    let source_artifact = input
        .source_artifact_id
        .map(|artifact_id| state.get_artifact(artifact_id))
        .transpose()?;
    if let Some(source) = source_artifact.as_ref() {
        if source.project_id != input.project_id {
            return Err(AppError::Validation("候选稿不属于当前项目".to_string()));
        }
        let valid_source = match input.stage {
            Stage::Setting | Stage::Outline | Stage::Characters => {
                source.chapter_id.is_none() && source.stage == input.stage.as_str()
            }
            Stage::Review => {
                source.chapter_id == input.chapter_id
                    && (source.stage == "draft" || source.stage == "revision")
            }
            Stage::Revision => {
                source.chapter_id == input.chapter_id
                    && matches!(source.stage.as_str(), "draft" | "revision" | "review")
            }
            _ => false,
        };
        if !valid_source {
            return Err(AppError::Validation(
                "当前阶段只支持把同阶段资料版本或章节候选稿作为上下文来源".to_string(),
            ));
        }
    }
    validate_prerequisites(
        state,
        input.project_id,
        &input.stage,
        input.chapter_id,
        source_artifact.as_ref(),
    )?;

    if let Some(instruction) = input.user_instruction.as_deref() {
        if !instruction.trim().is_empty() {
            state.insert_message(
                input.project_id,
                input.chapter_id,
                "human_instruction",
                &format!("{}：{}", input.stage.title(), instruction.trim()),
            )?;
        }
    }

    let agent = state.get_agent_for_project_stage(input.project_id, input.stage.as_str())?;
    let has_history_context =
        agent.has_tool(agent_tools::SEARCH_STORY) || agent.has_tool(agent_tools::HISTORY_CONTEXT);
    let has_reference_materials = agent.has_tool(agent_tools::REFERENCE_MATERIALS);
    let has_chapter_memory = agent.has_tool(agent_tools::CHAPTER_MEMORY);
    let has_continuity_check = agent.has_tool(agent_tools::CONTINUITY_CHECK);
    let has_web_search = agent.has_tool(agent_tools::WEB_SEARCH);
    let settings = agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| {
            AppError::Validation("请先在设置里为当前供应商保存 AI API Key".to_string())
        })?;
    if prepared_context.is_none()
        && has_chapter_memory
        && chapter_memory::is_enabled()
        && matches!(input.stage, Stage::Draft | Stage::Review | Stage::Revision)
    {
        if let Some(chapter_id) = input.chapter_id {
            if let Some((predecessor_id, predecessor_no)) =
                direct_predecessor_chapter(state, input.project_id, chapter_id)?
            {
                if state
                    .latest_approved_chapter_body(input.project_id, predecessor_id)?
                    .is_some()
                {
                    let memory_settings = state.get_ai_settings_for_agent("chapter_memory")?;
                    let memory_agent = state.get_agent("chapter_memory")?;
                    let memory_api_key = state
                        .get_api_key_for_base_url(&memory_settings.base_url)?
                        .ok_or_else(|| {
                            AppError::Validation("请先为章节记忆 Agent 配置 API Key".to_string())
                        })?;
                    let previous_plan = approved_outline_section_for_chapter(
                        state,
                        input.project_id,
                        predecessor_no,
                    )?;
                    if let Err(error) = chapter_memory::ensure_predecessor_memory(
                        state,
                        input.project_id,
                        chapter_id,
                        &memory_settings,
                        &memory_api_key,
                        previous_plan.as_deref(),
                        &memory_agent.system_prompt,
                    )
                    .await
                    {
                        eprintln!(
                            "chapter memory unavailable; falling back to existing context: {error}"
                        );
                    }
                }
            }
        }
    }
    if prepared_context.is_none()
        && has_continuity_check
        && matches!(input.stage, Stage::Draft | Stage::Review | Stage::Revision)
    {
        if let Err(error) = continuity_ledger::ensure_ledger_current(state, input.project_id).await
        {
            eprintln!("continuity ledger unavailable for context search: {error}");
        }
    }
    let ledger_report = if has_continuity_check && matches!(input.stage, Stage::Review) {
        if let Some(artifact) = source_artifact.as_ref() {
            match continuity_ledger::check_artifact_continuity(
                state,
                crate::models::LedgerContinuityCheckRequest {
                    project_id: input.project_id,
                    artifact_id: artifact.id,
                },
            )
            .await
            {
                Ok(report) => Some(report),
                Err(error) => {
                    eprintln!("continuity ledger unavailable; continuing trial read: {error}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    let tool_source = agent_search_source(
        state,
        input.project_id,
        &input.stage,
        input.chapter_id,
        input.user_instruction.as_deref(),
        source_artifact.as_ref(),
    )?;
    let tool_context = if prepared_context.is_some() || !has_history_context {
        None
    } else if let Some(chapter_id) = input.chapter_id {
        match context_search::prepare_tool_context(
            state,
            input.project_id,
            chapter_id,
            &input.stage,
            &tool_source,
            &settings,
            &api_key,
        )
        .await
        {
            Ok(context) => context,
            Err(error) => {
                eprintln!(
                    "App context search unavailable; continuing with static context: {error}"
                );
                None
            }
        }
    } else {
        None
    };
    let mut prompt = if let Some(prepared) = prepared_context.as_ref() {
        prepared.prompt.clone()
    } else {
        build_prompt_for_agent(
            state,
            input.project_id,
            &input.stage,
            input.chapter_id,
            input.user_instruction.as_deref(),
            source_artifact.as_ref(),
            tool_context.as_deref(),
            &agent,
        )?
    };
    if prepared_context.is_none() && has_reference_materials {
        if let Some(reference_context) = crate::reference::render_context(
            state,
            input.project_id,
            &input.stage,
            input.reference_selection.as_ref(),
            &reference_query(
                state,
                input.project_id,
                &input.stage,
                input.chapter_id,
                input.user_instruction.as_deref(),
                source_artifact.as_ref(),
            )?,
        )? {
            prompt.push_str("\n\n");
            prompt.push_str(&reference_context);
        }
    }
    if prepared_context.is_none() && has_web_search {
        if let Some(query) = crate::web_search::requested_query(input.user_instruction.as_deref()) {
            let results = crate::web_search::search_summaries(state, &query).await?;
            if let Some(web_context) = crate::web_search::render_context(&query, &results) {
                prompt.push_str("\n\n");
                prompt.push_str(&web_context);
            }
        }
    }
    if let Some(report) = ledger_report.as_ref() {
        let guidance = continuity_ledger::render_report_for_prompt(report);
        if !guidance.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&guidance);
        } else {
            prompt.push_str("\n\n# App 状态账本核对结论\n程序化核对没有发现候选稿与已结构化物件、资源、禁制或伤势末态之间的直接冲突。不要把同一数量或同一状态的不同措辞误报为硬断点；试读应继续检查尚未结构化的人物知情边界、动机、入场路径和旧事件结果。");
        }
    }
    let review_evidence = if matches!(input.stage, Stage::Review) {
        review_evidence_corpus(
            state,
            input.project_id,
            input.chapter_id,
            source_artifact.as_ref(),
        )?
    } else {
        String::new()
    };

    let started = Instant::now();
    let mut run = if let Some(run) = existing_run {
        if state.run_cancellation_requested(run.id)? {
            let cancelled =
                state.update_workflow_run(run.id, "", "cancelled", Some("用户请求取消"), 0)?;
            state.insert_run_event(
                cancelled.id,
                input.project_id,
                input.chapter_id,
                "cancelled",
                "",
                "cancelled",
                Some("用户请求取消"),
            )?;
            return Err(AppError::Validation("Agent 运行已取消".to_string()));
        }
        state.update_workflow_run(run.id, "", "streaming", None, 0)?
    } else {
        let run = state.insert_workflow_run(
            input.project_id,
            input.chapter_id,
            input.stage.as_str(),
            &prompt,
            "",
            "streaming",
            None,
            0,
        )?;
        state.insert_run_event(
            run.id,
            input.project_id,
            input.chapter_id,
            "started",
            "",
            "streaming",
            None,
        )?;
        run
    };
    if state.run_cancellation_requested(run.id)? {
        let message = "Agent 运行已取消";
        state.update_workflow_run(
            run.id,
            "",
            "cancelled",
            Some(message),
            started.elapsed().as_millis() as i64,
        )?;
        state.insert_run_event(
            run.id,
            input.project_id,
            input.chapter_id,
            "cancelled",
            "",
            "cancelled",
            Some(message),
        )?;
        return Err(AppError::Validation(message.to_string()));
    }
    let mut latest_output = String::new();
    let mut last_persisted_chars = 0usize;
    let mut last_event_chars = 0usize;
    let mut last_thinking_event_chars = 0usize;
    let mut last_persisted_at = Instant::now();
    state.insert_run_event(
        run.id,
        input.project_id,
        input.chapter_id,
        "generation_started",
        "正文生成请求已发送，等待模型首段响应……",
        "streaming",
        None,
    )?;
    match ai::complete_chat_streaming(
        &settings,
        &api_key,
        &agent.system_prompt,
        &prompt,
        agent.temperature,
        |partial, thinking| {
            if state.run_cancellation_requested(run.id)? {
                return Err(AppError::Validation("Agent 运行已取消".to_string()));
            }
            latest_output = partial.to_string();
            let chars = partial.chars().count();
            if partial.is_empty()
                || chars.saturating_sub(last_persisted_chars) >= 120
                || last_persisted_at.elapsed().as_millis() >= 500
            {
                run = state.update_workflow_run(
                    run.id,
                    partial,
                    "streaming",
                    None,
                    started.elapsed().as_millis() as i64,
                )?;
                last_persisted_chars = chars;
                last_persisted_at = Instant::now();
            }
            let output_reset = partial.is_empty() && last_event_chars > 0;
            let delta = if chars >= last_event_chars {
                partial.chars().skip(last_event_chars).collect::<String>()
            } else {
                last_event_chars = 0;
                partial.to_string()
            };
            if output_reset {
                state.insert_run_event(
                    run.id,
                    input.project_id,
                    input.chapter_id,
                    "output_reset",
                    "",
                    "streaming",
                    None,
                )?;
            }
            let thinking_chars = thinking.chars().count();
            let thinking_delta = if thinking_chars >= last_thinking_event_chars {
                thinking
                    .chars()
                    .skip(last_thinking_event_chars)
                    .collect::<String>()
            } else {
                last_thinking_event_chars = 0;
                thinking.to_string()
            };
            if !thinking_delta.is_empty() {
                state.insert_run_event(
                    run.id,
                    input.project_id,
                    input.chapter_id,
                    "thinking_delta",
                    &thinking_delta,
                    "thinking",
                    None,
                )?;
                last_thinking_event_chars = thinking_chars;
            }
            if !delta.is_empty() {
                state.insert_run_event(
                    run.id,
                    input.project_id,
                    input.chapter_id,
                    "output_delta",
                    &delta,
                    "streaming",
                    None,
                )?;
                last_event_chars = chars;
            }
            Ok(())
        },
    )
    .await
    {
        Ok(output) => {
            let normalized_output = if matches!(input.stage, Stage::Review) {
                let mut issues = ai::parse_review_issues(&output);
                // Item/resource/injury state is checked by the App ledger against
                // both exact source quotes. Do not let a prose model overrule that
                // deterministic result with an unsupported "hard" conflict.
                issues.retain(|issue| !model_issue_duplicates_ledger_check(issue));
                issues.extend(ledger_review_issues(ledger_report.as_ref()));
                serde_json::to_string_pretty(&constrain_review_issues(issues, &review_evidence))?
            } else if matches!(input.stage, Stage::Draft | Stage::Revision) {
                normalize_story_body_output(&output)
            } else {
                output.clone()
            };
            let artifact = state.insert_artifact(
                input.project_id,
                input.chapter_id,
                input.stage.as_str(),
                &artifact_title(&input.stage, chapter.as_ref().map(|c| c.chapter_no)),
                &normalized_output,
                // A trial-reading report must retain the exact candidate it reviewed.
                // That lets the revision flow work before either artifact is approved.
                if matches!(input.stage, Stage::Review) {
                    source_artifact.as_ref().map(|artifact| artifact.id)
                } else {
                    parent_for_stage(state, input.project_id, &input.stage, input.chapter_id)?
                },
            )?;
            if has_reference_materials {
                if let Some(warning) = crate::reference::overlap_warning(
                    state,
                    input.project_id,
                    input.reference_selection.as_ref(),
                    &normalized_output,
                ) {
                    state.insert_message(
                        input.project_id,
                        input.chapter_id,
                        "reference_overlap_warning",
                        &warning,
                    )?;
                }
            }
            run = state.update_workflow_run(
                run.id,
                &normalized_output,
                "success",
                None,
                started.elapsed().as_millis() as i64,
            )?;
            state.link_run_artifact(run.id, artifact.id)?;
            sync_story_threads_after_generation(state, &artifact)?;
            state.insert_message(
                input.project_id,
                input.chapter_id,
                "agent_result",
                &format!(
                    "{} 生成 v{}：{}",
                    input.stage.title(),
                    artifact.version,
                    artifact.title
                ),
            )?;
            if emit_completion {
                state.insert_run_event(
                    run.id,
                    input.project_id,
                    input.chapter_id,
                    "completed",
                    "",
                    "success",
                    None,
                )?;
            }
            Ok(AgentStepResult { artifact, run })
        }
        Err(err) => {
            let message = err.to_string();
            let cancelled = state.run_cancellation_requested(run.id)?;
            let final_status = if cancelled { "cancelled" } else { "failed" };
            state.update_workflow_run(
                run.id,
                &latest_output,
                final_status,
                Some(&message),
                started.elapsed().as_millis() as i64,
            )?;
            state.insert_run_event(
                run.id,
                input.project_id,
                input.chapter_id,
                final_status,
                "",
                final_status,
                Some(&message),
            )?;
            Err(AppError::Validation(message))
        }
    }
}

// Agent context is persisted as PreparedContext and read through the background-run API.
// The former standalone preview endpoint intentionally no longer has a second representation.

#[cfg(test)]
pub async fn request_revision(
    state: &AppState,
    input: RevisionRequest,
) -> AppResult<AgentStepResult> {
    let source = state.get_artifact(input.artifact_id)?;
    if source.project_id != input.project_id {
        return Err(AppError::Validation("修订目标不属于当前项目".to_string()));
    }
    if source.stage != "draft" && source.stage != "revision" && source.stage != "review" {
        return Err(AppError::Validation(
            "只能对章节草稿、试读报告或修订稿发起修订".to_string(),
        ));
    }
    if input.feedback.trim().is_empty() {
        return Err(AppError::Validation("请填写修订反馈".to_string()));
    }
    validate_prerequisites(
        state,
        input.project_id,
        &Stage::Revision,
        source.chapter_id,
        Some(&source),
    )?;
    state.insert_message(
        input.project_id,
        source.chapter_id,
        "revision_feedback",
        input.feedback.trim(),
    )?;

    let agent = state.get_agent_for_project_stage(input.project_id, "revision")?;
    let has_history_context =
        agent.has_tool(agent_tools::SEARCH_STORY) || agent.has_tool(agent_tools::HISTORY_CONTEXT);
    let has_reference_materials = agent.has_tool(agent_tools::REFERENCE_MATERIALS);
    let has_continuity_check = agent.has_tool(agent_tools::CONTINUITY_CHECK);
    let settings = agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| {
            AppError::Validation("请先在设置里为当前供应商保存 AI API Key".to_string())
        })?;
    let revision_source =
        resolve_revision_source(state, input.project_id, source.chapter_id, Some(&source))?
            .ok_or_else(|| AppError::Validation("没有可修订的章节稿件".to_string()))?;
    if has_continuity_check {
        if let Err(error) = continuity_ledger::ensure_ledger_current(state, input.project_id).await
        {
            eprintln!("continuity ledger unavailable for revision search: {error}");
        }
    }
    let tool_source = revision_search_source(&revision_source, &source, input.feedback.as_str());
    let tool_context = if !has_history_context {
        None
    } else if let Some(chapter_id) = source.chapter_id {
        match context_search::prepare_tool_context(
            state,
            input.project_id,
            chapter_id,
            &Stage::Revision,
            &tool_source,
            &settings,
            &api_key,
        )
        .await
        {
            Ok(context) => context,
            Err(error) => {
                eprintln!("App context search unavailable; continuing revision: {error}");
                None
            }
        }
    } else {
        None
    };
    let mut prompt = build_prompt_for_agent(
        state,
        input.project_id,
        &Stage::Revision,
        source.chapter_id,
        Some(input.feedback.as_str()),
        Some(&source),
        tool_context.as_deref(),
        &agent,
    )?;
    if has_reference_materials {
        if let Some(reference_context) = crate::reference::render_context(
            state,
            input.project_id,
            &Stage::Revision,
            input.reference_selection.as_ref(),
            &reference_query(
                state,
                input.project_id,
                &Stage::Revision,
                source.chapter_id,
                Some(input.feedback.as_str()),
                Some(&source),
            )?,
        )? {
            prompt.push_str("\n\n");
            prompt.push_str(&reference_context);
        }
    }
    if has_continuity_check {
        match continuity_ledger::check_artifact_continuity(
            state,
            crate::models::LedgerContinuityCheckRequest {
                project_id: input.project_id,
                artifact_id: revision_source.id,
            },
        )
        .await
        {
            Ok(report) => {
                let guidance = continuity_ledger::render_report_for_prompt(&report);
                if !guidance.is_empty() {
                    prompt.push_str("\n\n");
                    prompt.push_str(&guidance);
                }
            }
            Err(error) => eprintln!("continuity ledger unavailable; continuing revision: {error}"),
        }
    }
    if has_continuity_check {
        if let Some(chapter_id) = revision_source.chapter_id {
            if let Some(chapter) = state.ensure_chapter(input.project_id, Some(chapter_id))? {
                if chapter.chapter_no > 1 {
                    let chapter_ids = state
                        .list_chapters(input.project_id)?
                        .into_iter()
                        .filter(|item| item.chapter_no + 1 >= chapter.chapter_no)
                        .filter(|item| item.chapter_no <= chapter.chapter_no)
                        .map(|item| item.id)
                        .collect::<Vec<_>>();
                    if chapter_ids.len() >= 2 {
                        if let Ok(report) = review_project_continuity(
                            state,
                            ContinuityReviewRequest {
                                project_id: input.project_id,
                                chapter_ids: Some(chapter_ids),
                                candidate_artifact_id: Some(revision_source.id),
                                candidate_artifact_ids: None,
                            },
                        )
                        .await
                        {
                            prompt.push_str(&format!(
                            "\n\n# 当前候选稿连续性预检\n结论：{}。\n摘要：{}\n这些问题是拿当前候选稿直接对照最近两章得到的，不是泛化建议。若其中有 major 或 moderate，必须优先修这些，再修句子和结尾落点：",
                            report.verdict, report.summary
                        ));
                            for issue in report.issues.iter().take(5) {
                                prompt.push_str(&format!(
                                    "\n- {}（{}）：{} 建议：{}",
                                    issue.issue_type,
                                    issue.severity,
                                    issue.reason,
                                    issue.suggestion
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    let started = Instant::now();
    let mut run = state.insert_workflow_run(
        input.project_id,
        source.chapter_id,
        "revision",
        &prompt,
        "",
        "streaming",
        None,
        0,
    )?;
    let mut latest_output = String::new();
    let mut last_persisted_chars = 0usize;
    let mut last_event_chars = 0usize;
    let mut last_thinking_event_chars = 0usize;
    let mut last_persisted_at = Instant::now();
    match ai::complete_chat_streaming(
        &settings,
        &api_key,
        &agent.system_prompt,
        &prompt,
        agent.temperature,
        |partial, thinking| {
            if state.run_cancellation_requested(run.id)? {
                return Err(AppError::Validation("Agent 运行已取消".to_string()));
            }
            latest_output = partial.to_string();
            let chars = partial.chars().count();
            if partial.is_empty()
                || chars.saturating_sub(last_persisted_chars) >= 120
                || last_persisted_at.elapsed().as_millis() >= 500
            {
                run = state.update_workflow_run(
                    run.id,
                    partial,
                    "streaming",
                    None,
                    started.elapsed().as_millis() as i64,
                )?;
                last_persisted_chars = chars;
                last_persisted_at = Instant::now();
            }
            let delta = if chars >= last_event_chars {
                partial.chars().skip(last_event_chars).collect::<String>()
            } else {
                last_event_chars = 0;
                partial.to_string()
            };
            if !delta.is_empty() {
                state.insert_run_event(
                    run.id,
                    input.project_id,
                    source.chapter_id,
                    "output_delta",
                    &delta,
                    "streaming",
                    None,
                )?;
                last_event_chars = chars;
            }
            let thinking_chars = thinking.chars().count();
            let thinking_delta = if thinking_chars >= last_thinking_event_chars {
                thinking
                    .chars()
                    .skip(last_thinking_event_chars)
                    .collect::<String>()
            } else {
                last_thinking_event_chars = 0;
                thinking.to_string()
            };
            if !thinking_delta.is_empty() {
                state.insert_run_event(
                    run.id,
                    input.project_id,
                    source.chapter_id,
                    "thinking_delta",
                    &thinking_delta,
                    "thinking",
                    None,
                )?;
                last_thinking_event_chars = thinking_chars;
            }
            Ok(())
        },
    )
    .await
    {
        Ok(output) => {
            let normalized_output = normalize_story_body_output(&output);
            let artifact = state.insert_artifact(
                input.project_id,
                source.chapter_id,
                "revision",
                "章节修订稿",
                &normalized_output,
                Some(revision_source.id),
            )?;
            if has_reference_materials {
                if let Some(warning) = crate::reference::overlap_warning(
                    state,
                    input.project_id,
                    input.reference_selection.as_ref(),
                    &normalized_output,
                ) {
                    state.insert_message(
                        input.project_id,
                        source.chapter_id,
                        "reference_overlap_warning",
                        &warning,
                    )?;
                }
            }
            run = state.update_workflow_run(
                run.id,
                &normalized_output,
                "success",
                None,
                started.elapsed().as_millis() as i64,
            )?;
            sync_story_threads_after_generation(state, &artifact)?;
            state.insert_message(
                input.project_id,
                source.chapter_id,
                "agent_result",
                &format!("修订 Agent 生成 v{}：{}", artifact.version, artifact.title),
            )?;
            Ok(AgentStepResult { artifact, run })
        }
        Err(err) => {
            let message = err.to_string();
            let cancelled = state.run_cancellation_requested(run.id)?;
            state.update_workflow_run(
                run.id,
                &latest_output,
                if cancelled { "cancelled" } else { "failed" },
                Some(&message),
                started.elapsed().as_millis() as i64,
            )?;
            Err(AppError::Validation(message))
        }
    }
}

pub fn replace_artifact_span(
    state: &AppState,
    input: SpanReplacementRequest,
) -> AppResult<AgentStepResult> {
    let source = state.get_artifact(input.artifact_id)?;
    if source.project_id != input.project_id {
        return Err(AppError::Validation(
            "局部替换目标不属于当前项目".to_string(),
        ));
    }
    if !matches!(
        source.stage.as_str(),
        "setting" | "outline" | "characters" | "draft" | "revision"
    ) {
        return Err(AppError::Validation(
            "只能对设定、大纲、角色、章节草稿或修订稿做局部替换".to_string(),
        ));
    }
    if input.find_text.trim().is_empty() {
        return Err(AppError::Validation("请填写要替换的原文片段".to_string()));
    }
    let matches = source.content.matches(&input.find_text).count();
    if matches == 0 {
        return Err(AppError::Validation(
            "原文片段没有在当前稿件中找到，请复制一段完全一致的文本".to_string(),
        ));
    }
    if matches > 1 {
        return Err(AppError::Validation(format!(
            "原文片段匹配到 {matches} 处，请扩大片段范围保证唯一匹配"
        )));
    }

    let patched_content = source
        .content
        .replacen(&input.find_text, &input.replace_text, 1);
    let note = input.note.as_deref().unwrap_or("").trim();
    if note.starts_with("删除候选卡片")
        && matches!(source.stage.as_str(), "setting" | "outline" | "characters")
    {
        let artifact =
            state.update_artifact_content(input.project_id, source.id, &patched_content)?;
        let run_input = format!(
            "# in-place-candidate-card-delete\n\nsource_artifact_id: {}\nnote: {}",
            source.id, note
        );
        let run = state.insert_workflow_run(
            input.project_id,
            source.chapter_id,
            "revision_patch",
            &run_input,
            &patched_content,
            "completed",
            None,
            0,
        )?;
        return Ok(AgentStepResult { artifact, run });
    }
    let run_input = format!(
        "# local-span-replacement\n\nsource_artifact_id: {}\nfind_chars: {}\nreplace_chars: {}\nnote: {}\n\n## find_text\n{}\n\n## replace_text\n{}",
        source.id,
        input.find_text.chars().count(),
        input.replace_text.chars().count(),
        note,
        input.find_text,
        input.replace_text
    );
    let started = Instant::now();
    let target_stage = match source.stage.as_str() {
        "draft" | "revision" => "revision",
        other => other,
    };
    let target_title = match target_stage {
        "setting" => "设定局部修订稿",
        "outline" => "大纲局部修订稿",
        "characters" => "角色局部修订稿",
        _ => "局部替换修订稿",
    };
    let artifact = state.insert_artifact(
        input.project_id,
        source.chapter_id,
        target_stage,
        target_title,
        &patched_content,
        Some(source.id),
    )?;
    let run = state.insert_workflow_run(
        input.project_id,
        source.chapter_id,
        "revision_patch",
        &run_input,
        &patched_content,
        "success",
        None,
        started.elapsed().as_millis() as i64,
    )?;
    if artifact.stage == "draft" || artifact.stage == "revision" {
        sync_story_threads_after_generation(state, &artifact)?;
    }
    state.insert_message(
        input.project_id,
        source.chapter_id,
        "revision_feedback",
        &format!(
            "局部替换生成{} v{}。{}",
            stage_label(&artifact.stage),
            artifact.version,
            if note.is_empty() { "" } else { note }
        ),
    )?;
    Ok(AgentStepResult { artifact, run })
}

fn normalize_story_body_output(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut lines = trimmed.lines();
    let Some(first_line_raw) = lines.next() else {
        return String::new();
    };
    let first_line = first_line_raw.trim();

    if looks_like_story_title_line(first_line) {
        let remainder = lines.collect::<Vec<_>>().join("\n").trim().to_string();
        if !remainder.is_empty() {
            return remainder;
        }
    }

    trimmed.to_string()
}

fn looks_like_story_title_line(line: &str) -> bool {
    let normalized = line.trim_start_matches('#').trim_start_matches('*').trim();
    if normalized.is_empty() {
        return false;
    }
    if normalized.len() > 32 {
        return false;
    }
    if normalized.starts_with("第") && normalized.contains("章") {
        return true;
    }
    normalized.starts_with("章节")
}

pub async fn revise_artifact_span_with_ai(
    state: &AppState,
    input: AiSpanRevisionRequest,
) -> AppResult<AgentStepResult> {
    let source = state.get_artifact(input.artifact_id)?;
    if source.project_id != input.project_id {
        return Err(AppError::Validation(
            "局部修订目标不属于当前项目".to_string(),
        ));
    }
    if !matches!(
        source.stage.as_str(),
        "setting" | "outline" | "characters" | "draft" | "revision"
    ) {
        return Err(AppError::Validation(
            "只能对设定、大纲、角色、章节草稿或修订稿做 AI 局部修订".to_string(),
        ));
    }
    if input.find_text.trim().is_empty() {
        return Err(AppError::Validation("请填写要修改的原文片段".to_string()));
    }
    if input.instruction.trim().is_empty() {
        return Err(AppError::Validation("请填写局部修订要求".to_string()));
    }
    let matches = source.content.matches(&input.find_text).count();
    if matches == 0 {
        return Err(AppError::Validation(
            "原文片段没有在当前稿件中找到，请复制一段完全一致的文本".to_string(),
        ));
    }
    if matches > 1 {
        return Err(AppError::Validation(format!(
            "原文片段匹配到 {matches} 处，请扩大片段范围保证唯一匹配"
        )));
    }

    let revision_agent = state.get_agent("artifact_revision")?;
    let has_history_context = revision_agent.has_tool(agent_tools::SEARCH_STORY)
        || revision_agent.has_tool(agent_tools::HISTORY_CONTEXT);
    let has_continuity_check = revision_agent.has_tool(agent_tools::CONTINUITY_CHECK);
    let settings = revision_agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| {
            AppError::Validation("请先在设置里为当前供应商保存 AI API Key".to_string())
        })?;

    let tool_context = if matches!(source.stage.as_str(), "draft" | "revision") {
        if let Some(chapter_id) = source.chapter_id {
            if has_continuity_check {
                if let Err(error) =
                    continuity_ledger::ensure_ledger_current(state, input.project_id).await
                {
                    eprintln!("continuity ledger unavailable for local revision search: {error}");
                }
            }
            let search_source = format!(
                "# 被修订正文\n{}\n\n# 指定片段\n{}\n\n# 人工要求\n{}",
                source.content,
                input.find_text,
                input.instruction.trim()
            );
            if has_history_context {
                match context_search::prepare_tool_context(
                    state,
                    input.project_id,
                    chapter_id,
                    &Stage::Revision,
                    &search_source,
                    &settings,
                    &api_key,
                )
                .await
                {
                    Ok(context) => context,
                    Err(error) => {
                        eprintln!("App context search unavailable for local revision: {error}");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let system_prompt = &revision_agent.system_prompt;
    let user_prompt = format!(
        "# 当前产物阶段\n{}\n\n# 全文上下文（仅供你保持风格、信息和连续性，不要改未指定部分）\n{}{}\n\n# 需要局部改写的原文片段\n{}\n\n# 局部修订要求\n{}\n\n# 输出要求\n1. 只输出“替换后的新片段”本身，不要输出别的。\n2. 只改这段，未被要求修改的信息、事实、称呼、物件状态、上下文关系要尽量保持连续。\n3. 若要求与上下文冲突，优先做最小必要修改，不扩展新设定。\n4. 新片段必须能直接替换原文片段。",
        stage_label(&source.stage),
        source.content,
        tool_context
            .as_deref()
            .map(|context| format!("\n\n{context}"))
            .unwrap_or_default(),
        input.find_text,
        input.instruction.trim()
    );

    let started = Instant::now();
    let replacement = ai::complete_chat(&settings, &api_key, system_prompt, &user_prompt, 0.35)
        .await?
        .trim()
        .to_string();
    if replacement.is_empty() {
        return Err(AppError::Validation(
            "AI 没有返回可用的局部修订内容".to_string(),
        ));
    }

    let patched_content = source.content.replacen(&input.find_text, &replacement, 1);
    let run_input = format!(
        "# ai-local-revision\n\nsource_artifact_id: {}\nfind_chars: {}\ninstruction_chars: {}\n\n## find_text\n{}\n\n## instruction\n{}",
        source.id,
        input.find_text.chars().count(),
        input.instruction.chars().count(),
        input.find_text,
        input.instruction.trim()
    );
    let target_stage = match source.stage.as_str() {
        "draft" | "revision" => "revision",
        other => other,
    };
    let target_title = match target_stage {
        "setting" => "设定 AI 局部修订稿",
        "outline" => "大纲 AI 局部修订稿",
        "characters" => "角色 AI 局部修订稿",
        _ => "AI 局部修订稿",
    };
    let artifact = state.insert_artifact(
        input.project_id,
        source.chapter_id,
        target_stage,
        target_title,
        &patched_content,
        Some(source.id),
    )?;
    let run = state.insert_workflow_run(
        input.project_id,
        source.chapter_id,
        "ai_revision_patch",
        &run_input,
        &patched_content,
        "success",
        None,
        started.elapsed().as_millis() as i64,
    )?;
    if artifact.stage == "draft" || artifact.stage == "revision" {
        sync_story_threads_after_generation(state, &artifact)?;
    }
    state.insert_message(
        input.project_id,
        source.chapter_id,
        "revision_feedback",
        &format!(
            "AI 局部修订生成{} v{}。{}",
            stage_label(&artifact.stage),
            artifact.version,
            input.instruction.trim()
        ),
    )?;
    Ok(AgentStepResult { artifact, run })
}

pub async fn review_project_continuity(
    state: &AppState,
    input: ContinuityReviewRequest,
) -> AppResult<ContinuityReport> {
    let detail = state.get_detail(input.project_id)?;
    let candidate_artifacts = continuity_candidate_artifacts(
        state,
        input.project_id,
        input.candidate_artifact_id,
        input.candidate_artifact_ids.as_deref().unwrap_or(&[]),
    )?;
    let candidate_artifact_ids = candidate_artifacts
        .iter()
        .map(|artifact| artifact.id)
        .collect::<Vec<_>>();
    let mut candidate_by_chapter = HashMap::new();
    for artifact in candidate_artifacts {
        let chapter_id = artifact.chapter_id.unwrap_or_default();
        if let Some(existing) = candidate_by_chapter.insert(chapter_id, artifact.clone()) {
            return Err(AppError::Validation(format!(
                "同一章节不能同时传入多个候选稿：{} 和 {}",
                existing.id, artifact.id
            )));
        }
    }
    let review_agent = state.get_agent("continuity_review")?;
    if !review_agent.has_tool(agent_tools::CONTINUITY_CHECK) {
        return Err(AppError::Validation(
            "当前连续性审校 Agent 未启用“连续性检查”工具，请先在 Agent 设置中启用".to_string(),
        ));
    }
    let settings = review_agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| {
            AppError::Validation("请先在设置里为当前供应商保存 AI API Key".to_string())
        })?;

    let mut selected_chapters = if let Some(ids) = input.chapter_ids.as_ref() {
        detail
            .chapters
            .clone()
            .into_iter()
            .filter(|chapter| ids.contains(&chapter.id))
            .collect::<Vec<_>>()
    } else {
        detail.chapters.clone()
    };
    for candidate_chapter_id in candidate_by_chapter.keys().copied() {
        if !selected_chapters
            .iter()
            .any(|chapter| chapter.id == candidate_chapter_id)
        {
            let chapter = state.ensure_chapter(input.project_id, Some(candidate_chapter_id))?;
            if let Some(chapter) = chapter {
                selected_chapters.push(chapter);
            }
        }
    }

    if selected_chapters.len() < 2 {
        return Err(AppError::Validation(
            "连续性审校至少需要两章已通过正文".to_string(),
        ));
    }
    selected_chapters.sort_by_key(|chapter| chapter.chapter_no);
    let chapter_ids = selected_chapters
        .iter()
        .map(|chapter| chapter.id)
        .collect::<Vec<_>>();
    let chapter_titles = selected_chapters
        .iter()
        .map(|chapter| chapter.title.clone())
        .collect::<Vec<_>>();
    let cache_key = continuity_cache_key(input.project_id, &chapter_ids, &candidate_artifact_ids);

    if let Some(report) =
        find_cached_continuity_report(state, input.project_id, &cache_key, chapter_titles.clone())?
    {
        return Ok(report);
    }

    let mut chapter_blocks = Vec::new();
    for chapter in &selected_chapters {
        let artifact = if let Some(candidate) = candidate_by_chapter.get(&chapter.id) {
            Some(candidate.clone())
        } else {
            state.latest_approved_chapter_body(input.project_id, chapter.id)?
        };
        let Some(artifact) = artifact else {
            return Err(AppError::Validation(format!(
                "章节《{}》缺少已通过正文，无法做连续性审校",
                chapter.title
            )));
        };
        let candidate_label = if candidate_by_chapter.contains_key(&chapter.id) {
            "（候选稿，尚未人工通过）"
        } else {
            ""
        };
        chapter_blocks.push(format!(
            "## {}{}\n{}\n",
            chapter.title, candidate_label, artifact.content
        ));
    }

    let mut context = project_context(&detail.project);
    append_approved_context(state, input.project_id, &mut context)?;
    if let Some(candidate) = candidate_by_chapter.values().next() {
        if let Ok(ledger_report) = continuity_ledger::check_artifact_continuity(
            state,
            crate::models::LedgerContinuityCheckRequest {
                project_id: input.project_id,
                artifact_id: candidate.id,
            },
        )
        .await
        {
            let guidance = continuity_ledger::render_report_for_prompt(&ledger_report);
            if !guidance.is_empty() {
                context.push_str("\n\n");
                context.push_str(&guidance);
            }
        }
    }
    let prompt = format!(
        "{context}\n\n# 审校任务\n你是连载小说总编，请检查以下连续章节是否具备可追读的一致性和衔接性。重点检查：\n1. 角色口吻、动机、能力、已知信息是否前后一致\n2. 上一章钩子是否在下一章被有效承接\n3. 物件、地点、时间、规则是否自洽\n4. 节奏是否出现断层，是否像不同人拼接出来的\n5. 多章是否持续兑现题材卖点，而不是每章重置气氛\n6. 若相邻两章明显属于同一场景、同一时段或同一冲突的直接续接，检查对白语气、情绪张力、追逐/伤势/门禁/站位等即时状态是否自然延续\n\n# 候选稿事实边界（审批前硬审计）\n若某章标注为“候选稿，尚未人工通过”，先把已批准设定、角色、大纲和此前已通过正文视为唯一事实来源，再审候选稿。不能因为候选稿写得顺、情绪够强或有更好看的场面，就把候选稿新增的内容默认视为有效。\n- 必须逐项核对候选稿是否偷换既有规则的触发条件、作用对象、效果、代价、结算时点或可重复性。\n- 必须核对人物知道什么、为何到场、已经发生过什么；不得把未建立的交易、调查、目击、计划、旧账、组织、地点、物件或关系补写成“原来早有”。\n- 必须核对物件、资源、伤势、禁制、门、令牌、药物和交易余额的状态；已耗尽、受损、未获得、未支付或未确认的内容不能在候选稿中直接可用或变成既成事实。\n- 候选稿可以出现新的现场细节和自然衍生动作，但凡新增内容会改变主角能力、资源、风险、人物动机、世界规则或下一步选择，必须能在已批准资料或前章正文中找到明确来源。找不到来源就是“事实越界”，severity 必须为 major。\n- 事实越界的 suggestion 只能要求删除、降为未确认痕迹、改回已有事实，或补回已有动作与过渡；不得建议再发明一条新规则去解释它。\n- 这条事实审计优先级高于文笔、节奏和爽点建议。\n\n# 同场景衔接规则\n- 这是一条软检查，不是强制要求每次跨章都连续同场景。\n- 若作者显然已经切场景、切视角、切主线，或存在合理时间跳跃，不要硬判问题。\n- 只有当两章看起来是直接接续同一场面时，才检查对白、氛围和即时状态是否接得上。\n- 若只是承接略生硬、气氛突然变调、人物刚才还在对峙下一章却像重开一幕，severity 优先给 minor 或 moderate。\n- 只有出现明确硬伤，例如伤势、位置、门是否打开、谁听见了什么、谁正在追谁等即时事实自相矛盾时，才可以给 major。\n- 遇到这类问题时，issue_type 优先写“同场景衔接”或“即时状态延续”。\n\n请只输出 JSON 数组。每项字段：issue_type, severity, chapters, reason, suggestion。severity 只能是 minor、moderate、major。若问题很少，也至少给出 1-3 条最关键意见。\n\n# 连续章节\n{}\n",
        chapter_blocks.join("\n")
    );
    let run_input = format!("# continuity-cache-key: {}\n\n{}", cache_key, prompt);

    let started = Instant::now();
    let raw = ai::complete_chat(
        &settings,
        &api_key,
        &review_agent.system_prompt,
        &prompt,
        0.0,
    )
    .await?;
    if let Some(report) =
        find_cached_continuity_report(state, input.project_id, &cache_key, chapter_titles.clone())?
    {
        return Ok(report);
    }
    let report = continuity_report_from_issues(
        input.project_id,
        chapter_titles,
        normalize_continuity_issues(parse_continuity_issues(&raw)),
    );

    state.insert_workflow_run(
        input.project_id,
        None,
        "continuity_review",
        &run_input,
        &raw,
        "success",
        None,
        started.elapsed().as_millis() as i64,
    )?;

    Ok(report)
}

pub async fn generate_chapter_split_plan(
    state: &AppState,
    input: ChapterSplitPlanRequest,
) -> AppResult<ChapterSplitPlan> {
    let project = state.get_project(input.project_id)?;
    let chapter = state
        .ensure_chapter(input.project_id, Some(input.chapter_id))?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let artifact = state.get_artifact(input.artifact_id)?;
    if artifact.project_id != input.project_id
        || artifact.chapter_id != Some(input.chapter_id)
        || (artifact.stage != "draft" && artifact.stage != "revision")
    {
        return Err(AppError::Validation(
            "拆章方案只能基于当前章节的草稿或修订稿生成".to_string(),
        ));
    }

    let split_agent = state.get_agent("chapter_split_plan")?;
    if !split_agent.has_tool(agent_tools::CHAPTER_SPLIT) {
        return Err(AppError::Validation(
            "当前拆章 Agent 未启用“拆章规划”工具，请先在 Agent 设置中启用".to_string(),
        ));
    }
    let has_history_context = split_agent.has_tool(agent_tools::SEARCH_STORY)
        || split_agent.has_tool(agent_tools::HISTORY_CONTEXT);
    let has_continuity_check = split_agent.has_tool(agent_tools::CONTINUITY_CHECK);
    let has_quality_analysis = split_agent.has_tool(agent_tools::QUALITY_ANALYSIS);
    let quality_report = has_quality_analysis.then(|| quality::analyze_artifact(&artifact));
    let continuity_issues = if has_continuity_check {
        latest_relevant_continuity_issues(state, input.project_id, chapter.chapter_no)?
    } else {
        Vec::new()
    };
    let settings = split_agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先在设置里保存 AI API Key".to_string()))?;
    let next_chapter = state
        .list_chapters(input.project_id)?
        .into_iter()
        .find(|item| item.chapter_no == chapter.chapter_no + 1);
    let fallback_next_title = next_chapter
        .as_ref()
        .map(|item| item.title.clone())
        .unwrap_or_else(|| format!("第 {} 章", chapter.chapter_no + 1));
    let current_outline =
        approved_outline_section_for_chapter(state, input.project_id, chapter.chapter_no)?
            .unwrap_or_default();
    let next_outline =
        approved_outline_section_for_chapter(state, input.project_id, chapter.chapter_no + 1)?
            .unwrap_or_default();

    let mut prompt = project_context(&project);
    if has_history_context {
        append_split_plan_context(
            state,
            input.project_id,
            chapter.id,
            chapter.chapter_no,
            &mut prompt,
        )?;
        append_recent_chapter_context(state, input.project_id, chapter.id, &mut prompt)?;
    }
    append_chapter_task_card(state, input.project_id, chapter.chapter_no, &mut prompt)?;
    prompt.push_str(&format!(
        "\n\n# 当前候选稿\n章节：{}（第 {} 章）\n产物阶段：{} v{}\n\n{}",
        chapter.title, chapter.chapter_no, artifact.stage, artifact.version, artifact.content
    ));

    if !current_outline.trim().is_empty() {
        prompt.push_str(&format!("\n\n# 当前章节原大纲\n{}", current_outline));
    }
    if !next_outline.trim().is_empty() {
        prompt.push_str(&format!("\n\n# 下一章节原大纲\n{}", next_outline));
    }

    if let Some(quality_report) = quality_report.as_ref() {
        prompt.push_str(&format!(
            "\n\n# 当前本地判断\n- 质量结论：{} / {}\n- 质量摘要：{}",
            quality_report.score, quality_report.verdict, quality_report.summary
        ));
        if !quality_report.warnings.is_empty() {
            prompt.push_str("\n- 本地质量警告：");
            for warning in quality_report.warnings.iter().take(4) {
                prompt.push_str(&format!(
                    "\n  * {}：{} 建议：{}",
                    warning.title, warning.detail, warning.suggestion
                ));
            }
        }
    } else {
        prompt.push_str(
            "\n\n# 当前本地判断\n质量分析工具未启用；请只依据候选稿、章节任务卡和已批准上下文判断是否需要拆章。",
        );
    }
    if !continuity_issues.is_empty() {
        prompt.push_str("\n- 最近一次多章连续性记录：");
        for issue in continuity_issues.iter().take(4) {
            prompt.push_str(&format!(
                "\n  * {} / {}：{} 建议：{}",
                issue.severity, issue.issue_type, issue.reason, issue.suggestion
            ));
        }
    }

    prompt.push_str(
        "\n\n# 任务\n你是长篇连载策划编辑，不负责润色正文，只负责把超载章节拆成更可用的两章执行方案。目标不是把内容越拆越多，而是把当前候选稿里最影响读者消化的负载拆开，让当前章和下一章各自只承担一个更清晰的章节功能。\n\n规则：\n1. 优先沿用现有设定、大纲、角色和已批准章节，不要为了拆章新增新体系、新人物职责、新设定。\n2. 当前章要保留最能兑现本章标题/任务的一条主动作线；其余过密信息移到下一章。\n3. 若上一章压力仍在，当前章结尾必须把压力接住，而不是凭空重开。\n4. 下一章开头必须承接当前章章末状态，不能像重置场景。\n5. 只做最小必要拆分；如果原稿只需要减法，也允许把“下一章”写成较窄的承接任务。\n6. 输出必须具体可执行，避免空泛词，如“加强节奏”“增加冲突”。\n\n只输出 JSON 对象，不要解释，不要 Markdown。字段必须齐全：\n- suggested_current_title: string\n- suggested_next_title: string\n- rationale: string\n- current_chapter_mission: string\n- next_chapter_mission: string\n- keep_in_current: string[]\n- move_to_next: string[]\n- carryover_closing_beats: string[]\n- next_chapter_opening_beats: string[]\n- revision_prompt_current: string\n- next_chapter_instruction: string\n",
    );

    let raw = ai::complete_chat(
        &settings,
        &api_key,
        &split_agent.system_prompt,
        &prompt,
        0.0,
    )
    .await?;

    parse_chapter_split_plan(
        &raw,
        input.project_id,
        input.chapter_id,
        input.artifact_id,
        &chapter.title,
        &fallback_next_title,
    )
}

fn validate_stage_scope(stage: &Stage, has_chapter: bool) -> AppResult<()> {
    match stage {
        Stage::Setting | Stage::Outline | Stage::Characters if has_chapter => Err(
            AppError::Validation("设定、大纲、角色阶段不绑定单个章节".to_string()),
        ),
        Stage::Draft | Stage::Review | Stage::Revision if !has_chapter => Err(
            AppError::Validation("写作、试读、修订阶段必须选择章节".to_string()),
        ),
        _ => Ok(()),
    }
}

fn validate_prerequisites(
    state: &AppState,
    project_id: i64,
    stage: &Stage,
    chapter_id: Option<i64>,
    source_artifact: Option<&Artifact>,
) -> AppResult<()> {
    match stage {
        Stage::Setting => {}
        Stage::Outline => require_approved(state, project_id, "setting", None)?,
        Stage::Characters => {
            require_approved(state, project_id, "setting", None)?;
            require_approved(state, project_id, "outline", None)?;
        }
        Stage::Draft => {
            require_approved(state, project_id, "setting", None)?;
            require_approved(state, project_id, "outline", None)?;
            require_approved(state, project_id, "characters", None)?;
            story_architecture::ensure_ready_for_draft(state, project_id)?;
        }
        Stage::Review => {
            let chapter_id = chapter_id.ok_or_else(|| {
                AppError::Validation("写作、试读、修订阶段必须选择章节".to_string())
            })?;
            if source_artifact.is_none()
                && state
                    .latest_approved_chapter_body(project_id, chapter_id)?
                    .is_none()
            {
                return Err(AppError::Validation(
                    "请先选择候选稿，或人工通过章节草稿/修订稿".to_string(),
                ));
            }
        }
        Stage::Revision => {
            if source_artifact.is_none() {
                require_approved(state, project_id, "review", chapter_id)?;
            }
        }
    }
    Ok(())
}

fn require_approved(
    state: &AppState,
    project_id: i64,
    stage: &str,
    chapter_id: Option<i64>,
) -> AppResult<()> {
    if state
        .approved_artifact(project_id, stage, chapter_id)?
        .is_none()
    {
        return Err(AppError::Validation(format!(
            "请先人工通过{}",
            stage_label(stage)
        )));
    }
    Ok(())
}

fn agent_search_source(
    state: &AppState,
    project_id: i64,
    stage: &Stage,
    chapter_id: Option<i64>,
    user_instruction: Option<&str>,
    source_artifact: Option<&Artifact>,
) -> AppResult<String> {
    let mut parts = Vec::new();
    match stage {
        Stage::Draft => {
            if let Some(chapter_id) = chapter_id {
                if let Some(chapter) = state.ensure_chapter(project_id, Some(chapter_id))? {
                    if let Some(section) =
                        approved_outline_section_for_chapter(state, project_id, chapter.chapter_no)?
                    {
                        parts.push(format!("# 本章章纲\n{section}"));
                    }
                }
            }
        }
        Stage::Review => {
            if let Some(source) = source_artifact {
                parts.push(format!("# 待试读正文\n{}", source.content));
            } else if let Some(chapter_id) = chapter_id {
                if let Some(source) = state.latest_approved_chapter_body(project_id, chapter_id)? {
                    parts.push(format!("# 待试读正文\n{}", source.content));
                }
            }
        }
        Stage::Revision => {
            if let Some(source) =
                resolve_revision_source(state, project_id, chapter_id, source_artifact)?
            {
                parts.push(format!("# 被修订正文\n{}", source.content));
            }
            if let Some(report) = source_artifact.filter(|artifact| artifact.stage == "review") {
                parts.push(format!("# 试读报告\n{}", report.content));
            }
        }
        _ => {}
    }
    if let Some(instruction) = user_instruction.filter(|text| !text.trim().is_empty()) {
        parts.push(format!("# 人工指令\n{}", instruction.trim()));
    }
    Ok(parts.join("\n\n"))
}

#[cfg(test)]
fn revision_search_source(
    revision_source: &Artifact,
    requested_source: &Artifact,
    feedback: &str,
) -> String {
    let mut parts = vec![format!("# 被修订正文\n{}", revision_source.content)];
    if requested_source.stage == "review" {
        parts.push(format!("# 试读报告\n{}", requested_source.content));
    }
    parts.push(format!("# 人工反馈\n{}", feedback.trim()));
    parts.join("\n\n")
}

fn reference_query(
    state: &AppState,
    project_id: i64,
    stage: &Stage,
    chapter_id: Option<i64>,
    user_instruction: Option<&str>,
    source_artifact: Option<&Artifact>,
) -> AppResult<String> {
    let project = state.get_project(project_id)?;
    let mut parts = vec![project.title, project.genre, project.premise];
    parts.push(stage.title().to_string());
    if let Some(chapter_id) = chapter_id {
        if let Some(chapter) = state.ensure_chapter(project_id, Some(chapter_id))? {
            parts.push(chapter.title);
        }
    }
    if let Some(instruction) = user_instruction.filter(|value| !value.trim().is_empty()) {
        parts.push(instruction.trim().to_string());
    }
    if let Some(source) = source_artifact {
        parts.push(source.content.chars().take(1800).collect());
    }
    Ok(parts.join("\n"))
}

#[cfg(test)]
fn build_prompt(
    state: &AppState,
    project_id: i64,
    stage: &Stage,
    chapter_id: Option<i64>,
    user_instruction: Option<&str>,
    source_artifact: Option<&Artifact>,
    tool_context: Option<&str>,
) -> AppResult<String> {
    build_prompt_internal(
        state,
        project_id,
        stage,
        chapter_id,
        user_instruction,
        source_artifact,
        tool_context,
        None,
    )
}

pub(crate) fn build_prompt_for_agent(
    state: &AppState,
    project_id: i64,
    stage: &Stage,
    chapter_id: Option<i64>,
    user_instruction: Option<&str>,
    source_artifact: Option<&Artifact>,
    tool_context: Option<&str>,
    agent: &Agent,
) -> AppResult<String> {
    build_prompt_internal(
        state,
        project_id,
        stage,
        chapter_id,
        user_instruction,
        source_artifact,
        tool_context,
        Some(agent),
    )
}

fn build_prompt_internal(
    state: &AppState,
    project_id: i64,
    stage: &Stage,
    chapter_id: Option<i64>,
    user_instruction: Option<&str>,
    source_artifact: Option<&Artifact>,
    tool_context: Option<&str>,
    agent: Option<&Agent>,
) -> AppResult<String> {
    let project = state.get_project(project_id)?;
    let has_history_context = agent
        .map(|item| {
            item.has_tool(agent_tools::SEARCH_STORY) || item.has_tool(agent_tools::HISTORY_CONTEXT)
        })
        .unwrap_or(true);
    let has_chapter_memory = agent
        .map(|item| item.has_tool(agent_tools::CHAPTER_MEMORY))
        .unwrap_or(true);
    let has_continuity_check = agent
        .map(|item| item.has_tool(agent_tools::CONTINUITY_CHECK))
        .unwrap_or(true);
    let has_quality_analysis = agent
        .map(|item| item.has_tool(agent_tools::QUALITY_ANALYSIS))
        .unwrap_or(true);
    let mut prompt = project_context(&project);
    append_approved_context(state, project_id, &mut prompt)?;
    prompt.push_str(&render_project_genre_skill(state, &project, stage)?);
    prompt.push_str(&render_supporting_skills_for_agent(
        state,
        &project,
        stage,
        agent.map(|item| item.allowed_skill_keys.as_slice()),
    )?);
    if matches!(stage, Stage::Outline) {
        append_written_progress_context(state, project_id, &mut prompt)?;
    }

    if let Some(chapter_id) = chapter_id {
        if let Some(chapter) = state.ensure_chapter(project_id, Some(chapter_id))? {
            prompt.push_str(&format!(
                "\n\n# 当前章节\n章节序号：{}\n标题：{}\n状态：{}",
                chapter.chapter_no, chapter.title, chapter.status
            ));
            if matches!(stage, Stage::Draft | Stage::Review | Stage::Revision) {
                append_chapter_task_card(state, project_id, chapter.chapter_no, &mut prompt)?;
                if has_history_context {
                    append_recent_chapter_context_with_options(
                        state,
                        project_id,
                        chapter.id,
                        has_chapter_memory,
                        &mut prompt,
                    )?;
                    append_retrieved_history(
                        state,
                        project_id,
                        chapter.id,
                        chapter.chapter_no,
                        user_instruction,
                        &mut prompt,
                    )?;
                }
                if has_continuity_check {
                    append_continuity_guidance(
                        state,
                        project_id,
                        chapter.chapter_no,
                        stage,
                        &mut prompt,
                    )?;
                    append_chapter_state_ledger(
                        state,
                        project_id,
                        chapter.id,
                        chapter.chapter_no,
                        user_instruction,
                        &mut prompt,
                    )?;
                }
                append_foreshadowing_context(state, project_id, chapter.chapter_no, &mut prompt)?;
                append_chapter_task_contract(
                    state,
                    project_id,
                    chapter.id,
                    chapter.chapter_no,
                    user_instruction,
                    &mut prompt,
                )?;
            }
        }
    }

    if let Some(tool_context) = tool_context.filter(|context| !context.trim().is_empty()) {
        prompt.push_str("\n\n");
        prompt.push_str(tool_context);
    }

    if let Some(source) = source_artifact {
        if matches!(stage, Stage::Setting | Stage::Outline | Stage::Characters) {
            prompt.push_str(&format!(
                "\n\n# 当前已存在版本\n以下内容是当前正在迭代的同阶段版本。若人工指令只要求局部调整，你应优先保留未被点名修改的结构和有效信息，只改需要改的局部，而不是整篇推倒重写。\n\n{}",
                source.content
            ));
        }
    }

    match stage {
        Stage::Setting => prompt.push_str("\n\n# 任务\n生成或重写本书的核心设定。必须包含：一句话卖点、核心规则、主要禁忌、风格标尺、可持续冲突来源。按设定复杂度写足，不用为了压缩字数省掉必要边界；也不要写百科式世界史。"),
        Stage::Outline => prompt.push_str(&outline_task_for_prompt(state, project_id)?),
        Stage::Characters => prompt.push_str("\n\n# 任务\n基于已批准设定和大纲生成角色卡；每个主要角色必须包含：欲望、恐惧、底线、说话方式、首次登场功能、与主角的冲突/互补。按角色复杂度写足，减少履历堆砌。"),
        Stage::Draft => prompt.push_str("\n\n# 任务\n为当前章节写完整正文。只输出正文，不要写标题、分析或说明。优先场景、动作、对白和细节推进；少解释设定，少抽象总结，避免连续使用“像……一样”的模板化比喻。开篇应让读者逐渐或立即看清本章正在处理什么，以及人物为何选择现在行动；章末完成本章功能并形成自然延续，可以落在结果、决定、信息改写、行动启动、关系新平衡或情绪余韵，不强制危险和反转。严禁把当前章节写成资料汇总、线索清单或设定说明会。若你发现前文有很多待回收线索，本章最多处理其中 1-2 条，其余保留到后续章节。章节可按场景、对白、情绪承接和冲突解决的实际需要写长；只有在重复解释、重复确认或多个独立章节功能互相争抢篇幅时，才应删减或拆章。\n\n把这一章理解成一次主要推进，不是一次世界观清仓：优先完成 1 个明确章节功能，例如修炼小成、炼药试错、开门、核验、跟踪、谈判、藏证据、确认身份或布局下一场。新名词、新地点和新人物功能只在完成本章任务确有必要时引入，不按固定数量机械限制。若故事自然冒出更多谜底或设定，只保留最能改变主角下一步选择的一条，其余只写成物证、异常、疑点或未确认线索。不要用“第一/第二/第三”总结真相，不要连续写“不是……而是……”解释机制。\n\n若本章动用了旧能力、旧物件、旧血脉、旧令牌或旧资源做比前文更强的新动作，必须同时满足两件事：1）明确这仍然基于前文已出现的功能，或来自同源外物/一次性触发，不是主角无铺垫永久升级；2）当章立刻兑现更重代价、暴露或失控风险。若做不到，就降回感知、开门、试探、逼退、换取片段信息等较窄用途。"),
        Stage::Review => {
            let chapter_id = chapter_id.ok_or_else(|| {
                AppError::Validation("写作、试读、修订阶段必须选择章节".to_string())
            })?;
            let draft = source_artifact.cloned().map(Ok).unwrap_or_else(|| {
                state
                    .latest_approved_chapter_body(project_id, chapter_id)?
                    .ok_or_else(|| {
                        AppError::Validation(
                            "请先选择候选稿，或人工通过章节草稿/修订稿".to_string(),
                        )
                    })
            })?;
            let quality_context = if has_quality_analysis {
                quality_report_for_prompt(&quality::analyze_artifact(&draft))
            } else {
                "当前 Agent 未启用质量分析工具；本次试读不使用本地自动评分，只依据候选稿、已批准资料和人工指令审校。"
                    .to_string()
            };
            prompt.push_str(&format!(
                "\n\n# 待试读章节\n{}\n\n# 本地质量信号\n{}\n\n# 章节任务兑现审校\n本章柔性任务契约和章节任务卡是本次审校的语义合同。请先判断候选稿是否实质执行了其中的“本章目标、主要阻力、必须发生的变化、离开状态”：关键行动、选择或结果缺席，被其他事件替代，或只被一句话提及而没有改变局面时，标出 issue_type 为“章节任务未兑现”。只有主要章节功能整体缺席时才给 major；结尾形式与建议不同但已经完成等效状态变化，不算问题。允许同义表达、等效行动、合理场景改写和不同结尾类型；绝不要求照抄章节标题、人物名、资源名或任务卡的字面词汇。\n\n# 审校事实边界\n{}\n建议只能删减、重排、强化候选稿已有动作，或继续使用已批准资料中明确存在的事实。不要建议增加新人物、新物件、新地点、新规则、临时安排或过去事件；如果某个问题只能靠新增事实解决，直接标为事实越界，保留问题但不要给出该新增修法。\n\n# 输出格式\n请只输出 JSON 数组，列出 3-8 个最影响追读的问题。每项包含 issue_type、severity、location、reason、suggestion、evidence_quote、action_evidence_quote 七个字段，不要额外解释。evidence_quote 必须是候选稿、已批准资料或已通过前章中连续出现的 8-80 个字原文，用来证明问题。action_evidence_quote 是可选字段：建议需要强化或继续使用既有动作时，提供其中连续出现的 8-80 个字原文；建议只是删减、合并或重排已有文字时可以留空。若建议需要新增事实、物件、人物、地点、规则、安排或过去事件，就不要给出该建议。severity 只能是 minor、moderate、major。优先覆盖本地质量信号暴露出的真实阅读风险，但不要机械复述指标名。",
                draft.content,
                quality_context,
                REVIEW_FACT_BOUNDARY
            ));
        }
        Stage::Revision => {
            let source = resolve_revision_source(state, project_id, chapter_id, source_artifact)?
                .ok_or_else(|| AppError::Validation("没有可修订的章节稿件".to_string()))?;
            let review = if let Some(review) = source_artifact.filter(|artifact| artifact.stage == "review") {
                Some(review.clone())
            } else {
                state.approved_artifact(project_id, "review", chapter_id)?
            };
            let (quality_context, revision_contract) = if has_quality_analysis {
                let report = quality::analyze_artifact(&source);
                (
                    quality_report_for_prompt(&report),
                    revision_contract_for_prompt(&report),
                )
            } else {
                (
                    "当前 Agent 未启用质量分析工具；不要使用本地自动评分驱动修订，只依据源稿、试读报告、人工反馈和已批准事实。"
                        .to_string(),
                    "未启用本地质量分析工具。只修复试读报告和人工反馈中有证据的问题，不为了任何自动指标改写正文。"
                        .to_string(),
                )
            };
            let review_content = review
                .as_ref()
                .map(|artifact| artifact.content.as_str())
                .unwrap_or("尚无已批准试读报告；请严格依据人工反馈、本地质量信号和连续性修复卡修订。");
            prompt.push_str(&format!(
                "\n\n# 原稿\n{}\n\n# 已批准试读报告\n{}\n\n# 本地质量信号\n{}\n\n# 修订约束卡\n{}\n\n# 任务\n输出修订后的完整章节正文，不要解释修改。必须优先解决 major/moderate 问题；保留有效氛围和题材细节；删掉解释感、模板化比喻和重复句式。若本地质量信号显示开篇驱动力、结尾功能、段落重量、解释感或信息反转偏密存在问题，必须在正文里实质修正；但不能为了指标把成长、关系、恢复或过渡章强改成冲突章。若本地质量信号显示叙事失焦，保留必要场景、对白和情绪承接，只删重复解释、重复确认或无后果过渡。\n\n修订时如果发现原稿塞入了过多新名词、新规则、新人物职责、多层真相或多层反转，你必须主动减法：只保留最能改变主角选择的一条核心信息，其余改成疑点、物证、未确认线索或后续章再验证。不要用“第一/第二/第三”总结真相，不要连续写“不是……而是……”解释机制；把说明改成场景阻碍、对白试探、物件状态变化或具体代价。只有当人物互动确实承担本章变化时才补对白，不为对白密度硬塞对话。结尾应按本章模式完成结果、决定、信息改写、行动启动、关系新平衡或情绪余韵；不得为了更刺激凭空加入外部事件、时限、人物或威胁。若旧能力、旧物件、旧血脉或旧资源在本章被写出了明显超出前文的新用途，优先降效果、补外部来源或改成一次性触发，并把代价写得比收益更具体，不能把一次性爆发写成无铺垫永久升级。",
                source.content,
                review_content,
                quality_context,
                revision_contract
            ));
            prompt.push_str("\n\n# 连续性优先级\n修订不是润色。若以下任一类问题存在，必须先改结构再改句子：\n1. 角色已知信息断点：旧角色不能突然知道上一章没有给出的秘密；若必须知道，正文必须写出他/她刚刚获得该信息的动作、代价或路径。\n2. 角色动机断点：上一章为资源、排名、仇怨、交易而行动的人，本章不能无说明变成守门、旁观或单纯推动剧情的工具人。\n3. 物件/禁制状态断点：令牌、门、阵、伤势、药力、封锁若上一章有状态变化，本章必须交代复原、重新激活、持续生效或失效原因。\n4. 主角收益断点：探索/布局章必须让主角带走一个能用的小收益或明确避祸路线；只知道更多秘密不算可用收益。\n若原稿无法同时修好这些问题，允许重写场景顺序、删掉角色提前入场、推迟部分秘密揭示。");
            prompt.push_str("\n\n修订时必须保留并兑现上面的“本章柔性任务契约”。如果原稿没有完成目标状态变化，应先修因果与人物选择；不要为了贴合模板强行重排成固定场景数或固定转折位置。");
        }
    }

    if let Some(instruction) = user_instruction {
        if !instruction.trim().is_empty() {
            prompt.push_str(&format!("\n\n# 人工追加指令\n{}", instruction.trim()));
        }
    }

    if matches!(stage, Stage::Revision) {
        prompt.push_str(&format!(
            "\n\n# 修订不可突破的事实边界\n{REVISION_FACT_BOUNDARY}"
        ));
    }

    Ok(prompt)
}

fn render_project_genre_skill(
    state: &AppState,
    project: &Project,
    stage: &Stage,
) -> AppResult<String> {
    let profile = state.get_genre_agent_for_project(project.id)?;
    let skill = genre_skill::genre_skill_for_id(&profile.primary_skill_key)
        .unwrap_or_else(|| genre_skill::detect_genre_skill(&project.genre));
    let template = state
        .get_writing_skill_by_key(skill.skill_id())?
        .filter(|record| record.enabled && !record.content.trim().is_empty())
        .map(|record| record.content)
        .unwrap_or_else(|| skill.fallback_template().to_string());

    Ok(genre_skill::render_genre_skill_from_template(
        &project.genre,
        stage,
        skill.skill_id(),
        &template,
    ))
}

#[cfg(test)]
fn render_supporting_skills(
    state: &AppState,
    project: &Project,
    stage: &Stage,
) -> AppResult<String> {
    render_supporting_skills_for_agent(state, project, stage, None)
}

fn render_supporting_skills_for_agent(
    state: &AppState,
    project: &Project,
    stage: &Stage,
    agent_allowed_skill_keys: Option<&[String]>,
) -> AppResult<String> {
    let profile = state.get_genre_agent_for_project(project.id)?;
    let genre_skill_id = profile.primary_skill_key.as_str();
    let mut blocks = Vec::new();

    for skill in state.list_writing_skills()? {
        if !skill.enabled || skill.content.trim().is_empty() {
            continue;
        }
        if skill.skill_key == genre_skill_id || skill.category == "genre" {
            continue;
        }
        if !profile
            .allowed_skill_keys
            .iter()
            .any(|allowed| allowed == &skill.skill_key)
        {
            continue;
        }
        if let Some(agent_allowed_skill_keys) = agent_allowed_skill_keys {
            if !agent_allowed_skill_keys
                .iter()
                .any(|allowed| allowed == &skill.skill_key)
            {
                continue;
            }
        }

        let intro = match skill.category.as_str() {
            "craft" => "以下是跨题材复用的写作工艺约束，用来补足题材 skill 不负责的连续性、主动性、信息控制与章节落点。",
            _ => "以下是当前项目额外启用的辅助写作约束。",
        };

        blocks.push(genre_skill::render_stage_scoped_skill(
            "写作 Craft Skill",
            &skill.skill_key,
            &format!("技能名称：{}。{}", skill.name, intro),
            stage,
            &skill.content,
        ));
    }

    Ok(blocks.join(""))
}

fn append_approved_context(
    state: &AppState,
    project_id: i64,
    prompt: &mut String,
) -> AppResult<()> {
    for stage in ["setting", "outline", "characters"] {
        if let Some(artifact) = state.approved_artifact(project_id, stage, None)? {
            prompt.push_str(&format!(
                "\n\n# 已批准{} v{}\n{}",
                stage_label(stage),
                artifact.version,
                artifact.content
            ));
        }
    }
    let cards = state
        .list_knowledge_cards(project_id)?
        .into_iter()
        .filter(|card| card.status == "approved")
        .collect::<Vec<_>>();
    if !cards.is_empty() {
        prompt.push_str("\n\n# 已确认动态资料卡");
        prompt.push_str(
            "\n以下卡片与已批准设定同等有效。没有出现在这里的待确认资料不得当作事实使用。",
        );
        for card in cards {
            prompt.push_str(&format!(
                "\n\n## [{}] {}\n{}",
                card.category, card.title, card.content
            ));
        }
    }
    Ok(())
}

fn append_foreshadowing_context(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
    prompt: &mut String,
) -> AppResult<()> {
    let chapter_numbers = state
        .list_chapters(project_id)?
        .into_iter()
        .map(|chapter| (chapter.id, (chapter.chapter_no, chapter.title)))
        .collect::<HashMap<_, _>>();
    let entries = state
        .list_foreshadowings(project_id)?
        .into_iter()
        .filter(|item| item.status == "active" || item.status == "ready_for_payoff")
        .filter(|item| {
            item.planted_chapter_id
                .and_then(|id| chapter_numbers.get(&id).map(|entry| entry.0))
                .map(|planted_no| planted_no <= chapter_no)
                .unwrap_or(true)
        })
        .take(8)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Ok(());
    }

    prompt.push_str("\n\n# 已确认伏笔账本");
    prompt.push_str(
        "\n每章最多主动推进或回收其中 1-2 条。未到回收时机的伏笔只保留存在感，不得一次性讲透。",
    );
    for item in entries {
        let planned = item
            .planned_payoff_chapter_id
            .and_then(|id| chapter_numbers.get(&id).map(|entry| entry.0))
            .map(|number| format!("预计第 {} 章回收", number))
            .filter(|value| !value.is_empty())
            .or_else(|| {
                (!item.planned_payoff_note.trim().is_empty())
                    .then(|| item.planned_payoff_note.clone())
            })
            .unwrap_or_else(|| "回收时机尚未指定".to_string());
        prompt.push_str(&format!(
            "\n- {}（{}）：{} | {}",
            item.title, item.status, item.content, planned
        ));
    }
    Ok(())
}

fn append_recent_chapter_context(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    prompt: &mut String,
) -> AppResult<()> {
    append_recent_chapter_context_with_options(state, project_id, current_chapter_id, true, prompt)
}

fn append_recent_chapter_context_with_options(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    include_chapter_memory: bool,
    prompt: &mut String,
) -> AppResult<()> {
    let chapters = state.list_chapters(project_id)?;
    let Some(current) = chapters
        .iter()
        .find(|chapter| chapter.id == current_chapter_id)
    else {
        return Ok(());
    };

    let previous = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < current.chapter_no)
        .rev()
        .take(2)
        .collect::<Vec<_>>();

    if previous.is_empty() {
        return Ok(());
    }

    let direct_predecessor_id = previous.first().map(|chapter| chapter.id);
    prompt.push_str(
        "\n\n# 最近已通过章节上下文\n已批准正文才是事实来源，自动检索只补充较早相关资料。",
    );
    for chapter in previous.into_iter().rev() {
        if let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter.id)? {
            prompt.push_str(&format!(
                "\n\n## 近章结尾原文：{}\n{}",
                chapter.title,
                tail_excerpt(&artifact.content, 1_200)
            ));
            if Some(chapter.id) == direct_predecessor_id {
                let obligations = carryover_obligations_from_excerpt(&artifact.content);
                if !obligations.is_empty() {
                    prompt.push_str("\n原文可见的待承接压力：");
                    for item in obligations {
                        prompt.push_str(&format!("\n- {}", item));
                    }
                }
            }
        }
    }
    if include_chapter_memory && chapter_memory::is_enabled() {
        if let Some(predecessor_id) = direct_predecessor_id {
            if let Some(memory) =
                chapter_memory::current_memory_for_chapter(state, project_id, predecessor_id)?
            {
                prompt.push_str("\n\n# 直接前章交接记忆（全文提取，来源引文已校验）");
                prompt.push_str(
                "\n这是正式前章的可重建派生记忆，用来补足尾段之外的状态变化。若它与上方前章原文冲突，始终以前章原文为准。",
            );
                prompt.push('\n');
                prompt.push_str(&chapter_memory::render_memory_context(&memory)?);
            }
        }
    }
    prompt.push_str(
        "\n\n连续性优先级：已批准正文 > 已批准资料 > 本章任务 > 历史检索或派生账本。直接前章若与概括性资料不一致，以正文实际发生的动作、状态和已知信息为准；不要无故重置气氛或重复解释。",
    );
    Ok(())
}

fn append_written_progress_context(
    state: &AppState,
    project_id: i64,
    prompt: &mut String,
) -> AppResult<()> {
    let chapters = state.list_chapters(project_id)?;
    let written = chapters
        .iter()
        .filter_map(|chapter| {
            chapter
                .current_artifact_id
                .map(|artifact_id| (chapter, artifact_id))
        })
        .collect::<Vec<_>>();
    let next_unwritten = chapters
        .iter()
        .find(|chapter| chapter.current_artifact_id.is_none());

    prompt.push_str("\n\n# 已写正文进度（应用数据库）");
    prompt.push_str("\n只有章节当前正式正文会进入这里。候选稿、试读和未批准修订不属于已写进度。");
    if written.is_empty() {
        prompt.push_str("\n当前没有已完成的正式章节，可以从第 1 章开始规划。");
    } else {
        prompt.push_str("\n以下章节已经写完并锁定：只能总结其实际结果，不能重新规划、改名、替换事件或把它们当成待写章节。");
        for (chapter, artifact_id) in written {
            let artifact = state.get_artifact(artifact_id)?;
            prompt.push_str(&format!(
                "\n\n## 已锁定：第 {} 章 {}\n- 开场摘录：{}\n- 结尾摘录：{}",
                chapter.chapter_no,
                chapter.title,
                head_excerpt(&artifact.content, 180),
                tail_excerpt(&artifact.content, 320)
            ));
        }
    }
    if let Some(chapter) = next_unwritten {
        prompt.push_str(&format!(
            "\n\n规划起点：第 {} 章 {}。新章节任务必须从这里或更后面开始。",
            chapter.chapter_no, chapter.title
        ));
    } else {
        prompt.push_str("\n\n当前章节列表均已有正式正文；如需续写，只规划现有列表之后的新章节。");
    }
    Ok(())
}

fn outline_task_for_prompt(state: &AppState, project_id: i64) -> AppResult<String> {
    let chapters = state.list_chapters(project_id)?;
    let written_count = chapters
        .iter()
        .filter(|chapter| chapter.current_artifact_id.is_some())
        .count();
    let next_unwritten = chapters
        .iter()
        .find(|chapter| chapter.current_artifact_id.is_none())
        .map(|chapter| chapter.chapter_no)
        .unwrap_or_else(|| {
            chapters
                .last()
                .map(|chapter| chapter.chapter_no + 1)
                .unwrap_or(1)
        });
    let shared = "每个待写章节只写章节模式、进入状态、章节目标、主要阻力、进度/状态变化、退出状态或对下一章形成的新条件。结尾可以是结果落地、决定形成、信息改写、行动启动或情绪余韵，不强制每章制造危险式钩子。按生产需要完整展开，不设固定字数；但不要把章节正文、重复规则或远期谜底提前写进大纲。已批准设定是能力合同：核心器物、血脉、功法和身份只能使用设定中已明确的功能；任何新效果只能作为未验证痕迹、外部一次性条件或远期问题，不能直接成为稳定解法。";

    if written_count == 0 {
        Ok(format!(
            "\n\n# 任务\n基于已批准设定生成可执行大纲。先写整书主线，再写前 12 章待写列表；{shared}"
        ))
    } else {
        Ok(format!(
            "\n\n# 任务\n基于已批准设定和正式正文继续维护可执行大纲。输出分为两部分：\n1. 已写进度摘要：只列上方 {written_count} 个已锁定章节的标题，以及摘录中能够直接验证的实际结果；不得为这些章节重新生成章节模式、目标、阻力、事件、能力、资源或退出状态。\n2. 后续待写规划：详细章节生产卡必须从第 {next_unwritten} 章开始；不要再次输出第 1 章至第 {} 章的生产卡，也不要用规划内容修正正式正文。可以规划接下来最多 12 个待写章节，不按总目标字数平均切块。\n{shared}",
            next_unwritten - 1
        ))
    }
}

fn direct_predecessor_chapter(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
) -> AppResult<Option<(i64, i64)>> {
    let chapters = state.list_chapters(project_id)?;
    let Some(current) = chapters
        .iter()
        .find(|chapter| chapter.id == current_chapter_id)
    else {
        return Ok(None);
    };
    Ok(chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < current.chapter_no)
        .max_by_key(|chapter| chapter.chapter_no)
        .map(|chapter| (chapter.id, chapter.chapter_no)))
}

fn append_split_plan_context(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    chapter_no: i64,
    prompt: &mut String,
) -> AppResult<()> {
    let mut query_parts = Vec::new();
    if let Some(section) = approved_outline_section_for_chapter(state, project_id, chapter_no)? {
        query_parts.push(section);
    }
    if let Some(section) = approved_outline_section_for_chapter(state, project_id, chapter_no + 1)?
    {
        query_parts.push(section);
    }
    let query = query_parts.join("\n");
    if query.trim().is_empty() {
        return Ok(());
    }

    let snippets = search_story_context(
        state,
        StoryContextSearchInput {
            project_id,
            chapter_id: Some(current_chapter_id),
            query,
            limit: Some(6),
            include_immediate_previous: false,
        },
    )?;
    if snippets.is_empty() {
        return Ok(());
    }

    prompt.push_str("\n\n# 相关已批准上下文");
    prompt.push_str(
        "\n以下内容是根据当前章与下一章任务自动检索出的相关设定、角色、旧章节或人工记录。只用于约束拆章，不要把它们扩写成新信息：",
    );
    for snippet in snippets {
        prompt.push_str(&format!(
            "\n- [{}] 命中词：{} | {}",
            snippet.source_label, snippet.matched_term, snippet.content
        ));
    }
    Ok(())
}

fn append_chapter_task_card(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
    prompt: &mut String,
) -> AppResult<()> {
    let Some(section) = approved_outline_section_for_chapter(state, project_id, chapter_no)? else {
        return Ok(());
    };

    prompt.push_str("\n\n# 本章任务卡");
    prompt.push_str(
        "\n这是本章的叙事合同，用来保证它确实推进主线，而不是要求按固定模板逐项打卡。优先让章节目标、核心冲突、信息释放和结尾走向在正文中自然落地。",
    );
    prompt.push_str(&format!("\n{}", section));
    prompt.push_str("\n\n写作时要兑现任务卡里的核心冲突和结尾走向，不能只借用题目和气氛。通常让一个主要动作线主导本章；成长、关系、恢复、交易和转场也可以是完整章节功能，不必强行追加打斗或反转。若出现多个会改变主角选择的新设定、谜底或人物动机，优先让其中一项成为本章重心，其余保留为未确认线索或留待后文展开。");
    prompt.push_str("\n章节应能清楚说出“人物此刻想完成什么、遇到什么阻碍、最后局面怎样变化”。这是检查失焦的工具，不是要求把正文压缩成固定句式；章节长短由必要场景、对白和情绪承接决定。");
    Ok(())
}

fn append_continuity_guidance(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
    stage: &Stage,
    prompt: &mut String,
) -> AppResult<()> {
    let runs = state.list_workflow_runs(project_id)?;
    let Some(run) = runs
        .into_iter()
        .filter(|run| run.stage == "continuity_review" && run.status == "success")
        .max_by_key(|run| run.id)
    else {
        return Ok(());
    };

    let issues = parse_continuity_issues(&run.output);
    if issues.is_empty() {
        return Ok(());
    }

    let chapters = state.list_chapters(project_id)?;
    let active_titles = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no <= chapter_no)
        .map(|chapter| chapter.title.clone())
        .collect::<Vec<_>>();
    let relevant = issues
        .into_iter()
        .filter(|issue| {
            issue
                .chapters
                .iter()
                .any(|title| active_titles.iter().any(|active| active == title))
        })
        .collect::<Vec<_>>();

    if relevant.is_empty() {
        return Ok(());
    }

    prompt.push_str("\n\n# 连续性修复卡");
    if matches!(stage, Stage::Revision) {
        prompt.push_str("\n下面这些是上一轮多章审校发现的跨章问题。修订时必须优先修复 major / moderate 问题；修复方式必须是补过渡、删越级推断、统一时间线、改动机触发点，不得通过新增新组织、新名单、新法宝或新机制来解释。");
    } else {
        prompt.push_str("\n下面这些是上一轮多章审校已经发现的问题。本章只处理与当前章节目标直接相关的 1-2 个问题；用动作、过渡和线索承接来修，不要为了补连续性而牺牲本章节奏。");
    }
    for issue in relevant
        .iter()
        .filter(|issue| {
            matches!(stage, Stage::Revision)
                || issue.severity == "major"
                || issue.severity == "moderate"
        })
        .take(5)
    {
        prompt.push_str(&format!(
            "\n- {}（{}）：{} 建议：{}",
            issue.issue_type, issue.severity, issue.reason, issue.suggestion
        ));
    }
    Ok(())
}

fn latest_relevant_continuity_issues(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
) -> AppResult<Vec<ContinuityIssue>> {
    let runs = state.list_workflow_runs(project_id)?;
    let Some(run) = runs
        .into_iter()
        .filter(|run| run.stage == "continuity_review" && run.status == "success")
        .max_by_key(|run| run.id)
    else {
        return Ok(Vec::new());
    };

    let issues = parse_continuity_issues(&run.output);
    if issues.is_empty() {
        return Ok(Vec::new());
    }

    let chapters = state.list_chapters(project_id)?;
    let active_titles = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no <= chapter_no)
        .map(|chapter| chapter.title.clone())
        .collect::<Vec<_>>();
    Ok(issues
        .into_iter()
        .filter(|issue| {
            issue.chapters.iter().any(|title| {
                active_titles.iter().any(|active| {
                    active == title || active.replace(' ', "") == title.replace(' ', "")
                })
            })
        })
        .take(5)
        .collect())
}

fn append_retrieved_history(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    chapter_no: i64,
    user_instruction: Option<&str>,
    prompt: &mut String,
) -> AppResult<()> {
    let Some(query) = build_history_query(state, project_id, chapter_no, user_instruction)? else {
        return Ok(());
    };

    let snippets = search_story_context(
        state,
        StoryContextSearchInput {
            project_id,
            chapter_id: Some(current_chapter_id),
            query,
            limit: Some(6),
            include_immediate_previous: false,
        },
    )?;
    if snippets.is_empty() {
        return Ok(());
    }

    prompt.push_str("\n\n# 历史检索上下文");
    prompt.push_str(
        "\n以下片段是按当前章节任务自动检索出的较早上下文，用来帮助你回收远距离人物、物件、编号和旧事件。只在与本章任务直接相关时使用，不要为了回收而硬塞进正文：",
    );
    prompt.push_str(
        "\n使用协议：先判断当前章是否真的需要这些历史片段；若需要，优先只回收 1-2 条最直接相关的信息；若检索结果和本章主冲突无关，宁可不用，也不要机械点名。",
    );
    for snippet in snippets {
        prompt.push_str(&format!(
            "\n- [{}] 命中词：{} | {}",
            snippet.source_label, snippet.matched_term, snippet.content
        ));
    }
    Ok(())
}

fn append_chapter_state_ledger(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    chapter_no: i64,
    user_instruction: Option<&str>,
    prompt: &mut String,
) -> AppResult<()> {
    let Some(query) = build_history_query(state, project_id, chapter_no, user_instruction)? else {
        return Ok(());
    };
    let snippets = search_story_context(
        state,
        StoryContextSearchInput {
            project_id,
            chapter_id: Some(current_chapter_id),
            query,
            limit: Some(8),
            include_immediate_previous: false,
        },
    )?;
    let story_threads = state.list_story_threads(project_id)?;
    let active_threads = if story_threads.is_empty() {
        snippets
            .iter()
            .take(3)
            .map(|snippet| format!("{} -> {}", snippet.matched_term, snippet.source_label))
            .collect::<Vec<_>>()
    } else {
        story_threads
            .iter()
            .filter(|thread| thread.status == "active" || thread.status == "due")
            .take(3)
            .map(format_thread_summary)
            .collect::<Vec<_>>()
    };
    let deferred_threads = if story_threads.is_empty() {
        snippets
            .iter()
            .skip(3)
            .take(3)
            .map(|snippet| format!("{} -> {}", snippet.matched_term, snippet.source_label))
            .collect::<Vec<_>>()
    } else {
        story_threads
            .iter()
            .filter(|thread| thread.status == "deferred")
            .take(4)
            .map(format_thread_summary)
            .collect::<Vec<_>>()
    };
    let confirmed_facts = if snippets.is_empty() {
        story_threads
            .iter()
            .filter(|thread| thread.status == "active" || thread.status == "due")
            .take(3)
            .map(|thread| {
                if thread.notes.trim().is_empty() {
                    thread.label.clone()
                } else {
                    format!("{} | {}", thread.label, thread.notes.trim())
                }
            })
            .collect::<Vec<_>>()
    } else {
        snippets
            .iter()
            .take(3)
            .map(|snippet| format!("{} | {}", snippet.source_label, snippet.content))
            .collect::<Vec<_>>()
    };

    let mut current_costs = story_threads
        .iter()
        .filter_map(|thread| {
            thread
                .current_cost
                .as_ref()
                .filter(|cost| !cost.trim().is_empty())
                .map(|cost| format!("{} -> {}", thread.label, cost.trim()))
        })
        .take(3)
        .collect::<Vec<_>>();
    if current_costs.is_empty() {
        current_costs = collect_current_costs(state, project_id, chapter_no)?;
    }
    let do_not_expand = vec![
        "不要新增新的组织体系，除非本章任务卡明确要求".to_string(),
        "不要同时解释多个编号体系或多次归档机制".to_string(),
        "不要把暂缓线索写成新支线或新谜底爆发".to_string(),
    ];

    let chapters = state.list_chapters(project_id)?;
    let recent_titles = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < chapter_no)
        .rev()
        .take(2)
        .map(|chapter| chapter.title.clone())
        .collect::<Vec<_>>();
    let character_anchors = collect_character_anchors(state, project_id, current_chapter_id)?;
    let object_anchors = collect_object_anchors(state, project_id, current_chapter_id)?;
    let unresolved_hooks = collect_unresolved_hooks(state, project_id, chapter_no)?;

    prompt.push_str("\n\n# 章节状态账本");
    prompt.push_str(
        "\n这个账本不是让你逐条交代，而是帮助你决定“这一章现在处理什么，什么先别动”。如果你写的内容和账本冲突，以账本为准收束章节负载。",
    );

    if active_threads.is_empty() {
        prompt.push_str("\n- 当前主处理线：优先兑现本章任务卡，不额外扩展远距离旧线索。");
    } else {
        prompt.push_str("\n- 当前主处理线：");
        for thread in active_threads {
            prompt.push_str(&format!("\n  * {}", thread));
        }
    }

    if deferred_threads.is_empty() {
        prompt.push_str("\n- 暂缓线索：若本章未直接需要，不主动扩写额外旧线索。");
    } else {
        prompt.push_str("\n- 暂缓线索：以下线索可知道其存在，但本章最多轻触，不展开解释：");
        for thread in deferred_threads {
            prompt.push_str(&format!("\n  * {}", thread));
        }
    }

    if confirmed_facts.is_empty() {
        prompt.push_str("\n- 近期已确认事实：暂无额外历史片段需要确认。");
    } else {
        prompt.push_str("\n- 近期已确认事实：");
        for fact in confirmed_facts {
            prompt.push_str(&format!("\n  * {}", fact));
        }
    }

    if current_costs.is_empty() {
        prompt.push_str("\n- 当前代价：暂无显式代价记录；仍需承接最近章节已经成立的行动后果、关系变化或限制条件。");
    } else {
        prompt.push_str("\n- 当前代价：");
        for cost in current_costs {
            prompt.push_str(&format!("\n  * {}", cost));
        }
    }

    if recent_titles.is_empty() {
        prompt.push_str("\n- 近章状态：暂无已通过前章。");
    } else {
        prompt.push_str(&format!(
            "\n- 近章状态：上一阶段已发生的直接后果主要来自 {}，请承接这些后果，不要重置人物心理或规则压力。",
            recent_titles.join("、")
        ));
    }

    if character_anchors.is_empty() {
        prompt.push_str("\n- 角色立场锚点：若本章继续使用旧角色，默认沿用前章已表现出的立场、知道的信息和对主角的态度，不允许无说明改阵营。");
    } else {
        prompt.push_str("\n- 角色立场锚点：");
        for anchor in character_anchors {
            prompt.push_str(&format!("\n  * {}", anchor));
        }
    }

    if object_anchors.is_empty() {
        prompt.push_str("\n- 物件/规则状态锚点：重要物件、证据、工具、契约、伤势、限制条件一旦写过状态，本章必须承接，不能默认自动变化。");
    } else {
        prompt.push_str("\n- 物件/规则状态锚点：");
        for anchor in object_anchors {
            prompt.push_str(&format!("\n  * {}", anchor));
        }
    }

    if unresolved_hooks.is_empty() {
        prompt.push_str(
            "\n- 上章未结状态：若上一章留下仍在生效的行动、压力、决定、关系变化或情绪惯性，本章应在合适位置承接；允许通过时间跳跃或切线处理，但要让变化有依据。",
        );
    } else {
        prompt.push_str("\n- 上章未结状态：");
        for hook in unresolved_hooks {
            prompt.push_str(&format!("\n  * {}", hook));
        }
    }

    prompt.push_str("\n- 本章禁扩项：");
    for item in do_not_expand {
        prompt.push_str(&format!("\n  * {}", item));
    }

    prompt.push_str(
        "\n- 本章执行规则：1）形成一个清晰的主要推进；较长章节可以容纳多个因果相连的变化，但不能让互不相关的功能争抢篇幅；2）围绕当前主处理线兑现真正影响本章选择的内容；3）暂缓线索只保留必要存在感，不展开成新支线；4）优先把当前代价、成长、关系或认知变化写实，而不是继续加设定；5）角色立场、已知信息、物件状态若要变化，必须在正文里写出导致变化的动作或发现；6）结尾要落稳本章已经发生的结果、决定、关系、信息、行动或情绪变化，不以是否出现新危险判断好坏。",
    );

    Ok(())
}

fn append_chapter_task_contract(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    chapter_no: i64,
    user_instruction: Option<&str>,
    prompt: &mut String,
) -> AppResult<()> {
    let contract = build_chapter_task_contract(
        state,
        project_id,
        current_chapter_id,
        chapter_no,
        user_instruction,
    )?;

    prompt.push_str("\n\n# 本章柔性任务契约");
    prompt.push_str(
        "\n以下内容定义本章从什么状态进入、需要完成什么变化，以及结束后局面有什么不同。它不是场景拆分，也不是固定节拍公式；可使用一个长场景、多个短场景、概述、留白或时间跳跃，只要因果和人物反应成立。",
    );
    prompt.push_str(&format!("\n- 章节模式：{}", contract.chapter_mode));
    prompt.push_str(&format!("\n- 进入状态：{}", contract.entry_state));
    prompt.push_str(&format!("\n- 本章目标：{}", contract.objective));
    prompt.push_str(&format!("\n- 主要阻力：{}", contract.resistance));
    prompt.push_str(&format!("\n- 历史上下文用途：{}", contract.context_focus));
    prompt.push_str(&format!("\n- 必须发生的变化：{}", contract.required_change));
    prompt.push_str(&format!("\n- 结尾功能：{}", contract.ending_function));
    prompt.push_str(&format!(
        "\n- 离开状态 / 下一章新条件：{}",
        contract.next_condition
    ));
    if let Some(hook_carryover) = contract.hook_carryover {
        prompt.push_str(&format!("\n- 上章钩子回响：{}", hook_carryover));
    }
    if let Some(payoff) = contract.payoff {
        prompt.push_str(&format!("\n- 本章进度增量：{}", payoff));
    }

    if !contract.must_use_threads.is_empty() {
        prompt.push_str("\n- 本章必须兑现的线索：");
        for item in contract.must_use_threads {
            prompt.push_str(&format!("\n  * {}", item));
        }
    }
    if !contract.must_avoid_threads.is_empty() {
        prompt.push_str("\n- 本章禁止展开的线索：");
        for item in contract.must_avoid_threads {
            prompt.push_str(&format!("\n  * {}", item));
        }
    }
    prompt.push_str(
        "\n- 使用原则：开篇应让读者逐渐或立即看清本章在处理什么；若上一章留下即时压力，本章前段要让它仍然有效，但回应可以是躲避、恢复、准备、推演、布局或正面对抗。结尾不强制危险或反转，可以落在结果、决定、信息改写、行动启动、关系新平衡或情绪余韵上；关键是本章已经交付应有变化。",
    );
    prompt.push_str(
        "\n- 连续性要求：若本章写到旧角色、旧物件、旧禁制、旧编号，先沿用前文最后一次确认的状态；除非正文里真的发生了变化，否则不要私自改阵营、改已知信息、改物件归属、改规则描述。",
    );
    Ok(())
}

fn build_chapter_task_contract(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    chapter_no: i64,
    user_instruction: Option<&str>,
) -> AppResult<ChapterTaskContract> {
    let outline =
        approved_outline_section_for_chapter(state, project_id, chapter_no)?.unwrap_or_default();
    let chapter_title = state
        .ensure_chapter(project_id, Some(current_chapter_id))?
        .map(|chapter| chapter.title)
        .unwrap_or_else(|| format!("第 {} 章", chapter_no));
    let objective = extract_outline_field(&outline, "章节目标")
        .or_else(|| first_non_heading_line(&outline))
        .unwrap_or_else(|| format!("推进 {}", chapter_title));
    let conflict = extract_outline_field(&outline, "核心冲突").unwrap_or_else(|| {
        "让人物处理与本章目标直接相关的具体阻力，而不是先做背景说明".to_string()
    });
    let release = extract_outline_field(&outline, "信息释放").unwrap_or_else(|| {
        "释放足以改变人物判断或选择的信息，其余内容按需要保留为未确认状态".to_string()
    });
    let next_condition = extract_outline_field(&outline, "退出状态")
        .or_else(|| extract_outline_field(&outline, "下一章新条件"))
        .or_else(|| extract_outline_field(&outline, "结尾钩子"))
        .unwrap_or_else(|| "让本章状态变化形成自然结束，或为下一章建立清楚的新条件".to_string());
    let chapter_mode = infer_chapter_mode(
        &chapter_title,
        &objective,
        &conflict,
        &release,
        user_instruction,
    );

    let story_threads = state.list_story_threads(project_id)?;
    let must_use_threads = story_threads
        .iter()
        .filter(|thread| thread.status == "due" || thread.status == "active")
        .take(2)
        .map(|thread| thread.label.clone())
        .collect::<Vec<_>>();
    let must_avoid_threads = story_threads
        .iter()
        .filter(|thread| thread.status == "deferred")
        .take(3)
        .map(|thread| thread.label.clone())
        .collect::<Vec<_>>();
    let cost = collect_current_costs(state, project_id, chapter_no)?
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            "承接上一章仍然有效的行动后果、关系变化或限制条件，不能无依据重置".to_string()
        });
    let history_hint = build_history_query(state, project_id, chapter_no, user_instruction)?
        .map(|query| summarize_history_query(&query))
        .unwrap_or_else(|| "只回收最直接相关的旧线索".to_string());
    let genre_skill = crate::genre_skill::detect_genre_skill(&state.get_project(project_id)?.genre);
    let hook_carryover = recent_chapter_hook_carryover(state, project_id, chapter_no)?;
    let payoff = match genre_skill {
        crate::genre_skill::GenreSkillKind::XianxiaPowerFantasy => Some(xianxia_progress_requirement(&chapter_mode)),
        crate::genre_skill::GenreSkillKind::Mystery => Some(
            "本章至少兑现 1 项证据或判断推进，例如确认线索真伪、拿到新证据、排除一种解释、暴露证词矛盾或提出更精确的问题。"
                .to_string(),
        ),
        crate::genre_skill::GenreSkillKind::UrbanSupernatural => Some(
            "本章至少兑现 1 项能力或现实处境变化，例如试出能力边界、取得成长收益、承担使用代价、改变身份关系或触发可信的社会反馈。"
                .to_string(),
        ),
        crate::genre_skill::GenreSkillKind::GeneralSerialized => None,
    };

    let entry_state = hook_carryover
        .clone()
        .unwrap_or_else(|| format!("承接当前仍有效的压力或代价：{cost}"));
    let ending_function = ending_function_for_mode(&chapter_mode);

    Ok(ChapterTaskContract {
        chapter_mode,
        entry_state,
        objective,
        resistance: conflict,
        context_focus: history_hint,
        required_change: format!(
            "让信息、关系、资源、能力、位置、目标或情绪惯性发生与本章任务相称的可验证变化；当前计划优先关注：{release}。变化可以通过结果、决定或代价逐步形成，不要求固定在中段。"
        ),
        ending_function,
        next_condition,
        payoff,
        hook_carryover,
        must_use_threads,
        must_avoid_threads,
    })
}

fn ending_function_for_mode(chapter_mode: &str) -> String {
    if chapter_mode.contains("成长准备") || chapter_mode.contains("资源技艺") {
        "优先使用结果落地型或决定形成型：写清本次练习、恢复、制作或资源处理实际完成了什么，还留下什么限制；不必追加突发敌人。".to_string()
    } else if chapter_mode.contains("关系谈判") {
        "优先落在关系新平衡、承诺、拒绝或筹码变化；可以安静收束，但双方下一次行动条件必须已经改变。"
            .to_string()
    } else if chapter_mode.contains("探索调查") {
        "优先使用信息改写型或行动启动型：确认一层事实并让角色据此做出选择；不要只停在看见新门、新光或新轮廓。".to_string()
    } else if chapter_mode.contains("强冲突") {
        "优先使用结果落地型：交代对抗结果、筹码变化和代价；后续压力应从本章结果自然产生，而不是另塞无关悬念。".to_string()
    } else {
        "可使用决定形成型、行动启动型或情绪余韵型；不要求制造危险，但必须让路线、关系、准备状态或读者理解比章初更明确。".to_string()
    }
}

fn infer_chapter_mode(
    chapter_title: &str,
    objective: &str,
    conflict: &str,
    release: &str,
    user_instruction: Option<&str>,
) -> String {
    let text = [
        chapter_title,
        objective,
        conflict,
        release,
        user_instruction.unwrap_or_default(),
    ]
    .join("\n");

    if contains_any(
        &text,
        &[
            "训练", "学习", "练习", "提升", "恢复", "养伤", "疗伤", "准备", "修炼",
        ],
    ) {
        "成长准备章：重点写方法、瓶颈、试错、小成和远期压力回响，不要求当场爆发。".to_string()
    } else if contains_any(
        &text,
        &[
            "制作", "调配", "采购", "材料", "工具", "资源", "样品", "修复",
        ],
    ) {
        "资源技艺章：重点写资源获取/处理、操作风险、产物效果和现实代价。".to_string()
    } else if contains_any(
        &text,
        &["交易", "谈判", "换", "盟友", "人情", "立场", "合作"],
    ) {
        "关系谈判章：重点写筹码、立场、试探和关系变化，不要求正面冲突。".to_string()
    } else if contains_any(
        &text,
        &[
            "潜入", "确认", "核验", "探索", "线索", "真相", "遗迹", "秘", "门",
        ],
    ) {
        "探索调查章：重点写行动、痕迹、判断和一层信息揭示，禁止一章讲透整段谜底。".to_string()
    } else if contains_any(
        &text,
        &[
            "比赛", "挑战", "追", "杀", "堵", "压迫", "抢", "夺", "反击", "对峙",
        ],
    ) {
        "强冲突章：重点写正面压力、筹码变化、对抗结果和代价。".to_string()
    } else {
        "过渡布局章：重点写选择、准备、路线调整和下一场压力，不要求当场爆发。".to_string()
    }
}

fn xianxia_progress_requirement(chapter_mode: &str) -> String {
    if chapter_mode.contains("成长准备") {
        "本章至少兑现 1 项成长增量：练法更清楚、瓶颈被定位、招式有雏形、伤势有处理、境界或战力推进一小步。".to_string()
    } else if chapter_mode.contains("资源技艺") {
        "本章至少兑现 1 项资源增量：材料、丹药、灵物、炼制经验、火候判断或资源代价必须具体可见。"
            .to_string()
    } else if chapter_mode.contains("关系谈判") {
        "本章至少兑现 1 项关系/筹码增量：换到情报、建立人情、暴露立场、达成交易或埋下可用承诺。"
            .to_string()
    } else if chapter_mode.contains("探索调查") {
        "本章至少兑现 1 项信息增量：确认一条旧线索、发现一层规则或拿到一个新判断，但不要一次揭完整卷谜底。".to_string()
    } else if chapter_mode.contains("强冲突") {
        "本章至少兑现 1 项对抗增量：压回挑衅、保住资源、夺到资格、逼退敌人、暴露新风险或付出明确代价。".to_string()
    } else {
        "本章至少兑现 1 项布局增量：明确下一步路线、准备一张底牌、改变一个选择或把远期压力推近一格。".to_string()
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn recent_chapter_hook_carryover(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
) -> AppResult<Option<String>> {
    let chapters = state.list_chapters(project_id)?;
    let previous = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < chapter_no)
        .max_by_key(|chapter| chapter.chapter_no);
    let Some(previous) = previous else {
        return Ok(None);
    };
    let Some(body) = state.latest_approved_chapter_body(project_id, previous.id)? else {
        return Ok(None);
    };
    let tail = tail_excerpt(&body.content, 260);
    let triggers = [
        "盯",
        "跟",
        "追",
        "封",
        "时限",
        "倒计时",
        "敲门",
        "脚步",
        "比赛",
        "挑战",
        "杀",
        "开门",
        "回来",
        "必须",
        "不能",
    ];
    if triggers.iter().any(|needle| tail.contains(needle)) {
        Ok(Some(format!(
            "上一章章末的直接压力来自《{}》尾段：先回应那里的追兵、监视、时限、对峙或门槛，再展开新动作。",
            previous.title
        )))
    } else {
        Ok(None)
    }
}

fn extract_outline_field(section: &str, label: &str) -> Option<String> {
    section.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with('-') {
            return None;
        }
        let rest = trimmed.trim_start_matches('-').trim();
        let prefix = format!("{label}：");
        if let Some(value) = rest.strip_prefix(&prefix) {
            return Some(value.trim().to_string());
        }
        let prefix = format!("{label}:");
        rest.strip_prefix(&prefix)
            .map(|value| value.trim().to_string())
    })
}

fn first_non_heading_line(section: &str) -> Option<String> {
    section.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            None
        } else {
            Some(trimmed.trim_start_matches('-').trim().to_string())
        }
    })
}

fn summarize_history_query(query: &str) -> String {
    let terms = extract_history_terms(query);
    if terms.is_empty() {
        "只回收最直接相关的旧线索".to_string()
    } else {
        format!(
            "旧线索里只优先使用 {}",
            terms.into_iter().take(2).collect::<Vec<_>>().join("、")
        )
    }
}

fn format_thread_summary(thread: &crate::models::StoryThread) -> String {
    let mut summary = format!("{} [{}]", thread.label, thread.status);
    if let Some(chapter_no) = thread.last_seen_chapter_no {
        summary.push_str(&format!(" 第{}章最近触及", chapter_no));
    }
    if !thread.notes.trim().is_empty() {
        summary.push_str(&format!(" -> {}", thread.notes.trim()));
    }
    summary
}

fn sync_story_threads_after_generation(state: &AppState, artifact: &Artifact) -> AppResult<()> {
    if artifact.stage != "draft" && artifact.stage != "revision" {
        return Ok(());
    }
    sync_story_threads_from_artifact(state, artifact)
}

pub(crate) fn sync_story_threads_from_artifact(
    state: &AppState,
    artifact: &Artifact,
) -> AppResult<()> {
    // Only an approved body is canonical enough to update future chapter state.
    // Pending candidates can contain exactly the inconsistencies a later revision must remove.
    let artifact = state.get_artifact(artifact.id)?;
    if (artifact.stage != "draft" && artifact.stage != "revision") || artifact.status != "approved"
    {
        return Ok(());
    }
    let Some(chapter_id) = artifact.chapter_id else {
        return Ok(());
    };
    let chapter = state
        .ensure_chapter(artifact.project_id, Some(chapter_id))?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    // The ledger is input for later chapters, so its sources must be canonical.
    // In particular, human revision notes describe what to remove and must never
    // become story facts simply because a candidate mentioned them.
    let mut query_parts = vec![artifact.content.clone()];
    if let Some(section) =
        approved_outline_section_for_chapter(state, artifact.project_id, chapter.chapter_no)?
    {
        query_parts.push(section);
    }
    let mut snippets = retrieve_history_snippets(
        state,
        artifact.project_id,
        chapter_id,
        &query_parts.join("\n"),
        true,
        false,
    )?;
    let registered_labels = registered_story_thread_labels(state, artifact.project_id)?;
    snippets.retain(|snippet| is_valid_story_thread_term(&snippet.matched_term));
    snippets.sort_by(|left, right| {
        story_thread_term_priority(right, &registered_labels)
            .cmp(&story_thread_term_priority(left, &registered_labels))
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| {
                right
                    .matched_term
                    .chars()
                    .count()
                    .cmp(&left.matched_term.chars().count())
            })
    });

    let mut active_keys = Vec::new();
    let mut deferred_keys = Vec::new();
    let mut resolved_keys = Vec::new();
    let current_cost = extract_primary_cost(&artifact.content);

    for (index, snippet) in snippets.iter().enumerate() {
        let key = normalize_thread_key(&snippet.matched_term);
        let kind = classify_thread_kind(&snippet.matched_term, &snippet.source_label);
        let label = snippet.matched_term.trim();
        let notes = format!("{} | {}", snippet.source_label, snippet.content);
        let status = if snippet_used_in_artifact(&artifact.content, snippet) {
            if snippet_resolved_in_artifact(&artifact.content, snippet) {
                resolved_keys.push(key.clone());
                "resolved"
            } else {
                active_keys.push(key.clone());
                "active"
            }
        } else if index < 5 {
            deferred_keys.push(key.clone());
            "deferred"
        } else {
            "deferred"
        };
        state.upsert_story_thread(
            artifact.project_id,
            &key,
            label,
            kind,
            status,
            current_cost.as_deref(),
            Some(chapter.chapter_no),
            Some(artifact.id),
            &notes,
        )?;
    }

    active_keys.truncate(2);
    deferred_keys.truncate(4);
    resolved_keys.truncate(2);
    state.update_story_thread_statuses_after_chapter(
        artifact.project_id,
        chapter.chapter_no,
        artifact.id,
        &active_keys,
        &deferred_keys,
        &resolved_keys,
        current_cost.as_deref(),
    )?;
    Ok(())
}

/// Rebuild the derived continuity ledger after an application restart.
/// Artifacts and messages remain untouched; only cache-like thread metadata is refreshed.
pub(crate) fn rebuild_story_threads(state: &AppState) -> AppResult<()> {
    for project in state.list_projects()? {
        state.clear_story_threads(project.id)?;
        for chapter in state.list_chapters(project.id)? {
            if let Some(body) = state.latest_approved_chapter_body(project.id, chapter.id)? {
                sync_story_threads_from_artifact(state, &body)?;
            }
        }
    }
    Ok(())
}

fn normalize_thread_key(term: &str) -> String {
    term.trim().to_lowercase()
}

fn registered_story_thread_labels(state: &AppState, project_id: i64) -> AppResult<HashSet<String>> {
    let mut labels = HashSet::new();
    for card in state.list_knowledge_cards(project_id)? {
        if card.status == "approved" && is_valid_story_thread_term(&card.title) {
            labels.insert(card.title.trim().to_string());
        }
    }
    for foreshadowing in state.list_foreshadowings(project_id)? {
        if foreshadowing.status != "pending_human_approval"
            && is_valid_story_thread_term(&foreshadowing.title)
        {
            labels.insert(foreshadowing.title.trim().to_string());
        }
    }
    for entry in state.list_continuity_ledger_entries(project_id)? {
        if is_valid_story_thread_term(&entry.entity_label) {
            labels.insert(entry.entity_label.trim().to_string());
        }
    }
    Ok(labels)
}

fn story_thread_term_priority(
    term: &StoryContextSnippet,
    registered_labels: &HashSet<String>,
) -> usize {
    let mut priority = if registered_labels.contains(term.matched_term.trim()) {
        1_000
    } else {
        0
    };
    if term.source_label.contains("角色") {
        priority += 120;
    } else if term.source_label.contains("设定") {
        priority += 100;
    } else if term.source_label.contains("大纲") {
        priority += 80;
    } else if term.source_label.starts_with("第") {
        priority += 60;
    }
    if looks_like_object_term(term.matched_term.trim()) {
        priority += 20;
    }
    priority
}

fn is_valid_story_thread_term(term: &str) -> bool {
    let term = term.trim();
    let length = term.chars().count();
    if !(2..=12).contains(&length) || !term.chars().all(is_han) || is_noise_term(term) {
        return false;
    }

    // A derived story thread is an entity or event label, never an outline
    // instruction, chapter range, or prose sentence.
    [
        "章节目标",
        "核心冲突",
        "信息释放",
        "结尾钩子",
        "退出状态",
        "下一章",
        "上一章",
        "本章",
        "章纲",
        "必须",
        "不得",
        "不要",
        "建议",
        "需要",
        "重点",
        "优先",
        "推进",
        "完成",
        "释放",
        "交代",
        "避免",
        "只揭",
        "模式",
    ]
    .iter()
    .all(|marker| !term.contains(marker))
}

fn classify_thread_kind(term: &str, source_label: &str) -> &'static str {
    if source_label.contains("角色") {
        "character"
    } else if term.chars().any(|ch| ch.is_ascii_digit()) || term.contains('-') {
        "object"
    } else if source_label.contains("设定") {
        "rule"
    } else {
        "fact"
    }
}

fn snippet_used_in_artifact(content: &str, snippet: &StoryContextSnippet) -> bool {
    content.contains(snippet.matched_term.as_str())
}

fn snippet_resolved_in_artifact(content: &str, snippet: &StoryContextSnippet) -> bool {
    if let Some(index) = content.find(snippet.matched_term.as_str()) {
        let window = excerpt_around(content, index, snippet.matched_term.chars().count(), 80);
        return ["原来", "终于", "确认", "就是", "证实", "真是"]
            .iter()
            .any(|marker| window.contains(marker));
    }
    false
}

fn extract_primary_cost(content: &str) -> Option<String> {
    for needle in [
        "代价", "发烫", "疼", "流血", "不能", "不该", "风险", "威胁", "选择", "后果",
    ] {
        if let Some(index) = content.find(needle) {
            return Some(excerpt_around(content, index, needle.chars().count(), 90));
        }
    }
    None
}

fn collect_current_costs(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
) -> AppResult<Vec<String>> {
    let chapters = state.list_chapters(project_id)?;
    let previous_ids = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < chapter_no)
        .rev()
        .take(2)
        .map(|chapter| chapter.id)
        .collect::<Vec<_>>();

    let mut costs = Vec::new();
    for chapter_id in previous_ids {
        if let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter_id)? {
            for needle in [
                "代价", "发烫", "归档", "不能", "不该", "选择", "风险", "威胁", "别开", "别信",
            ] {
                if let Some(index) = artifact.content.find(needle) {
                    costs.push(excerpt_around(
                        &artifact.content,
                        index,
                        needle.chars().count(),
                        90,
                    ));
                    break;
                }
            }
        }
    }
    costs.truncate(3);
    Ok(costs)
}

fn collect_character_anchors(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
) -> AppResult<Vec<String>> {
    let mut anchors = Vec::new();
    let chapters = state.list_chapters(project_id)?;
    let current_chapter_no = chapters
        .iter()
        .find(|chapter| chapter.id == current_chapter_id)
        .map(|chapter| chapter.chapter_no)
        .unwrap_or_default();

    for chapter in chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < current_chapter_no)
        .rev()
        .take(3)
    {
        if let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter.id)? {
            for name in extract_character_like_terms(&artifact.content)
                .into_iter()
                .take(4)
            {
                if let Some(index) = artifact.content.find(&name) {
                    anchors.push(format!(
                        "{} 在《{}》最后已表现为：{}",
                        name,
                        chapter.title,
                        excerpt_around(&artifact.content, index, name.chars().count(), 90)
                    ));
                }
            }
        }
    }

    anchors.dedup();
    anchors.truncate(4);
    Ok(anchors)
}

fn collect_object_anchors(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
) -> AppResult<Vec<String>> {
    let mut anchors = Vec::new();
    let chapters = state.list_chapters(project_id)?;
    let current_chapter_no = chapters
        .iter()
        .find(|chapter| chapter.id == current_chapter_id)
        .map(|chapter| chapter.chapter_no)
        .unwrap_or_default();

    for chapter in chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < current_chapter_no)
        .rev()
        .take(3)
    {
        if let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter.id)? {
            for term in extract_object_like_terms(&artifact.content)
                .into_iter()
                .take(6)
            {
                if let Some(index) = artifact.content.find(&term) {
                    anchors.push(format!(
                        "{} 在《{}》最近状态：{}",
                        term,
                        chapter.title,
                        excerpt_around(&artifact.content, index, term.chars().count(), 80)
                    ));
                }
            }
        }
    }

    anchors.dedup();
    anchors.truncate(5);
    Ok(anchors)
}

fn extract_object_like_terms(text: &str) -> Vec<String> {
    let mut terms = split_query_tokens(text)
        .into_iter()
        .filter(|token| token.chars().count() >= 2 && token.chars().count() <= 6)
        .filter(|token| token.chars().all(is_han))
        .filter(|token| !is_noise_term(token))
        .filter(|token| !looks_like_character_name(token))
        .filter(|token| object_context_score(text, token) >= 2)
        .collect::<Vec<_>>();
    terms.sort_by(|a, b| {
        object_context_score(text, b)
            .cmp(&object_context_score(text, a))
            .then_with(|| b.chars().count().cmp(&a.chars().count()))
    });
    terms.dedup();
    terms.truncate(12);
    terms
}

fn object_context_score(text: &str, term: &str) -> usize {
    let mut score = 0;
    let mut search_start = 0;

    while let Some(relative_index) = text[search_start..].find(term) {
        let index = search_start + relative_index;
        let excerpt = excerpt_around(text, index, term.chars().count(), 48);
        score += count_any(
            &excerpt,
            &[
                "拿", "握", "塞", "藏", "收", "递", "贴", "按", "刻", "亮", "裂", "碎", "门", "锁",
                "钥", "牌", "令", "骨", "符", "阵", "药", "丹", "炉", "灯", "镜", "册", "纸", "血",
                "纹", "灰", "石", "匣", "珠", "剑", "刀", "枪", "衣", "袋", "柜", "瓶", "盒", "图",
                "印", "禁",
            ],
        );
        search_start = index + term.len();
        if score >= 8 {
            break;
        }
    }

    score
}

fn count_any(text: &str, needles: &[&str]) -> usize {
    needles
        .iter()
        .map(|needle| text.matches(needle).count())
        .sum()
}

fn collect_unresolved_hooks(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
) -> AppResult<Vec<String>> {
    let chapters = state.list_chapters(project_id)?;
    let previous_ids = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < chapter_no)
        .rev()
        .take(2)
        .map(|chapter| (chapter.id, chapter.title.clone()))
        .collect::<Vec<_>>();
    let triggers = [
        "追",
        "封",
        "盯",
        "回来",
        "时限",
        "倒计时",
        "威胁",
        "不能",
        "必须",
        "否则",
        "来不及",
        "发现",
        "决定",
    ];
    let mut hooks = Vec::new();

    for (chapter_id, title) in previous_ids {
        if let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter_id)? {
            let tail = tail_excerpt(&artifact.content, 220);
            for trigger in triggers {
                if let Some(index) = tail.find(trigger) {
                    hooks.push(format!(
                        "《{}》尾段未结压力：{}",
                        title,
                        excerpt_around(&tail, index, trigger.chars().count(), 90)
                    ));
                    break;
                }
            }
        }
    }

    hooks.truncate(3);
    Ok(hooks)
}

fn extract_character_like_terms(text: &str) -> Vec<String> {
    let mut terms = split_query_tokens(text)
        .into_iter()
        .filter(|token| token.chars().count() >= 2 && token.chars().count() <= 4)
        .filter(|token| token.chars().all(is_han))
        .filter(|token| !is_noise_term(token))
        .filter(|token| !looks_like_object_term(token))
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms.truncate(6);
    terms
}

fn looks_like_character_name(term: &str) -> bool {
    term.ends_with("执事")
        || term.ends_with("师兄")
        || term.ends_with("师姐")
        || term.ends_with("长老")
        || term.ends_with("先生")
        || term.ends_with("小姐")
        || term.ends_with("姑娘")
        || term.ends_with("兄")
        || term.ends_with("姐")
}

fn looks_like_object_term(term: &str) -> bool {
    [
        "牌", "令", "骨", "符", "阵", "丹", "药", "炉", "门", "灯", "镜", "册", "纸", "血", "纹",
        "灰", "石", "匣", "珠", "剑", "刀", "枪", "衣", "袋", "柜", "瓶", "盒", "图", "印", "锁",
        "钥",
    ]
    .iter()
    .any(|suffix| term.ends_with(suffix) || term.contains(suffix))
}

fn project_context(project: &Project) -> String {
    format!(
        "# 项目\n标题：{}\n类型：{}\n预计总字数（仅供整体节奏规划）：{}\n状态：{}\n核心想法：{}",
        project.title, project.genre, project.target_words, project.status, project.premise
    )
}

fn tail_excerpt(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    chars[chars.len() - max_chars..].iter().collect()
}

fn head_excerpt(text: &str, max_chars: usize) -> String {
    text.chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn approved_outline_section_for_chapter(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
) -> AppResult<Option<String>> {
    let Some(outline) = state.approved_artifact(project_id, "outline", None)? else {
        return Ok(None);
    };
    let lines = outline.content.lines().collect::<Vec<_>>();
    let targets = [
        format!("第{}章", chapter_no),
        format!("第 {} 章", chapter_no),
        format!("第{}章：", chapter_no),
        format!("第 {} 章：", chapter_no),
    ];

    let Some(start) = lines.iter().position(|line| {
        let trimmed = line.trim();
        targets.iter().any(|target| trimmed.contains(target))
    }) else {
        return Ok(None);
    };

    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = line.trim();
        if trimmed.starts_with("## 第") || trimmed.starts_with("### 第") {
            end = index;
            break;
        }
    }

    let section = lines[start..end].join("\n").trim().to_string();
    if section.is_empty() {
        Ok(None)
    } else {
        Ok(Some(section))
    }
}

fn parent_for_stage(
    state: &AppState,
    project_id: i64,
    stage: &Stage,
    chapter_id: Option<i64>,
) -> AppResult<Option<i64>> {
    match stage {
        Stage::Review => {
            if let Some(chapter_id) = chapter_id {
                Ok(state
                    .latest_approved_chapter_body(project_id, chapter_id)?
                    .map(|artifact| artifact.id))
            } else {
                Ok(None)
            }
        }
        Stage::Revision => Ok(state
            .latest_artifact(project_id, "revision", chapter_id)?
            .or_else(|| {
                state
                    .latest_artifact(project_id, "draft", chapter_id)
                    .ok()
                    .flatten()
            })
            .map(|artifact| artifact.id)),
        _ => Ok(None),
    }
}

fn resolve_revision_source(
    state: &AppState,
    project_id: i64,
    chapter_id: Option<i64>,
    source_artifact: Option<&Artifact>,
) -> AppResult<Option<Artifact>> {
    if let Some(source) = source_artifact {
        if source.stage == "review" {
            let chapter_id = source.chapter_id.or(chapter_id).ok_or_else(|| {
                AppError::Validation("试读报告缺少章节信息，无法发起修订".to_string())
            })?;
            if let Some(parent_id) = source.parent_artifact_id {
                let parent = state.get_artifact(parent_id)?;
                if parent.project_id == project_id
                    && parent.chapter_id == Some(chapter_id)
                    && (parent.stage == "draft" || parent.stage == "revision")
                {
                    return Ok(Some(parent));
                }
            }
            return latest_chapter_candidate(state, project_id, chapter_id);
        }
        return Ok(Some(source.clone()));
    }

    if let Some(chapter_id) = chapter_id {
        return latest_chapter_candidate(state, project_id, chapter_id);
    }

    Ok(None)
}

fn latest_chapter_candidate(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<Option<Artifact>> {
    Ok(state
        .latest_artifact(project_id, "revision", Some(chapter_id))?
        .or(state.latest_artifact(project_id, "draft", Some(chapter_id))?))
}

fn quality_report_for_prompt(report: &crate::models::QualityReport) -> String {
    let warning_lines = report
        .warnings
        .iter()
        .take(4)
        .map(|warning| {
            format!(
                "- {}：{} 建议：{}",
                warning.title, warning.detail, warning.suggestion
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "评分：{} / 100\n结论：{}\n摘要：{}\n{}",
        report.score, report.verdict, report.summary, warning_lines
    )
}

fn revision_contract_for_prompt(report: &crate::models::QualityReport) -> String {
    let warning_titles = report
        .warnings
        .iter()
        .map(|warning| warning.title.as_str())
        .collect::<Vec<_>>();
    let has_reveal_overload = warning_titles
        .iter()
        .any(|title| title.contains("信息反转") || title.contains("信息释放"));
    let has_weak_ending = warning_titles
        .iter()
        .any(|title| title.contains("结尾功能"));
    let has_low_dialogue = warning_titles.iter().any(|title| title.contains("对白"));
    let has_overlength = warning_titles
        .iter()
        .any(|title| title.contains("叙事失焦"));

    let mut lines = vec![
        "- 修订不是续写：不得为了抬高悬念而新增未在原稿、试读报告或人工反馈中出现的新组织、新法宝、新名单、新规则、新称号、新倒计时。".to_string(),
        "- 保留清单：保留本章已经成立的主动作、已出现的关键人物、已出现的关键物件和已批准前文的连续状态。".to_string(),
        "- 删除清单：删除只负责解释机制、扩展世界观、补作者说明的段落；删除不能立刻改变主角选择的次要反转。".to_string(),
    ];

    if has_reveal_overload {
        lines.push("- 信息过载硬约束：整章只允许 1 条核心确认；其他真相必须降级成疑点、物证、未确认线索或角色隐瞒。禁止使用“第一/第二/第三”列举结论，禁止连续使用“不是……而是……”讲机制。".to_string());
        lines.push("- 禁止新增事实：不要新增新的幕后层级、第二套关键物件、额外持有人、额外名单、额外身份反转；若需要悬念，只能回收原稿已有的人、物、时限或地点。".to_string());
    }
    if has_weak_ending {
        lines.push("- 结尾功能约束：按本章模式落稳已经形成的结果、决定、信息改写、关系变化、行动启动或情绪余波；不强制危险、反转或悬念句。".to_string());
        lines.push("- 事实边界：需要延续压力时，只能推进前文已经出现的时限、监视、封锁、伤势、资源或角色命令；不要为了修结尾新增人物、物件、身份或规则。".to_string());
    }
    if has_low_dialogue {
        lines.push("- 对白判断：先判断人物互动是否承担本章主要变化。若承担，应把已有互动改成能改变局面的对白或潜台词，例如逼出条件、暴露隐瞒、迫使让步或换取筹码；若本章主要是独处修炼、恢复、探索或行动执行，不为指标硬塞第二个人和对白对撞。".to_string());
    }
    if has_overlength {
        lines.push("- 叙事失焦约束：不要求缩短正文。保留必要的场景、对白和情绪承接；只删重复确认、二次解释、低信息量过桥段和同功能的第二段氛围描写。".to_string());
        lines.push("- 收束优先级：先删一条次级信息、一个不会立刻改变主角选择的补充事实、或一段只负责把同一压力再说一遍的过渡；不要为了缩短而跳过场景转折或人物反应。".to_string());
    }

    lines.join("\n")
}

fn continuity_candidate_artifacts(
    state: &AppState,
    project_id: i64,
    candidate_artifact_id: Option<i64>,
    candidate_artifact_ids: &[i64],
) -> AppResult<Vec<Artifact>> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    if let Some(id) = candidate_artifact_id {
        if seen.insert(id) {
            ids.push(id);
        }
    }
    for id in candidate_artifact_ids {
        if seen.insert(*id) {
            ids.push(*id);
        }
    }

    let mut artifacts = Vec::new();
    for id in ids {
        let artifact = state.get_artifact(id)?;
        if artifact.project_id != project_id {
            return Err(AppError::Validation("候选稿不属于当前项目".to_string()));
        }
        if artifact.chapter_id.is_none()
            || (artifact.stage != "draft" && artifact.stage != "revision")
        {
            return Err(AppError::Validation(
                "连续性候选审校只支持章节草稿或修订稿".to_string(),
            ));
        }
        artifacts.push(artifact);
    }

    artifacts.sort_by_key(|artifact| artifact.id);
    Ok(artifacts)
}

fn continuity_cache_key(
    project_id: i64,
    chapter_ids: &[i64],
    candidate_artifact_ids: &[i64],
) -> String {
    let chapters = chapter_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut candidates = candidate_artifact_ids.to_vec();
    candidates.sort_unstable();
    let candidate_part = if candidates.is_empty() {
        "approved-only".to_string()
    } else {
        candidates
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "review=v2|project={}|candidate={}|chapters={}",
        project_id, candidate_part, chapters
    )
}

fn find_cached_continuity_report(
    state: &AppState,
    project_id: i64,
    cache_key: &str,
    chapter_titles: Vec<String>,
) -> AppResult<Option<ContinuityReport>> {
    let marker = format!("# continuity-cache-key: {}", cache_key);
    let cached = state
        .list_workflow_runs(project_id)?
        .into_iter()
        .find(|run| {
            run.stage == "continuity_review"
                && run.status == "success"
                && run.input.starts_with(&marker)
        });

    Ok(cached.map(|run| {
        continuity_report_from_issues(
            project_id,
            chapter_titles,
            normalize_continuity_issues(parse_continuity_issues(&run.output)),
        )
    }))
}

fn continuity_report_from_issues(
    project_id: i64,
    chapter_titles: Vec<String>,
    issues: Vec<ContinuityIssue>,
) -> ContinuityReport {
    let summary = if issues.is_empty() {
        "连续性审校未发现明显的跨章硬伤，但仍建议结合人工审读继续验证多章节推进能力。".to_string()
    } else {
        format!(
            "共发现 {} 个跨章问题，最需要优先处理的是：{}",
            issues.len(),
            issues
                .iter()
                .take(3)
                .map(|issue| issue.issue_type.as_str())
                .collect::<Vec<_>>()
                .join("、")
        )
    };
    let verdict = if issues.iter().any(|issue| issue.severity == "major") {
        "needs_revision"
    } else if issues.is_empty() {
        "strong"
    } else {
        "usable"
    }
    .to_string();

    ContinuityReport {
        project_id,
        chapter_titles,
        verdict,
        summary,
        issues,
    }
}

fn parse_continuity_issues(raw: &str) -> Vec<ContinuityIssue> {
    let trimmed = raw.trim();
    if let Ok(items) = serde_json::from_str::<Vec<ContinuityIssue>>(trimmed) {
        return items;
    }
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if let Ok(items) = serde_json::from_str::<Vec<ContinuityIssue>>(&trimmed[start..=end]) {
                return items;
            }
        }
    }
    Vec::new()
}

fn carryover_obligations_from_excerpt(text: &str) -> Vec<String> {
    let tail = tail_excerpt(text, 420);
    let mut obligations = Vec::new();

    if contains_any(
        &tail,
        &["药效", "药力", "反噬", "伤势", "发麻", "失去知觉", "勒住"],
    ) {
        obligations.push(
            "上章遗留的药效、伤势或反噬必须在本章前段持续有身体感，不能到中段才突然重新想起。"
                .to_string(),
        );
    }
    if contains_any(
        &tail,
        &["反光", "监视", "盯", "跟", "搜", "火把", "脚步", "看了一眼"],
    ) {
        obligations.push(
            "上章已经出现监视、跟踪或搜查痕迹，本章中段至少要回收一次对方存在感，不能让这条压力直接蒸发。"
                .to_string(),
        );
    }
    if contains_any(&tail, &["今晚", "子时", "天亮", "明晚", "时辰", "必须"]) {
        obligations.push(
            "上章已建立明确时限，本章开头就要让读者知道这条时限还在流逝，并影响主角选择。"
                .to_string(),
        );
    }
    if contains_any(&tail, &["一定会下", "下炉", "开门", "先下去", "旧炉"]) {
        obligations.push(
            "若上章已经判断敌手先行或门已被动过，本章必须交代主角为何迟一步、绕路或改策略。"
                .to_string(),
        );
    }

    obligations
}

fn parse_chapter_split_plan(
    raw: &str,
    project_id: i64,
    chapter_id: i64,
    artifact_id: i64,
    fallback_current_title: &str,
    fallback_next_title: &str,
) -> AppResult<ChapterSplitPlan> {
    let value = parse_json_object(raw)
        .ok_or_else(|| AppError::Validation("拆章方案返回不是有效 JSON 对象".to_string()))?;

    let suggested_current_title = json_string_field(&value, "suggested_current_title")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback_current_title.to_string());
    let suggested_next_title = json_string_field(&value, "suggested_next_title")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback_next_title.to_string());
    let rationale = json_string_field(&value, "rationale").unwrap_or_else(|| {
        "当前候选稿单章负载偏高，需要把最影响主角下一步选择的一部分信息后移。".to_string()
    });
    let current_chapter_mission = json_string_field(&value, "current_chapter_mission")
        .unwrap_or_else(|| {
            "把当前章收束成一个更单一的章节功能，并保留最关键的当前压力。".to_string()
        });
    let next_chapter_mission = json_string_field(&value, "next_chapter_mission")
        .unwrap_or_else(|| "承接当前章结尾状态，继续处理被后移的信息和后续动作。".to_string());
    let keep_in_current = json_string_list_field(&value, "keep_in_current");
    let move_to_next = json_string_list_field(&value, "move_to_next");
    let carryover_closing_beats = json_string_list_field(&value, "carryover_closing_beats");
    let next_chapter_opening_beats = json_string_list_field(&value, "next_chapter_opening_beats");
    let revision_prompt_current =
        json_string_field(&value, "revision_prompt_current").unwrap_or_else(|| {
            format!(
                "把当前章收束为：{}。保留这些内容：{}。移走这些内容：{}。章末必须接住当前压力，不要再继续堆新的规则解释。",
                current_chapter_mission,
                fallback_bullets(&keep_in_current, "保留最直接的主动作线"),
                fallback_bullets(&move_to_next, "把过密信息后移到下一章")
            )
        });
    let next_chapter_instruction = json_string_field(&value, "next_chapter_instruction")
        .unwrap_or_else(|| {
            format!(
                "写下一章正文，章节目标是：{}。开头先承接上一章章末状态，再推进这些内容：{}。",
                next_chapter_mission,
                fallback_bullets(
                    &next_chapter_opening_beats,
                    "承接上一章章末压力并推进后移内容"
                )
            )
        });

    Ok(ChapterSplitPlan {
        project_id,
        chapter_id,
        artifact_id,
        suggested_current_title,
        suggested_next_title,
        rationale,
        current_chapter_mission,
        next_chapter_mission,
        keep_in_current,
        move_to_next,
        carryover_closing_beats,
        next_chapter_opening_beats,
        revision_prompt_current,
        next_chapter_instruction,
    })
}

fn normalize_continuity_issues(issues: Vec<ContinuityIssue>) -> Vec<ContinuityIssue> {
    issues
        .into_iter()
        .map(|mut issue| {
            if should_soften_same_scene_issue(&issue) && issue.severity == "major" {
                issue.severity = "moderate".to_string();
            }
            issue
        })
        .collect()
}

fn should_soften_same_scene_issue(issue: &ContinuityIssue) -> bool {
    let issue_text = format!("{} {} {}", issue.issue_type, issue.reason, issue.suggestion);
    let same_scene_signal = issue_text.contains("同场景")
        || issue_text.contains("即时状态")
        || issue_text.contains("场景衔接")
        || issue_text.contains("氛围延续")
        || issue_text.contains("对白延续")
        || issue_text.contains("直接续接");
    let hard_contradiction_signal = issue_text.contains("自相矛盾")
        || issue_text.contains("硬伤")
        || issue_text.contains("伤势前后不一致")
        || issue_text.contains("位置前后不一致")
        || issue_text.contains("门已开合状态矛盾")
        || issue_text.contains("追逐关系矛盾")
        || issue_text.contains("事实冲突");

    same_scene_signal && !hard_contradiction_signal
}

fn parse_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if value.is_object() {
            return Some(value);
        }
    }
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if let Ok(value) = serde_json::from_str::<Value>(&trimmed[start..=end]) {
                if value.is_object() {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn json_string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|item| item.as_str())
        .map(|item| item.trim().to_string())
}

fn json_string_list_field(value: &Value, field: &str) -> Vec<String> {
    let Some(item) = value.get(field) else {
        return Vec::new();
    };

    if let Some(items) = item.as_array() {
        return items
            .iter()
            .filter_map(|entry| entry.as_str())
            .map(|entry| entry.trim().trim_start_matches('-').trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect();
    }

    if let Some(text) = item.as_str() {
        return text
            .lines()
            .map(|line| line.trim().trim_start_matches('-').trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
    }

    Vec::new()
}

fn fallback_bullets(items: &[String], fallback: &str) -> String {
    if items.is_empty() {
        fallback.to_string()
    } else {
        items.join("；")
    }
}

fn artifact_title(stage: &Stage, chapter_no: Option<i64>) -> String {
    match chapter_no {
        Some(no) => format!("第 {no} 章 {}", stage.title()),
        None => stage.title().to_string(),
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

fn export_role_label(role: &str) -> &'static str {
    match role {
        "human_instruction" => "人工指令",
        "revision_feedback" => "修订要求",
        "approval_note" => "人工确认",
        "agent_result" => "Agent 结果",
        _ => "记录",
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use tempfile::NamedTempFile;

    use super::*;
    use crate::models::{
        ChapterUpdate, NewChapter, NewProject, RunAgentRequest, SaveAiSettings,
        SpanReplacementRequest,
    };

    #[test]
    fn draft_requires_approved_foundation() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "书".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120000,
                premise: "一句话".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);

        let err = validate_prerequisites(&state, project.id, &Stage::Draft, Some(chapter.id), None)
            .unwrap_err()
            .to_string();

        assert!(err.contains("设定"));
    }

    #[test]
    fn specialist_agent_ignores_skills_outside_its_allowlist() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "Skill 隔离".to_string(),
                genre: "悬疑".to_string(),
                target_words: 120000,
                premise: "测试白名单".to_string(),
            })
            .unwrap();
        state
            .save_writing_skill(crate::models::SaveWritingSkill {
                id: None,
                skill_key: "unrelated_romance_craft".to_string(),
                name: "无关言情规则".to_string(),
                category: "craft".to_string(),
                description: "不应进入悬疑 Agent".to_string(),
                content: "## Always\nUNRELATED_SKILL_MARKER".to_string(),
                enabled: true,
            })
            .unwrap();

        let rendered = render_supporting_skills(&state, &project, &Stage::Draft).unwrap();

        assert!(rendered.contains("continuity_and_agency"));
        assert!(!rendered.contains("UNRELATED_SKILL_MARKER"));
    }

    #[test]
    fn urban_supernatural_and_mystery_prompts_do_not_cross_load() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let urban_project = state
            .create_project(NewProject {
                title: "都市异能隔离".to_string(),
                genre: "都市异能".to_string(),
                target_words: 120000,
                premise: "测试题材隔离".to_string(),
            })
            .unwrap();
        let mystery_project = state
            .create_project(NewProject {
                title: "悬疑隔离".to_string(),
                genre: "悬疑".to_string(),
                target_words: 120000,
                premise: "测试题材隔离".to_string(),
            })
            .unwrap();

        let urban = render_project_genre_skill(&state, &urban_project, &Stage::Draft).unwrap();
        let mystery = render_project_genre_skill(&state, &mystery_project, &Stage::Draft).unwrap();

        assert!(urban.contains("能力通过行动、限制和后果"));
        assert!(!urban.contains("关键结论必须有可追溯线索支持"));
        assert!(mystery.contains("关键结论必须有可追溯线索支持"));
        assert!(!mystery.contains("能力通过行动、限制和后果"));
    }

    #[test]
    fn pending_candidate_does_not_pollute_story_threads() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "候选稿账本隔离".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let setting = state
            .insert_artifact(project.id, None, "setting", "设定", "周延掌管矿场。", None)
            .unwrap();
        state
            .approve_stage(project.id, "setting", setting.id, "")
            .unwrap();
        let chapter = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第二章".to_string()),
            })
            .unwrap();
        let candidate = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "草稿",
                "周延在矿场外停下脚步。",
                None,
            )
            .unwrap();

        sync_story_threads_from_artifact(&state, &candidate).unwrap();
        assert!(state.list_story_threads(project.id).unwrap().is_empty());
    }

    #[test]
    fn story_thread_filter_keeps_entity_labels_and_rejects_outline_noise() {
        for label in ["黑牌", "矿洞坍塌", "七号旧炉"] {
            assert!(
                is_valid_story_thread_term(label),
                "{label} should be a label"
            );
        }
        for label in [
            "1-2",
            "章节目标",
            "本章必须推进",
            "主角必须在执事和同门盯梢下完成试探",
        ] {
            assert!(
                !is_valid_story_thread_term(label),
                "{label} should be rejected"
            );
        }
    }

    #[test]
    fn rebuilt_story_threads_ignore_pending_candidates_and_feedback() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        let state = AppState::from_path(path.clone()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "账本重建隔离".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter_one = state.list_chapters(project.id).unwrap().remove(0);
        let approved = state
            .insert_artifact(
                project.id,
                Some(chapter_one.id),
                "draft",
                "第一章",
                "林缺收起了青铜令牌，决定明日进山。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", approved.id, "")
            .unwrap();
        let chapter_two = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第二章".to_string()),
            })
            .unwrap();
        state
            .insert_artifact(
                project.id,
                Some(chapter_two.id),
                "revision",
                "候选修订",
                "林缺用九枚虚构血晶预付了天外商会的通行税。",
                None,
            )
            .unwrap();
        state
            .insert_message(
                project.id,
                Some(chapter_two.id),
                "revision_feedback",
                "删除九枚虚构血晶和天外商会的设定。",
            )
            .unwrap();
        drop(state);

        let reopened = AppState::from_path(path).unwrap();
        let threads = reopened.list_story_threads(project.id).unwrap();
        assert!(threads.iter().all(|thread| {
            !thread.notes.contains("虚构血晶")
                && !thread.notes.contains("天外商会")
                && !thread.notes.contains("删除九枚")
        }));
    }

    #[test]
    fn local_span_replacement_creates_revision_without_overwriting_source() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "局部替换测试".to_string(),
                genre: "男频修仙爽文".to_string(),
                target_words: 120000,
                premise: "测试局部替换".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let source = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "草稿",
                "他推开炉门。\n火光扑面。\n他退后一步。",
                None,
            )
            .unwrap();

        let result = replace_artifact_span(
            &state,
            SpanReplacementRequest {
                project_id: project.id,
                artifact_id: source.id,
                find_text: "火光扑面。".to_string(),
                replace_text: "火光扑面，黑灰里露出半枚裂纹玉牌。".to_string(),
                note: Some("只替换中段发现。".to_string()),
            },
        )
        .unwrap();

        let unchanged_source = state.get_artifact(source.id).unwrap();
        assert_eq!(unchanged_source.content, source.content);
        assert_eq!(result.artifact.stage, "revision");
        assert_eq!(result.artifact.parent_artifact_id, Some(source.id));
        assert!(result.artifact.content.contains("裂纹玉牌"));
        assert!(!result.artifact.content.contains("火光扑面。\n"));
    }

    #[test]
    fn revision_from_pending_review_uses_the_reviewed_candidate() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "试读修订关联".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120000,
                premise: "验证试读后修订".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        let draft = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "草稿",
                "正文候选稿",
                None,
            )
            .unwrap();
        let review = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "review",
                "试读报告",
                "[]",
                Some(draft.id),
            )
            .unwrap();

        let resolved = resolve_revision_source(&state, project.id, Some(chapter.id), Some(&review))
            .unwrap()
            .unwrap();

        assert_eq!(resolved.id, draft.id);
    }

    #[test]
    fn revision_fact_boundary_forbids_invented_evidence() {
        assert!(REVISION_FACT_BOUNDARY.contains("隐藏物件/证据"));
        assert!(REVISION_FACT_BOUNDARY.contains("原来早有"));
        assert!(REVISION_FACT_BOUNDARY.contains("不得编造补丁"));
    }

    #[test]
    fn review_fact_boundary_requires_major_issue_for_invented_history() {
        assert!(REVIEW_FACT_BOUNDARY.contains("事实越界"));
        assert!(REVIEW_FACT_BOUNDARY.contains("severity 为 major"));
        assert!(REVIEW_FACT_BOUNDARY.contains("不得建议编造"));
    }

    #[test]
    fn review_issue_without_verifiable_evidence_is_discarded() {
        let issues = constrain_review_issues(
            vec![ReviewIssue {
                issue_type: "钩子不足".to_string(),
                severity: "major".to_string(),
                location: "章末".to_string(),
                reason: "结尾停在等待。".to_string(),
                suggestion: "让主角发现一件旧物。".to_string(),
                evidence_quote: "不存在的来源引文".to_string(),
                action_evidence_quote: "".to_string(),
            }],
            "当前稿只有既有内容。",
        );

        assert!(issues.is_empty());
    }

    #[test]
    fn review_suggestion_with_verifiable_evidence_is_kept() {
        let issues = constrain_review_issues(
            vec![ReviewIssue {
                issue_type: "钩子不足".to_string(),
                severity: "moderate".to_string(),
                location: "章末".to_string(),
                reason: "结尾停在等待。".to_string(),
                suggestion: "推进既有倒计时。".to_string(),
                evidence_quote: "距离黑市明晚开市".to_string(),
                action_evidence_quote: "林缺得知距离黑市明晚开市".to_string(),
            }],
            "林缺得知距离黑市明晚开市，但灵石不够。",
        );

        assert_eq!(issues[0].suggestion, "推进既有倒计时。");
        assert_eq!(issues[0].evidence_quote, "距离黑市明晚开市");
        assert_eq!(issues[0].action_evidence_quote, "林缺得知距离黑市明晚开市");
        assert!(!evidence_quote_is_verifiable(
            &"甲".repeat(81),
            &"甲".repeat(81)
        ));
    }

    #[test]
    fn review_keeps_finding_but_quarantines_unbound_repair() {
        let issues = constrain_review_issues(
            vec![ReviewIssue {
                issue_type: "威胁断线".to_string(),
                severity: "moderate".to_string(),
                location: "探索中段".to_string(),
                reason: "已建立的监视压力在中段消失。".to_string(),
                suggestion: "加入一支火把和执夜弟子，让监视重新出现。".to_string(),
                evidence_quote: "赵执事盯着他看了一会儿，没动，也没说话".to_string(),
                action_evidence_quote: "他没有走谷口正路，而是绕到草棚后面".to_string(),
            }],
            "赵执事盯着他看了一会儿，没动，也没说话。他没有走谷口正路，而是绕到草棚后面。",
        );

        assert_eq!(issues.len(), 1);
        assert!(issues[0].suggestion.contains("已拦截"));
        assert!(issues[0].action_evidence_quote.is_empty());
    }

    #[test]
    fn review_quarantines_substitution_that_invents_a_new_clue() {
        let issues = constrain_review_issues(
            vec![ReviewIssue {
                issue_type: "线索重复".to_string(),
                severity: "moderate".to_string(),
                location: "铜门".to_string(),
                reason: "同一结论第三次确认。".to_string(),
                suggestion: "删除重复名字，铜门上的刻字可以是另一个失踪者姓名或新编号。"
                    .to_string(),
                evidence_quote: "铜门背面又刻着赵吞赤髓四个字".to_string(),
                action_evidence_quote: "".to_string(),
            }],
            "铜门背面又刻着赵吞赤髓四个字。前文已经两次确认相同结论。",
        );

        assert_eq!(issues.len(), 1);
        assert!(issues[0].suggestion.contains("已拦截"));
    }

    #[test]
    fn review_issue_allows_empty_action_evidence_for_deletion_or_reordering() {
        let issues = constrain_review_issues(
            vec![ReviewIssue {
                issue_type: "信息重复".to_string(),
                severity: "moderate".to_string(),
                location: "中段".to_string(),
                reason: "同一判断被重复解释。".to_string(),
                suggestion: "保留第一处解释，删去后面的重复说明。".to_string(),
                evidence_quote: "他已经看见柜门上的血字".to_string(),
                action_evidence_quote: "".to_string(),
            }],
            "他已经看见柜门上的血字，随后又把同一行血字逐字解释了一遍。",
        );

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].suggestion, "保留第一处解释，删去后面的重复说明。");
        assert!(issues[0].action_evidence_quote.is_empty());
    }

    #[test]
    fn hard_state_findings_come_from_the_app_ledger() {
        let model_issue = ReviewIssue {
            issue_type: "物件状态断点".to_string(),
            severity: "major".to_string(),
            location: "青瓷瓶".to_string(),
            reason: "模型误判半瓶与剩下半瓶冲突。".to_string(),
            suggestion: "改写数量。".to_string(),
            evidence_quote: "青瓷瓶里还剩下半瓶赤髓原浆".to_string(),
            action_evidence_quote: "陆烬把剩下半瓶原浆封好".to_string(),
        };
        assert!(model_issue_duplicates_ledger_check(&model_issue));

        let empty_report = crate::models::LedgerContinuityReport {
            project_id: 1,
            artifact_id: 2,
            summary: "未发现冲突".to_string(),
            issues: Vec::new(),
        };
        assert!(ledger_review_issues(Some(&empty_report)).is_empty());

        let report = crate::models::LedgerContinuityReport {
            project_id: 1,
            artifact_id: 2,
            summary: "发现冲突".to_string(),
            issues: vec![crate::models::LedgerContinuityIssue {
                severity: "major".to_string(),
                entity_label: "裂纹玉牌".to_string(),
                entity_kind: "item".to_string(),
                state_kind: "availability".to_string(),
                candidate_quote: "他再次催动早已碎掉的裂纹玉牌".to_string(),
                source_chapter: "第 3 章".to_string(),
                source_quote: "裂纹玉牌在掌心彻底碎成粉末".to_string(),
                reason: "已毁物件被直接使用".to_string(),
                suggestion: "删除使用动作，或使用正文已经存在的替代物。".to_string(),
            }],
        };
        let issues = ledger_review_issues(Some(&report));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, "状态账本冲突/availability");
        assert!(issues[0].reason.contains("第 3 章"));
    }

    #[test]
    fn review_evidence_normalizes_common_quote_punctuation() {
        assert!(evidence_quote_is_verifiable(
            "“柜门上的血字：不要回头。”",
            "柜门上的血字：不要回头。",
        ));
    }

    #[test]
    fn manual_context_search_includes_immediate_preceding_chapter() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "检索范围测试".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120000,
                premise: "验证章节历史检索范围。".to_string(),
            })
            .unwrap();
        let first_chapter = state.list_chapters(project.id).unwrap().remove(0);
        let first_body = state
            .insert_artifact(
                project.id,
                Some(first_chapter.id),
                "draft",
                "第 1 章草稿",
                "林缺将赤纹古轴藏进袖中，决定明夜去黑市。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", first_body.id, "通过")
            .unwrap();
        let second_chapter = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第 2 章".to_string()),
            })
            .unwrap();

        let automatic = search_story_context(
            &state,
            StoryContextSearchInput {
                project_id: project.id,
                chapter_id: Some(second_chapter.id),
                query: "赤纹古轴".to_string(),
                limit: Some(8),
                include_immediate_previous: false,
            },
        )
        .unwrap();
        assert!(!automatic
            .iter()
            .any(|snippet| snippet.source_label.contains("第 1 章")));

        let manual = search_story_context(
            &state,
            StoryContextSearchInput {
                project_id: project.id,
                chapter_id: Some(second_chapter.id),
                query: "赤纹古轴".to_string(),
                limit: Some(8),
                include_immediate_previous: true,
            },
        )
        .unwrap();
        assert!(manual
            .iter()
            .any(|snippet| snippet.source_label.contains("第 1 章")));
    }

    #[test]
    fn same_scene_continuity_issue_is_softened_to_moderate() {
        let issues = normalize_continuity_issues(vec![ContinuityIssue {
            issue_type: "同场景衔接".to_string(),
            severity: "major".to_string(),
            chapters: vec!["第一章".to_string(), "第二章".to_string()],
            reason: "两章明显是直接续接，但对白气口和紧张氛围像重开了一幕。".to_string(),
            suggestion: "补一个承接动作或上一句对白的尾音，让即时状态接上。".to_string(),
        }]);

        assert_eq!(issues[0].severity, "moderate");
    }

    #[test]
    fn hard_same_scene_contradiction_stays_major() {
        let issues = normalize_continuity_issues(vec![ContinuityIssue {
            issue_type: "即时状态延续".to_string(),
            severity: "major".to_string(),
            chapters: vec!["第一章".to_string(), "第二章".to_string()],
            reason: "同一场景直接续接时，角色伤势前后不一致，属于明确硬伤和事实冲突。".to_string(),
            suggestion: "统一伤势、站位和门的开合状态。".to_string(),
        }]);

        assert_eq!(issues[0].severity, "major");
    }

    #[test]
    fn parses_split_plan_from_wrapped_json_object() {
        let raw = r#"好的，下面是结果：
        {
          "suggested_current_title":"第 3 章 炉底开门",
          "suggested_next_title":"第 4 章 血书指路",
          "rationale":"当前章同时承担开门、探查、发现出口和多层信息解释，单章负载偏高。",
          "current_chapter_mission":"把当前章收束成开门并确认危险。",
          "next_chapter_mission":"承接开门后的发现与出口选择。",
          "keep_in_current":["开门动作","当前危险"],
          "move_to_next":["出口信息","后续取舍"],
          "carryover_closing_beats":["门开后的异常反应"],
          "next_chapter_opening_beats":["承接门开后的状态"],
          "revision_prompt_current":"重写当前章，只保留开门和危险。",
          "next_chapter_instruction":"写下一章，承接门开后的异常与出口选择。"
        }"#;

        let plan = parse_chapter_split_plan(raw, 1, 2, 3, "第 3 章", "第 4 章").unwrap();

        assert_eq!(plan.suggested_current_title, "第 3 章 炉底开门");
        assert_eq!(plan.keep_in_current.len(), 2);
        assert_eq!(plan.next_chapter_opening_beats[0], "承接门开后的状态");
    }

    #[test]
    fn draft_prompt_includes_flexible_chapter_contract() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "雨夜收尸人".to_string(),
                genre: "都市异能".to_string(),
                target_words: 180000,
                premise: "主角在殡仪馆卷入怪异秩序。".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        state
            .update_chapter(ChapterUpdate {
                project_id: project.id,
                id: chapter.id,
                title: "第 1 章 雨夜收尸人".to_string(),
                status: chapter.status,
            })
            .unwrap();

        let setting = state
            .insert_artifact(
                project.id,
                None,
                "setting",
                "设定",
                "## 一句话卖点\n殡仪馆杂工卷入地下怪异秩序。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "setting", setting.id, "通过")
            .unwrap();

        let outline = state
            .insert_artifact(
                project.id,
                None,
                "outline",
                "大纲",
                "## 第1章：雨夜收尸人\n- 章节目标：从暴雨夜收尸开始，确认一具无名女尸与主角能力有关。\n- 核心冲突：主角想按流程收尸，却被迫处理不该看的异常。\n- 信息释放：女尸手腕编号、尸视能力、清理者夜访。\n- 结尾钩子：主角发现07号柜与自己有关。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "outline", outline.id, "通过")
            .unwrap();

        let characters = state
            .insert_artifact(
                project.id,
                None,
                "characters",
                "角色",
                "主角陈渡：殡仪馆杂工，谨慎，职业化。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "characters", characters.id, "通过")
            .unwrap();

        let prompt = build_prompt(
            &state,
            project.id,
            &Stage::Draft,
            Some(chapter.id),
            Some("开篇要更快进入异常"),
            None,
            None,
        )
        .unwrap();

        assert!(prompt.contains("# 本章柔性任务契约"));
        assert!(prompt.contains("- 章节模式："));
        assert!(prompt.contains("- 进入状态："));
        assert!(prompt.contains("- 本章目标："));
        assert!(prompt.contains("- 必须发生的变化："));
        assert!(prompt.contains("- 结尾功能："));
        assert!(prompt.contains("- 离开状态 / 下一章新条件："));
        assert!(!prompt.contains("3. 中段发现："));
        assert!(!prompt.contains("4. 代价转折："));

        let candidate = state
            .insert_artifact(
                project.id,
                Some(chapter.id),
                "draft",
                "候选稿",
                "陈渡推开停尸间的门，雨水顺着袖口滴进地面。07号柜忽然响了一声。",
                None,
            )
            .unwrap();
        let review_prompt = build_prompt(
            &state,
            project.id,
            &Stage::Review,
            Some(chapter.id),
            None,
            Some(&candidate),
            None,
        )
        .unwrap();

        assert!(review_prompt.contains("# 章节任务兑现审校"));
        assert!(review_prompt.contains("章节任务未兑现"));
        assert!(review_prompt.contains("允许同义表达、等效行动"));
        assert!(review_prompt.contains("# 本章任务卡"));
        assert!(review_prompt.contains("核心冲突：主角想按流程收尸"));
    }

    #[test]
    fn base_chapter_modes_are_not_bound_to_xianxia_terms() {
        let mode = infer_chapter_mode(
            "第 8 章 修复录音",
            "修复损坏录音，确认死者留下的时间线",
            "主角必须在证物被销毁前完成处理",
            "录音里出现一段陌生的求救声",
            None,
        );

        assert!(mode.starts_with("资源技艺章"));
        assert!(!mode.contains("炼药"));
        assert!(!mode.contains("修仙"));
    }

    #[test]
    fn ending_function_changes_with_chapter_mode_without_forcing_danger() {
        let growth = ending_function_for_mode("成长准备章：修炼与恢复");
        let relationship = ending_function_for_mode("关系谈判章：交换筹码");
        let transition = ending_function_for_mode("过渡布局章：调整路线");

        assert!(growth.contains("不必追加突发敌人"));
        assert!(relationship.contains("关系新平衡"));
        assert!(transition.contains("情绪余韵型"));
        assert!(transition.contains("不要求制造危险"));
    }

    #[test]
    fn recent_chapter_context_uses_only_the_two_latest_chapters() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "上下文实验".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120000,
                premise: "验证近章上下文层级。".to_string(),
            })
            .unwrap();
        let chapter_one = state.list_chapters(project.id).unwrap().remove(0);
        let chapter_two = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第二章".to_string()),
            })
            .unwrap();
        let chapter_three = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第三章".to_string()),
            })
            .unwrap();
        let chapter_four = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第四章".to_string()),
            })
            .unwrap();

        for (chapter, body) in [
            (&chapter_one, "第一章原文事实。"),
            (&chapter_two, "第二章原文事实。"),
            (&chapter_three, "第三章原文事实。"),
        ] {
            let artifact = state
                .insert_artifact(
                    project.id,
                    Some(chapter.id),
                    "draft",
                    "章节草稿",
                    body,
                    None,
                )
                .unwrap();
            state
                .approve_stage(project.id, "draft", artifact.id, "通过")
                .unwrap();
        }

        let mut compact = String::new();
        append_recent_chapter_context(&state, project.id, chapter_four.id, &mut compact).unwrap();
        assert!(compact.contains("最近已通过章节上下文"));
        assert!(compact.contains("第三章原文事实"));
        assert!(compact.contains("第二章原文事实"));
        assert!(!compact.contains("第一章原文事实"));
    }

    #[test]
    fn outline_prompt_reads_locked_written_progress_from_the_database() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "进度边界实验".to_string(),
                genre: "男频修仙".to_string(),
                target_words: 200_000,
                premise: "只能从未完成章节继续规划。".to_string(),
            })
            .unwrap();
        let chapter_one = state.list_chapters(project.id).unwrap().remove(0);
        let chapter_two = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第二章 已写".to_string()),
            })
            .unwrap();
        let chapter_three = state
            .create_chapter(NewChapter {
                project_id: project.id,
                title: Some("第三章 待写".to_string()),
            })
            .unwrap();
        let setting = state
            .insert_artifact(
                project.id,
                None,
                "setting",
                "设定",
                "山门以灵砂控制杂役。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "setting", setting.id, "通过")
            .unwrap();

        for (chapter, body) in [
            (
                &chapter_one,
                "第一章开场事实。主角拿到黑牌。第一章结尾事实。",
            ),
            (
                &chapter_two,
                "第二章开场事实。主角确认药效。第二章结尾事实。",
            ),
        ] {
            let artifact = state
                .insert_artifact(
                    project.id,
                    Some(chapter.id),
                    "draft",
                    "正式正文候选",
                    body,
                    None,
                )
                .unwrap();
            state
                .approve_stage(project.id, "draft", artifact.id, "通过")
                .unwrap();
        }
        state
            .insert_artifact(
                project.id,
                Some(chapter_three.id),
                "draft",
                "未批准候选",
                "未批准候选绝不能进入故事架构上下文。",
                None,
            )
            .unwrap();

        let prompt =
            build_prompt(&state, project.id, &Stage::Outline, None, None, None, None).unwrap();

        assert!(prompt.contains("# 已写正文进度（应用数据库）"));
        assert!(prompt.contains("已锁定：第 1 章"));
        assert!(prompt.contains("已锁定：第 2 章 第二章 已写"));
        assert!(prompt.contains("第一章开场事实"));
        assert!(prompt.contains("第二章结尾事实"));
        assert!(prompt.contains("规划起点：第 3 章 第三章 待写"));
        assert!(prompt.contains("详细章节生产卡必须从第 3 章开始"));
        assert!(prompt.contains("不得为这些章节重新生成章节模式、目标、阻力"));
        assert!(!prompt.contains("再写前 12 章待写列表"));
        assert!(!prompt.contains("未批准候选绝不能进入故事架构上下文"));
    }

    #[test]
    fn outline_task_uses_initial_twelve_chapter_mode_only_before_writing_starts() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "初始大纲实验".to_string(),
                genre: "奇幻".to_string(),
                target_words: 120_000,
                premise: "尚无正式正文。".to_string(),
            })
            .unwrap();

        let task = outline_task_for_prompt(&state, project.id).unwrap();

        assert!(task.contains("再写前 12 章待写列表"));
        assert!(!task.contains("已写进度摘要"));
    }

    #[test]
    fn draft_prompt_uses_genre_specific_skill_guidance() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "烬骨长生".to_string(),
                genre: "男频修仙爽文".to_string(),
                target_words: 220000,
                premise: "废徒在焚炉谷获得烬火机缘。".to_string(),
            })
            .unwrap();
        let chapter = state.list_chapters(project.id).unwrap().remove(0);

        let setting = state
            .insert_artifact(
                project.id,
                None,
                "setting",
                "设定",
                "## 一句话卖点\n焚骨废徒拿到烬火机缘。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "setting", setting.id, "通过")
            .unwrap();

        let outline = state
            .insert_artifact(
                project.id,
                None,
                "outline",
                "大纲",
                "## 第1章：焚骨夜\n- 章节目标：拿到第一份机缘。\n- 核心冲突：主角必须在压迫下保命并判断机缘值不值得冒险。\n- 信息释放：焚炉谷规则、真龙指骨、黑牌。\n- 结尾钩子：主角发现黑牌能引动旧炉。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "outline", outline.id, "通过")
            .unwrap();

        let characters = state
            .insert_artifact(
                project.id,
                None,
                "characters",
                "角色",
                "陆烬：外门废徒，火脉残缺。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "characters", characters.id, "通过")
            .unwrap();

        let prompt = build_prompt(
            &state,
            project.id,
            &Stage::Draft,
            Some(chapter.id),
            None,
            None,
            None,
        )
        .unwrap();

        assert!(prompt.contains("# 题材 Skill"));
        assert!(prompt.contains("男频修仙/升级流"));
        assert!(prompt.contains("发育修炼章"));
        assert!(prompt.contains("本章进度增量"));
        assert!(!prompt.contains("都市异能/悬疑"));
        assert!(prompt.contains("写作 Craft Skill"));
        assert!(prompt.contains("长篇连续性与主动性"));
        assert!(prompt.contains("同一个悬念或结论，最多用两件证据确认"));
    }

    #[test]
    fn reuses_cached_continuity_review_for_identical_key() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "缓存测试书".to_string(),
                genre: "悬疑".to_string(),
                target_words: 120000,
                premise: "测试连续性缓存".to_string(),
            })
            .unwrap();
        let cache_key = continuity_cache_key(project.id, &[11, 12, 13], &[99]);
        state
            .insert_workflow_run(
                project.id,
                None,
                "continuity_review",
                &format!("# continuity-cache-key: {}\n\nprompt body", cache_key),
                r#"[{"issue_type":"物件状态断点","severity":"major","chapters":["第2章","第3章"],"reason":"旧锁状态跳变","suggestion":"补状态过渡"}]"#,
                "success",
                None,
                12,
            )
            .unwrap();

        let report = find_cached_continuity_report(
            &state,
            project.id,
            &cache_key,
            vec![
                "第1章".to_string(),
                "第2章".to_string(),
                "第3章".to_string(),
            ],
        )
        .unwrap()
        .unwrap();

        assert_eq!(report.verdict, "needs_revision");
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].issue_type, "物件状态断点");
    }

    #[tokio::test]
    #[ignore = "requires live AI credentials"]
    async fn live_writing_case_quality_check() {
        let api_key =
            env::var("BOOK_STUDIO_LIVE_API_KEY").expect("missing BOOK_STUDIO_LIVE_API_KEY");
        let base_url = env::var("BOOK_STUDIO_LIVE_BASE_URL")
            .unwrap_or_else(|_| "https://api.deepseek.com".to_string());
        let model =
            env::var("BOOK_STUDIO_LIVE_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".to_string());

        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        state
            .save_ai_settings(SaveAiSettings {
                base_url,
                model,
                temperature: 0.72,
                thinking_enabled: false,
                thinking_level: "off".to_string(),
                api_key: Some(api_key),
            })
            .unwrap();

        let project = state
            .create_project(NewProject {
                title: "雨夜收尸人".to_string(),
                genre: "都市异能".to_string(),
                target_words: 180000,
                premise: "一个在殡仪馆打杂的青年，能在尸体上看见死者临终残留的执念，并借此追索超凡事件。他原本只想混口饭吃，却在一场暴雨夜的无名尸体中，被卷进一座城市地下的怪异秩序。".to_string(),
            })
            .unwrap();

        let chapter = state.list_chapters(project.id).unwrap().remove(0);
        state
            .update_chapter(ChapterUpdate {
                project_id: project.id,
                id: chapter.id,
                title: "第 1 章 雨夜收尸人".to_string(),
                status: chapter.status,
            })
            .unwrap();

        let setting = run_agent_step(
            &state,
            RunAgentRequest {
                project_id: project.id,
                stage: Stage::Setting,
                chapter_id: None,
                source_artifact_id: None,
            user_instruction: Some("面向男频商业连载，风格克制冷峻但易读，核心卖点是职业细节、怪异规则、低位逆袭和层层揭露。设定必须具体、可执行、能支持长篇推进。".to_string()),
                reference_selection: None,
                prepared_context_id: None,
            },
        )
        .await
        .unwrap();
        state
            .approve_stage(project.id, "setting", setting.artifact.id, "通过")
            .unwrap();

        let outline = run_agent_step(
            &state,
            RunAgentRequest {
                project_id: project.id,
                stage: Stage::Outline,
                chapter_id: None,
                source_artifact_id: None,
            user_instruction: Some("给出整书主线，并明确第一卷前12章的章节目标，尤其说明第1章的开场钩子、冲突和结尾悬念。".to_string()),
                reference_selection: None,
                prepared_context_id: None,
            },
        )
        .await
        .unwrap();
        state
            .approve_stage(project.id, "outline", outline.artifact.id, "通过")
            .unwrap();

        let characters = run_agent_step(
            &state,
            RunAgentRequest {
                project_id: project.id,
                stage: Stage::Characters,
                chapter_id: None,
                source_artifact_id: None,
            user_instruction: Some("重点把主角、殡仪馆师父、第一章出现的女警、幕后怪异势力的前台人物写清楚，确保角色有鲜明口吻和现实职业感。".to_string()),
                reference_selection: None,
                prepared_context_id: None,
            },
        )
        .await
        .unwrap();
        state
            .approve_stage(project.id, "characters", characters.artifact.id, "通过")
            .unwrap();

        let draft = run_agent_step(
            &state,
            RunAgentRequest {
                project_id: project.id,
                stage: Stage::Draft,
                chapter_id: Some(chapter.id),
                source_artifact_id: None,
            user_instruction: Some("只写第一章正文，目标约1200到1800字。必须从暴雨夜收尸开场，尽快展示主角职业细节和特殊能力，章末给出强悬念，避免摘要式写法，多用场景、动作、对话和细节推进。".to_string()),
                reference_selection: None,
                prepared_context_id: None,
            },
        )
        .await
        .unwrap();
        state
            .approve_stage(project.id, "draft", draft.artifact.id, "通过")
            .unwrap();

        let review = run_agent_step(
            &state,
            RunAgentRequest {
                project_id: project.id,
                stage: Stage::Review,
                chapter_id: Some(chapter.id),
                source_artifact_id: None,
            user_instruction: Some("重点检查是否像真正网文开篇，而不是设定说明；对节奏、信息投放、悬念强度、对白质感从严挑问题。".to_string()),
                reference_selection: None,
                prepared_context_id: None,
            },
        )
        .await
        .unwrap();
        state
            .approve_stage(project.id, "review", review.artifact.id, "通过")
            .unwrap();

        let revision = request_revision(
            &state,
            RevisionRequest {
                project_id: project.id,
                artifact_id: draft.artifact.id,
                feedback: "优先解决开头抓力、对白僵硬、解释感过重的问题。修订时保留职业氛围和怪异感，结尾悬念再抬高一档。".to_string(),
                reference_selection: None,
            },
        )
        .await
        .unwrap();

        let exported = export_markdown(&state, project.id).unwrap();
        let export_path = env::temp_dir().join("xiic-book-studio-live-case.md");
        fs::write(&export_path, &exported).unwrap();

        let draft_len = draft.artifact.content.chars().count();
        let revision_len = revision.artifact.content.chars().count();
        let review_issues =
            serde_json::from_str::<Vec<crate::models::ReviewIssue>>(&review.artifact.content)
                .unwrap_or_default();

        println!("\n=== SETTING ===\n{}\n", setting.artifact.content);
        println!("\n=== OUTLINE ===\n{}\n", outline.artifact.content);
        println!("\n=== CHARACTERS ===\n{}\n", characters.artifact.content);
        println!("\n=== DRAFT ===\n{}\n", draft.artifact.content);
        println!("\n=== REVIEW ===\n{}\n", review.artifact.content);
        println!("\n=== REVISION ===\n{}\n", revision.artifact.content);
        println!("\n=== EXPORTED ===\n{}\n", export_path.display());

        assert!(setting.artifact.content.chars().count() > 400);
        assert!(outline.artifact.content.chars().count() > 800);
        assert!(characters.artifact.content.chars().count() > 500);
        assert!(draft_len > 1000, "draft too short: {}", draft_len);
        assert!(revision_len > 1000, "revision too short: {}", revision_len);
        assert!(
            !review_issues.is_empty(),
            "review did not produce structured issues"
        );
        assert!(draft.artifact.content.contains('“') || draft.artifact.content.contains('"'));
        assert!(revision.artifact.content.contains('“') || revision.artifact.content.contains('"'));
        assert!(exported.contains("## 章节工作记录"));
    }
}
