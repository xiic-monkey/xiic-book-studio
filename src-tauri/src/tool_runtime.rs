use std::{collections::HashSet, time::Instant};

use serde_json::{json, Value};

use crate::{
    agent_tools, ai, chapter_memory, continuity_ledger,
    db::AppState,
    error::{AppError, AppResult},
    models::{
        ActionProposal, Agent, AgentToolDefinition, ChapterSplitPlanRequest, ReferenceSelection,
        Stage, StoryContextSearchInput, ToolCall, ToolKind, ToolProtocol, ToolResult,
    },
    quality, workflow,
};

const MAX_TOOL_ROUNDS: usize = 4;
const MAX_TOOL_CALLS: usize = 12;
const MAX_RENDERED_CONTEXT_CHARS: usize = 24_000;

pub struct ToolExecutionContext<'a> {
    pub state: &'a AppState,
    pub agent: &'a Agent,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: &'a Stage,
    pub source_artifact_id: Option<i64>,
    pub user_instruction: Option<&'a str>,
    pub reference_selection: Option<&'a ReferenceSelection>,
    pub run_id: Option<i64>,
    pub preview: bool,
}

#[derive(Debug, Default)]
pub struct ToolPreparation {
    pub rendered_context: Option<String>,
    pub invocation_ids: Vec<i64>,
    pub proposals: Vec<ActionProposal>,
    pub protocol: Option<String>,
}

pub fn definitions_for_agent(
    agent: &Agent,
    stage: &Stage,
    preview: bool,
) -> Vec<AgentToolDefinition> {
    agent_tools::definitions()
        .into_iter()
        .filter(|definition| agent.has_tool(&definition.key))
        .filter(|definition| {
            definition
                .supported_stages
                .iter()
                .any(|item| item == stage.as_str())
        })
        .filter(|definition| {
            !preview || (definition.previewable && definition.kind == ToolKind::Read)
        })
        .collect()
}

pub async fn prepare_tools(
    context: ToolExecutionContext<'_>,
    task_prompt: &str,
) -> AppResult<ToolPreparation> {
    let definitions = definitions_for_agent(context.agent, context.stage, context.preview);
    if definitions.is_empty() {
        return Ok(ToolPreparation::default());
    }
    let settings = context.agent.ai_settings();
    let api_key = context
        .state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先为当前 Agent 供应商保存 API Key".to_string()))?;
    let capabilities = context.state.provider_capabilities(&settings.base_url)?;
    let configured = capabilities.configured_protocol;
    let mut detected = capabilities.detected_protocol;
    let mut planning_context = sample(task_prompt, 10_000);
    let mut preparation = ToolPreparation::default();
    let mut seen = HashSet::new();
    let mut total_calls = 0usize;

    for round in 0..MAX_TOOL_ROUNDS {
        let (calls, protocol) = plan_calls(
            context.state,
            &settings,
            &api_key,
            &context.agent.system_prompt,
            &planning_context,
            &definitions,
            configured.clone(),
            detected.clone(),
        )
        .await?;
        if matches!(configured, ToolProtocol::Auto) {
            detected = Some(ToolProtocol::parse(&protocol));
        }
        preparation.protocol = Some(protocol.clone());
        if calls.is_empty() {
            break;
        }

        let mut pending_calls = Vec::new();
        let mut round_keys = HashSet::new();
        for call in calls {
            let dedup_key = format!(
                "{}:{}",
                call.tool_key,
                serde_json::to_string(&call.arguments)?
            );
            if seen.contains(&dedup_key) || !round_keys.insert(dedup_key.clone()) {
                continue;
            }
            pending_calls.push((call, dedup_key));
        }
        if total_calls.saturating_add(pending_calls.len()) > MAX_TOOL_CALLS {
            return Err(AppError::Validation(format!(
                "Agent 工具调用超过单次运行上限 {MAX_TOOL_CALLS}"
            )));
        }
        for (call, _) in &pending_calls {
            if let Err(error) = validate_call(&context, &definitions, call) {
                context.state.insert_tool_invocation(
                    context.run_id,
                    None,
                    context.project_id,
                    context.chapter_id,
                    context.stage.as_str(),
                    &call.tool_key,
                    &protocol,
                    &call.arguments,
                    &json!({}),
                    "rejected",
                    Some(&error.to_string()),
                    0,
                )?;
                return Err(error);
            }
        }

        let mut round_results = Vec::new();
        for (call, dedup_key) in pending_calls {
            seen.insert(dedup_key);
            total_calls += 1;
            let started = Instant::now();
            let execution = execute_call(&context, &call).await;
            let elapsed_ms = started.elapsed().as_millis() as i64;
            let (result, proposal) = match execution {
                Ok((data, citations, truncated, proposal)) => (
                    ToolResult {
                        call_id: call.call_id.clone(),
                        tool_key: call.tool_key.clone(),
                        status: "success".to_string(),
                        data,
                        citations,
                        error: None,
                        elapsed_ms,
                        truncated,
                    },
                    proposal,
                ),
                Err(error) => (
                    ToolResult {
                        call_id: call.call_id.clone(),
                        tool_key: call.tool_key.clone(),
                        status: "failed".to_string(),
                        data: json!({}),
                        citations: Vec::new(),
                        error: Some(error.to_string()),
                        elapsed_ms,
                        truncated: false,
                    },
                    None,
                ),
            };
            let record = context.state.insert_tool_invocation(
                context.run_id,
                None,
                context.project_id,
                context.chapter_id,
                context.stage.as_str(),
                &call.tool_key,
                &protocol,
                &call.arguments,
                &serde_json::to_value(&result)?,
                &result.status,
                result.error.as_deref(),
                result.elapsed_ms,
            )?;
            preparation.invocation_ids.push(record.id);
            if let Some(proposal) = proposal {
                preparation.proposals.push(proposal);
            }
            round_results.push(result);
        }
        if round_results.is_empty() {
            break;
        }
        planning_context.push_str("\n\n# 已执行工具结果\n");
        planning_context.push_str(&serde_json::to_string_pretty(&round_results)?);
        if planning_context.chars().count() > MAX_RENDERED_CONTEXT_CHARS {
            planning_context = sample(&planning_context, MAX_RENDERED_CONTEXT_CHARS);
        }
        if round + 1 == MAX_TOOL_ROUNDS {
            return Err(AppError::Validation(format!(
                "Agent 工具交互达到单次运行上限 {MAX_TOOL_ROUNDS} 轮"
            )));
        }
    }

    if !preparation.invocation_ids.is_empty() {
        let records = preparation
            .invocation_ids
            .iter()
            .filter_map(|id| {
                context
                    .state
                    .with_conn(|conn| {
                        conn.query_row(
                            "SELECT result_json FROM tool_invocations WHERE id = ?1",
                            [id],
                            |row| row.get::<_, String>(0),
                        )
                        .map_err(Into::into)
                    })
                    .ok()
            })
            .collect::<Vec<_>>();
        let mut rendered = String::from(
            "# Agent 工具执行结果\n以下结果由 App 工具运行时执行并记录。只能使用结果中明确出现的事实；失败或未命中不得被解释为事实成立。",
        );
        for raw in records {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                rendered.push_str("\n\n");
                rendered.push_str(&serde_json::to_string_pretty(&value)?);
            }
        }
        preparation.rendered_context = Some(sample(&rendered, MAX_RENDERED_CONTEXT_CHARS));
    }

    Ok(preparation)
}

async fn plan_calls(
    state: &AppState,
    settings: &crate::models::AiSettings,
    api_key: &str,
    system_prompt: &str,
    task_prompt: &str,
    definitions: &[AgentToolDefinition],
    configured: ToolProtocol,
    detected: Option<ToolProtocol>,
) -> AppResult<(Vec<ToolCall>, String)> {
    match configured {
        ToolProtocol::Structured => Ok((
            ai::plan_tool_calls_structured(
                settings,
                api_key,
                system_prompt,
                task_prompt,
                definitions,
            )
            .await?,
            "structured".to_string(),
        )),
        ToolProtocol::Native => {
            ai::plan_tool_calls_native(settings, api_key, system_prompt, task_prompt, definitions)
                .await
                .map(|calls| (calls, "native".to_string()))
                .map_err(|error| AppError::Validation(error.to_string()))
        }
        ToolProtocol::Auto if matches!(detected, Some(ToolProtocol::Structured)) => Ok((
            ai::plan_tool_calls_structured(
                settings,
                api_key,
                system_prompt,
                task_prompt,
                definitions,
            )
            .await?,
            "structured".to_string(),
        )),
        ToolProtocol::Auto => match ai::plan_tool_calls_native(
            settings,
            api_key,
            system_prompt,
            task_prompt,
            definitions,
        )
        .await
        {
            Ok(calls) => {
                state.record_provider_tool_protocol(
                    &settings.base_url,
                    Some(ToolProtocol::Native),
                    None,
                )?;
                Ok((calls, "native".to_string()))
            }
            Err(ai::ToolPlanningError::Unsupported(message)) => {
                state.record_provider_tool_protocol(
                    &settings.base_url,
                    Some(ToolProtocol::Structured),
                    Some(&message),
                )?;
                Ok((
                    ai::plan_tool_calls_structured(
                        settings,
                        api_key,
                        system_prompt,
                        task_prompt,
                        definitions,
                    )
                    .await?,
                    "structured".to_string(),
                ))
            }
            Err(ai::ToolPlanningError::Other(error)) => Err(error),
        },
    }
}

fn validate_call(
    context: &ToolExecutionContext<'_>,
    definitions: &[AgentToolDefinition],
    call: &ToolCall,
) -> AppResult<()> {
    let definition = definitions
        .iter()
        .find(|definition| definition.key == call.tool_key)
        .ok_or_else(|| AppError::Validation(format!("Agent 无权调用工具：{}", call.tool_key)))?;
    if context.preview && (!definition.previewable || definition.kind != ToolKind::Read) {
        return Err(AppError::Validation(format!(
            "预览阶段禁止调用写入工具：{}",
            call.tool_key
        )));
    }
    validate_arguments(&definition.parameters_schema, &call.arguments)
}

fn validate_arguments(schema: &Value, arguments: &Value) -> AppResult<()> {
    validate_schema_value(schema, arguments, "工具参数")
}

fn validate_schema_value(schema: &Value, value: &Value, path: &str) -> AppResult<()> {
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            return Err(AppError::Validation(format!("{path} 不在允许值范围内")));
        }
    }

    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let object = value
                .as_object()
                .ok_or_else(|| AppError::Validation(format!("{path} 必须是 JSON 对象")))?;
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for key in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(key) {
                        return Err(AppError::Validation(format!("{path}缺少字段：{key}")));
                    }
                }
            }
            let properties = schema.get("properties").and_then(Value::as_object);
            for (key, child) in object {
                let Some(child_schema) = properties.and_then(|items| items.get(key)) else {
                    if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
                        return Err(AppError::Validation(format!("{path}包含未知字段：{key}")));
                    }
                    continue;
                };
                validate_schema_value(child_schema, child, &format!("{path}.{key}"))?;
            }
        }
        Some("string") => {
            let text = value
                .as_str()
                .ok_or_else(|| AppError::Validation(format!("{path} 必须是字符串")))?;
            let chars = text.chars().count() as u64;
            if schema
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|min| chars < min)
            {
                return Err(AppError::Validation(format!("{path} 长度不足")));
            }
            if schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|max| chars > max)
            {
                return Err(AppError::Validation(format!("{path} 长度超过限制")));
            }
        }
        Some("integer") => {
            let number = value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|item| i64::try_from(item).ok()))
                .ok_or_else(|| AppError::Validation(format!("{path} 必须是整数")))?;
            if schema
                .get("minimum")
                .and_then(Value::as_i64)
                .is_some_and(|min| number < min)
            {
                return Err(AppError::Validation(format!("{path} 小于允许下限")));
            }
            if schema
                .get("maximum")
                .and_then(Value::as_i64)
                .is_some_and(|max| number > max)
            {
                return Err(AppError::Validation(format!("{path} 超过允许上限")));
            }
        }
        Some("number") if !value.is_number() => {
            return Err(AppError::Validation(format!("{path} 必须是数字")));
        }
        Some("boolean") if !value.is_boolean() => {
            return Err(AppError::Validation(format!("{path} 必须是布尔值")));
        }
        Some("array") => {
            let values = value
                .as_array()
                .ok_or_else(|| AppError::Validation(format!("{path} 必须是数组")))?;
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in values.iter().enumerate() {
                    validate_schema_value(item_schema, item, &format!("{path}[{index}]"))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn execute_call(
    context: &ToolExecutionContext<'_>,
    call: &ToolCall,
) -> AppResult<(Value, Vec<String>, bool, Option<ActionProposal>)> {
    match call.tool_key.as_str() {
        agent_tools::HISTORY_CONTEXT => {
            let query = required_string(&call.arguments, "query")?;
            let limit = call
                .arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(8)
                .clamp(1, 12) as usize;
            let snippets = workflow::search_story_context(
                context.state,
                StoryContextSearchInput {
                    project_id: context.project_id,
                    chapter_id: context.chapter_id,
                    query: query.to_string(),
                    limit: Some(limit),
                    include_immediate_previous: true,
                },
            )?;
            let citations = snippets
                .iter()
                .map(|snippet| snippet.source_label.clone())
                .collect();
            Ok((serde_json::to_value(snippets)?, citations, false, None))
        }
        agent_tools::REFERENCE_MATERIALS => {
            let query = call
                .arguments
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("");
            let content = crate::reference::render_context(
                context.state,
                context.project_id,
                context.stage,
                context.reference_selection,
                query,
            )?;
            Ok((json!({"context": content}), Vec::new(), false, None))
        }
        agent_tools::CHAPTER_MEMORY => {
            let chapter_id = context
                .chapter_id
                .ok_or_else(|| AppError::Validation("章节记忆工具需要当前章节".to_string()))?;
            let chapters = context.state.list_chapters(context.project_id)?;
            let current = chapters
                .iter()
                .find(|chapter| chapter.id == chapter_id)
                .ok_or_else(|| AppError::Validation("当前章节不存在".to_string()))?;
            let predecessor = chapters
                .iter()
                .filter(|chapter| chapter.chapter_no < current.chapter_no)
                .max_by_key(|chapter| chapter.chapter_no);
            let memory = if let Some(predecessor) = predecessor {
                chapter_memory::current_memory_for_chapter(
                    context.state,
                    context.project_id,
                    predecessor.id,
                )?
            } else {
                None
            };
            Ok((serde_json::to_value(memory)?, Vec::new(), false, None))
        }
        agent_tools::CONTINUITY_CHECK => {
            let artifact_id = optional_i64(&call.arguments, "artifact_id")
                .or(context.source_artifact_id)
                .ok_or_else(|| {
                    AppError::Validation("连续性检查需要候选产物 artifact_id".to_string())
                })?;
            let report = continuity_ledger::check_artifact_continuity(
                context.state,
                crate::models::LedgerContinuityCheckRequest {
                    project_id: context.project_id,
                    artifact_id,
                },
            )
            .await?;
            let citations = report
                .issues
                .iter()
                .map(|issue| issue.source_chapter.clone())
                .collect();
            Ok((serde_json::to_value(report)?, citations, false, None))
        }
        agent_tools::QUALITY_ANALYSIS => {
            let artifact_id = required_i64(&call.arguments, "artifact_id")?;
            let artifact = context.state.get_artifact(artifact_id)?;
            if artifact.project_id != context.project_id {
                return Err(AppError::Validation("产物不属于当前项目".to_string()));
            }
            Ok((
                serde_json::to_value(quality::analyze_artifact(&artifact))?,
                Vec::new(),
                false,
                None,
            ))
        }
        agent_tools::CHAPTER_SPLIT => {
            let chapter_id = context
                .chapter_id
                .ok_or_else(|| AppError::Validation("拆章工具需要当前章节".to_string()))?;
            let artifact_id = required_i64(&call.arguments, "artifact_id")?;
            let plan = workflow::generate_chapter_split_plan(
                context.state,
                ChapterSplitPlanRequest {
                    project_id: context.project_id,
                    chapter_id,
                    artifact_id,
                },
            )
            .await?;
            Ok((serde_json::to_value(plan)?, Vec::new(), false, None))
        }
        agent_tools::WEB_SEARCH => {
            let explicit_query = crate::web_search::requested_query(context.user_instruction)
                .ok_or_else(|| {
                    AppError::Validation("联网搜索只能由明确的人工联网指令触发".to_string())
                })?;
            let requested = required_string(&call.arguments, "query")?;
            let query = if requested.trim().is_empty() {
                explicit_query
            } else {
                requested.to_string()
            };
            let results = crate::web_search::search_summaries(context.state, &query).await?;
            let citations = results.iter().map(|result| result.url.clone()).collect();
            Ok((serde_json::to_value(results)?, citations, false, None))
        }
        agent_tools::PROPOSE_CREATE_CHAPTER => {
            let title = required_string(&call.arguments, "title")?;
            create_proposal(
                context,
                "create_chapter",
                &format!("创建章节《{}》", title.trim()),
                &json!({"title": title.trim()}),
                None,
            )
        }
        agent_tools::PROPOSE_RENAME_CHAPTER => {
            let chapter_id = required_i64(&call.arguments, "chapter_id")?;
            let title = required_string(&call.arguments, "title")?;
            let chapter = context
                .state
                .ensure_chapter(context.project_id, Some(chapter_id))?
                .ok_or_else(|| AppError::Validation("章节不属于当前项目".to_string()))?;
            create_proposal(
                context,
                "rename_chapter",
                &format!("将《{}》重命名为《{}》", chapter.title, title.trim()),
                &json!({"chapter_id": chapter_id, "title": title.trim()}),
                Some(&chapter.updated_at),
            )
        }
        agent_tools::PROPOSE_ARTIFACT_CANDIDATE => create_proposal(
            context,
            "artifact_candidate",
            "创建资料候选版本",
            &call.arguments,
            None,
        ),
        agent_tools::PROPOSE_KNOWLEDGE_CARD => create_proposal(
            context,
            "knowledge_card",
            "创建知识卡候选",
            &call.arguments,
            None,
        ),
        agent_tools::PROPOSE_UPDATE_KNOWLEDGE_CARD => {
            let card_id = required_i64(&call.arguments, "card_id")?;
            let card = context
                .state
                .get_knowledge_card(context.project_id, card_id)?;
            create_proposal(
                context,
                "knowledge_card_update",
                &format!("更新知识卡 #{}", card_id),
                &call.arguments,
                Some(&card.updated_at),
            )
        }
        agent_tools::PROPOSE_DELETE_KNOWLEDGE_CARD => {
            let card_id = required_i64(&call.arguments, "card_id")?;
            let card = context
                .state
                .get_knowledge_card(context.project_id, card_id)?;
            create_proposal(
                context,
                "knowledge_card_delete",
                &format!("删除知识卡 #{}", card_id),
                &call.arguments,
                Some(&card.updated_at),
            )
        }
        agent_tools::PROPOSE_FORESHADOWING => create_proposal(
            context,
            "foreshadowing",
            "创建伏笔候选",
            &call.arguments,
            None,
        ),
        _ => Err(AppError::Validation(format!(
            "未注册的工具：{}",
            call.tool_key
        ))),
    }
}

fn create_proposal(
    context: &ToolExecutionContext<'_>,
    proposal_type: &str,
    summary: &str,
    payload: &Value,
    expected_version: Option<&str>,
) -> AppResult<(Value, Vec<String>, bool, Option<ActionProposal>)> {
    if context.preview {
        return Err(AppError::Validation("预览阶段禁止创建写入提案".to_string()));
    }
    let proposal = context.state.create_action_proposal(
        context.project_id,
        context.chapter_id,
        context.run_id,
        proposal_type,
        summary,
        payload,
        expected_version,
    )?;
    Ok((
        json!({"proposal_id": proposal.id, "status": proposal.status, "summary": proposal.summary}),
        Vec::new(),
        false,
        Some(proposal),
    ))
}

fn required_string<'a>(arguments: &'a Value, key: &str) -> AppResult<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Validation(format!("工具参数 {key} 不能为空")))
}

fn required_i64(arguments: &Value, key: &str) -> AppResult<i64> {
    optional_i64(arguments, key)
        .filter(|value| *value > 0)
        .ok_or_else(|| AppError::Validation(format!("工具参数 {key} 不合法")))
}

fn optional_i64(arguments: &Value, key: &str) -> Option<i64> {
    arguments.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().map(|item| item as i64))
    })
}

fn sample(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let head = max_chars / 2;
    let tail = max_chars - head;
    format!(
        "{}\n\n[中间内容已截断]\n\n{}",
        text.chars().take(head).collect::<String>(),
        text.chars().skip(count - tail).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Agent;

    fn agent(keys: &[&str]) -> Agent {
        Agent {
            id: 1,
            stage: "draft".to_string(),
            name: "test".to_string(),
            role: "test".to_string(),
            editable_role: "test".to_string(),
            system_prompt: "test".to_string(),
            editable_system_prompt: "test".to_string(),
            temperature: 0.0,
            provider_base_url: "http://example.test".to_string(),
            model: "test".to_string(),
            thinking_enabled: false,
            thinking_level: "off".to_string(),
            uses_global_runtime_settings: false,
            enabled_tool_keys: keys.iter().map(|key| (*key).to_string()).collect(),
            allowed_skill_keys: Vec::new(),
        }
    }

    #[test]
    fn preview_excludes_proposal_tools() {
        let agent = agent(&[
            agent_tools::HISTORY_CONTEXT,
            agent_tools::PROPOSE_CREATE_CHAPTER,
        ]);
        let definitions = definitions_for_agent(&agent, &Stage::Draft, true);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].key, agent_tools::HISTORY_CONTEXT);
    }

    #[test]
    fn rejects_unknown_arguments_when_schema_is_closed() {
        let definition = agent_tools::get(agent_tools::CHAPTER_MEMORY).unwrap();
        assert!(
            validate_arguments(&definition.parameters_schema, &json!({"extra": true})).is_err()
        );
    }

    #[test]
    fn validates_schema_bounds_and_enums() {
        let web = agent_tools::get(agent_tools::WEB_SEARCH).unwrap();
        assert!(validate_arguments(&web.parameters_schema, &json!({"query": ""})).is_err());
        assert!(
            validate_arguments(&web.parameters_schema, &json!({"query": "x".repeat(241)})).is_err()
        );

        let artifact = agent_tools::get(agent_tools::PROPOSE_ARTIFACT_CANDIDATE).unwrap();
        assert!(validate_arguments(
            &artifact.parameters_schema,
            &json!({"stage": "draft", "title": "标题", "content": "内容"})
        )
        .is_err());
    }
}
