use reqwest::header::{HeaderMap, HeaderValue, ACCEPT_ENCODING};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use crate::{
    error::{AppError, AppResult},
    models::{AgentToolDefinition, AiSettings, ModelInfo, ModelsResponse, ReviewIssue, ToolCall},
};

/// 资料整理/索引这类机器可读步骤统一关闭深度思考，换取稳定的 JSON 输出与低延迟。
pub fn deterministic_settings(mut settings: AiSettings) -> AiSettings {
    settings.thinking_enabled = false;
    settings
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    #[serde(
        default,
        deserialize_with = "null_to_empty_string",
        skip_serializing_if = "String::is_empty"
    )]
    pub content: String,
    /// assistant 回合带的工具调用（多轮 tool-calling 用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AssistantToolCall>>,
    /// tool 消息回填时指向被调用的那次调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// 流式响应里解析出的一次工具调用。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamToolCall {
    pub id: String,
    pub name: String,
    /// 原始 JSON 文本（模型分片拼接后）。
    pub arguments: String,
}

/// 一次流式响应的结果：正文增量与工具调用。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamChunk {
    pub content: String,
    pub tool_calls: Vec<StreamToolCall>,
}

/// 多轮 tool-calling 的最终结果：正文是模型收尾时的人话总结，
/// tool_calls 是整轮里提交过的工具调用。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamingOutcome {
    pub content: String,
    pub tool_calls: Vec<StreamToolCall>,
}

/// assistant 回合声明的工具调用。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: AssistantToolCallFunction,
}

/// 供应商可能把 content 回成 null（纯工具调用回合）。
fn null_to_empty_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_split: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// 下发给模型的工具目录（OpenAI 兼容的 function calling 形状）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

const MAX_AI_RETRIES: usize = 10;
const AI_REQUEST_TIMEOUT_SECONDS: u64 = 240;
const AI_ORCHESTRATOR_TIMEOUT_SECONDS: u64 = 60;
const AI_STREAM_MAX_ATTEMPTS: usize = 2;
/// 工具规划调用的尝试次数。上游偶发「连上了但不返回」，重试一次通常就能拿到结果。
const AI_PLAN_MAX_ATTEMPTS: usize = 3;
const AI_STREAM_FIRST_CHUNK_TIMEOUT_SECONDS: u64 = 30;
const AI_RETRY_DELAY_MILLIS: u64 = 1_000;

/// 供应商不支持原生工具调用时的错误前缀。
///
/// 上层据此判断能否退回另一套协议重试，所以前缀本身属于对外契约。
pub const UNSUPPORTED_TOOLS_PREFIX: &str = "供应商不支持原生工具调用";

/// 判断错误是不是"供应商不支持原生工具调用"。
pub fn is_unsupported_tools_error(error: &AppError) -> bool {
    error.to_string().starts_with(UNSUPPORTED_TOOLS_PREFIX)
}

#[derive(Debug)]
pub enum ToolPlanningError {
    Unsupported(String),
    Other(AppError),
}

impl From<AppError> for ToolPlanningError {
    fn from(value: AppError) -> Self {
        Self::Other(value)
    }
}

impl std::fmt::Display for ToolPlanningError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) | Self::Other(AppError::Validation(message)) => {
                formatter.write_str(message)
            }
            Self::Other(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

pub fn normalize_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("api.minimax.com") || lower.contains("api.minimaxi.com") {
        return "https://api.minimaxi.com/v1".to_string();
    }

    trimmed.to_string()
}

pub fn build_chat_request(
    settings: &AiSettings,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> ChatCompletionRequest {
    let thinking_enabled =
        settings.thinking_enabled && !settings.thinking_level.eq_ignore_ascii_case("off");
    ChatCompletionRequest {
        model: settings.model.clone(),
        temperature,
        stream: None,
        max_completion_tokens: provider_max_completion_tokens(&settings.base_url),
        thinking: provider_thinking_config(&settings.base_url, thinking_enabled),
        reasoning_split: provider_reasoning_split(&settings.base_url, thinking_enabled),
        response_format: None,
        tools: None,
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
    }
}

pub async fn complete_chat(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> AppResult<String> {
    complete_chat_with_format(
        settings,
        api_key,
        system_prompt,
        user_prompt,
        temperature,
        false,
    )
    .await
}

pub async fn complete_json_chat(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> AppResult<String> {
    complete_chat_with_format(
        settings,
        api_key,
        system_prompt,
        user_prompt,
        temperature,
        true,
    )
    .await
}

pub async fn complete_json_chat_for_orchestrator(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
) -> AppResult<String> {
    complete_chat_with_format_options(
        settings,
        api_key,
        system_prompt,
        user_prompt,
        temperature,
        true,
        AI_ORCHESTRATOR_TIMEOUT_SECONDS,
        0,
    )
    .await
}

pub async fn plan_tool_calls_native(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    tools: &[AgentToolDefinition],
) -> Result<(Vec<ToolCall>, Option<String>), ToolPlanningError> {
    let mut last_error: Option<ToolPlanningError> = None;
    for attempt in 1..=AI_PLAN_MAX_ATTEMPTS {
        match plan_tool_calls_native_once(settings, api_key, system_prompt, user_prompt, tools).await {
            Ok(result) => return Ok(result),
            // 供应商不支持原生工具调用是确定性结论，重试没有意义。
            Err(error @ ToolPlanningError::Unsupported(_)) => return Err(error),
            Err(error) => {
                last_error = Some(error);
                if should_retry(attempt, AI_PLAN_MAX_ATTEMPTS) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AppError::Validation("工具规划请求失败".to_string()).into()
    }))
}

async fn plan_tool_calls_native_once(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    tools: &[AgentToolDefinition],
) -> Result<(Vec<ToolCall>, Option<String>), ToolPlanningError> {
    let normalized_base_url = normalize_base_url(&settings.base_url);
    if normalized_base_url.is_empty() {
        return Err(AppError::Validation("请先设置 API Base URL".to_string()).into());
    }
    if settings.model.trim().is_empty() {
        return Err(AppError::Validation("请先设置模型名称".to_string()).into());
    }
    if tools.is_empty() {
        return Ok((Vec::new(), None));
    }

    let thinking_enabled =
        settings.thinking_enabled && !settings.thinking_level.eq_ignore_ascii_case("off");
    let native_tools = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.key,
                    "description": tool.description,
                    "parameters": tool.parameters_schema,
                }
            })
        })
        .collect::<Vec<_>>();
    let mut request = serde_json::json!({
        "model": settings.model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "temperature": 0.0,
        "tools": native_tools,
        "tool_choice": "auto"
    });
    if let Some(tokens) = provider_max_completion_tokens(&settings.base_url) {
        request["max_completion_tokens"] = serde_json::json!(tokens);
    }
    if let Some(config) = provider_thinking_config(&settings.base_url, thinking_enabled) {
        request["thinking"] = serde_json::to_value(config).map_err(AppError::from)?;
    }
    if let Some(split) = provider_reasoning_split(&settings.base_url, thinking_enabled) {
        request["reasoning_split"] = serde_json::json!(split);
    }

    let response = timeout(
        Duration::from_secs(AI_REQUEST_TIMEOUT_SECONDS),
        build_client()
            .map_err(ToolPlanningError::Other)?
            .post(format!("{normalized_base_url}/chat/completions"))
            .bearer_auth(api_key)
            .json(&request)
            .send(),
    )
    .await
    .map_err(|_| {
        ToolPlanningError::Other(AppError::Validation(format!(
            "工具规划请求超过 {} 秒仍未完成",
            AI_REQUEST_TIMEOUT_SECONDS
        )))
    })?
    .map_err(AppError::from)
    .map_err(ToolPlanningError::Other)?;

    let status = response.status();
    let body = read_response_text_bounded(response)
        .await
        .map_err(ToolPlanningError::Other)?;
    if !status.is_success() {
        let compact = compact_error_body(&body);
        if native_tools_unsupported(status.as_u16(), &body) {
            return Err(ToolPlanningError::Unsupported(format!(
                "供应商不支持原生工具调用：HTTP {status} {compact}"
            )));
        }
        return Err(ToolPlanningError::Other(AppError::Validation(format!(
            "工具规划请求失败：HTTP {status} {compact}"
        ))));
    }

    parse_native_tool_calls(&body)
        .map_err(ToolPlanningError::Other)
        .map(|calls| {
            let narrative = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("choices")
                        .and_then(Value::as_array)
                        .and_then(|choices| choices.first())
                        .and_then(|choice| choice.get("message"))
                        .and_then(narrative_from_message)
                });
            (calls, narrative)
        })
}

/// 从模型的工具决策消息里提取真实思考文本：优先 reasoning_content，
/// 其次与 tool_calls 同现的说明文字；纯 JSON 协议输出不算思考。
fn narrative_from_message(message: &Value) -> Option<String> {
    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if let Some(text) = reasoning {
        return Some(text.to_string());
    }
    let content = message.get("content").and_then(Value::as_str).map(str::trim);
    match content {
        Some(text)
            if !text.is_empty()
                && !(text.starts_with('{') && text.trim_end().ends_with('}'))
                && !(text.starts_with('[') && text.trim_end().ends_with(']')) =>
        {
            Some(text.to_string())
        }
        _ => None,
    }
}

pub async fn plan_tool_calls_structured(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    task_prompt: &str,
    tools: &[AgentToolDefinition],
) -> AppResult<(Vec<ToolCall>, Option<String>)> {
    if tools.is_empty() {
        return Ok((Vec::new(), None));
    }
    let catalog = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "key": tool.key,
                "description": tool.description,
                "parameters_schema": tool.parameters_schema,
            })
        })
        .collect::<Vec<_>>();
    let prompt = format!(
        "# 当前任务\n{task_prompt}\n\n# 可用工具\n{}\n\n# 输出协议\n只输出 JSON 对象：{{\"calls\":[{{\"call_id\":\"call-1\",\"tool_key\":\"工具 key\",\"arguments\":{{}}}}]}}。只在确实需要外部证据或创建人工提案时调用；无需工具则输出 {{\"calls\":[]}}。不得调用目录之外的工具。",
        serde_json::to_string_pretty(&catalog)?
    );
    let raw = complete_chat(settings, api_key, system_prompt, &prompt, 0.0).await?;
    let narrative = structured_preamble(&raw);
    parse_structured_tool_calls(&raw).map(|calls| (calls, narrative))
}

/// 结构化协议下模型写在 JSON 前的分析文字（通常为空）。
fn structured_preamble(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let preamble = raw[..start]
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if preamble.is_empty() {
        None
    } else {
        Some(preamble.to_string())
    }
}

fn native_tools_unsupported(status: u16, body: &str) -> bool {
    if !matches!(status, 400 | 404 | 405 | 415 | 422) {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    [
        "tool_choice",
        "tool calls",
        "tool_calls",
        "function calling",
        "function_call",
        "unknown field `tools`",
        "unsupported parameter: tools",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn parse_native_tool_calls(raw: &str) -> AppResult<Vec<ToolCall>> {
    let value: Value = serde_json::from_str(raw)?;
    let message = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| AppError::Validation("原生工具响应缺少 choices[0].message".to_string()))?;
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let function = call
                .get("function")
                .ok_or_else(|| AppError::Validation("原生工具调用缺少 function".to_string()))?;
            let tool_key = function
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| AppError::Validation("原生工具调用缺少 name".to_string()))?;
            let raw_arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let arguments = serde_json::from_str(raw_arguments).map_err(|error| {
                AppError::Validation(format!("工具 {tool_key} 参数不是合法 JSON：{error}"))
            })?;
            Ok(ToolCall {
                call_id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("native-{index}")),
                tool_key: tool_key.to_string(),
                arguments,
                protocol: "native".to_string(),
            })
        })
        .collect()
}

fn parse_structured_tool_calls(raw: &str) -> AppResult<Vec<ToolCall>> {
    let trimmed = trim_code_fence(raw);
    let value: Value = serde_json::from_str(trimmed).or_else(|_| {
        let start = trimmed.find('{').ok_or_else(|| {
            serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing JSON object",
            ))
        })?;
        let end = trimmed.rfind('}').ok_or_else(|| {
            serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing JSON object",
            ))
        })?;
        serde_json::from_str(&trimmed[start..=end])
    })?;
    let calls = value
        .get("calls")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Validation("结构化工具计划缺少 calls 数组".to_string()))?;
    calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let tool_key = call
                .get("tool_key")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| AppError::Validation("工具计划缺少 tool_key".to_string()))?;
            Ok(ToolCall {
                call_id: call
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("structured-{index}")),
                tool_key: tool_key.to_string(),
                arguments: call
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
                protocol: "structured".to_string(),
            })
        })
        .collect()
}

async fn complete_chat_with_format(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
    json_mode: bool,
) -> AppResult<String> {
    complete_chat_with_format_options(
        settings,
        api_key,
        system_prompt,
        user_prompt,
        temperature,
        json_mode,
        AI_REQUEST_TIMEOUT_SECONDS,
        MAX_AI_RETRIES,
    )
    .await
}

async fn complete_chat_with_format_options(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
    json_mode: bool,
    timeout_seconds: u64,
    max_retries: usize,
) -> AppResult<String> {
    let normalized_base_url = normalize_base_url(&settings.base_url);
    if normalized_base_url.is_empty() {
        return Err(AppError::Validation("请先设置 API Base URL".to_string()));
    }
    if settings.model.trim().is_empty() {
        return Err(AppError::Validation("请先设置模型名称".to_string()));
    }

    let mut request = build_chat_request(settings, system_prompt, user_prompt, temperature);
    if json_mode {
        request.response_format = Some(ResponseFormat {
            kind: "json_object".to_string(),
        });
    }
    let url = format!("{}/chat/completions", normalized_base_url);
    let client = build_client_with_timeout(timeout_seconds)?;
    let mut last_error = None;
    let max_attempts = max_retries + 1;

    for attempt in 1..=max_attempts {
        let response = match timeout(
            Duration::from_secs(timeout_seconds),
            client.post(&url).bearer_auth(api_key).json(&request).send(),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Err(_) => {
                last_error = Some(AppError::Validation(format!(
                    "AI 请求超过 {} 秒仍未完成",
                    timeout_seconds
                )));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
                continue;
            }
            Ok(Err(error)) => {
                last_error = Some(AppError::Network(error));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = read_response_text_bounded(response).await.unwrap_or_default();
            last_error = Some(AppError::Validation(format!(
                "AI 请求失败：HTTP {status} {}",
                compact_error_body(&body)
            )));
            if should_retry(attempt, max_attempts) {
                sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                continue;
            }
            break;
        }

        let body = match read_response_text_bounded(response).await {
            Ok(body) => body,
            Err(error) => {
                last_error = Some(error);
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                    continue;
                }
                break;
            }
        };

        match parse_chat_completion(&body) {
            Ok(chunk) => return Ok(chunk.content),
            Err(error) => {
                last_error = Some(AppError::Validation(format!(
                    "{}；原始响应片段：{}",
                    error,
                    compact_error_body(&body)
                )));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                    continue;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::Validation(format!(
            "AI 没有返回可用内容；已尝试 {} 次",
            max_retries + 1
        ))
    }))
}

pub async fn complete_chat_streaming<F>(
    settings: &AiSettings,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    temperature: f64,
    mut on_update: F,
) -> AppResult<String>
where
    F: FnMut(&str, &str) -> AppResult<()>,
{
    let mut request = build_chat_request(settings, system_prompt, user_prompt, temperature);
    request.stream = Some(true);
    let chunk = stream_completion(settings, api_key, request, &mut on_update).await?;
    Ok(chunk.content)
}

/// 发一轮带工具目录的流式请求，返回本轮的正文与工具调用。
///
/// 多轮 tool-calling 的循环由调用方驱动（它需要拿到工具执行能力与运行上下文），
/// 这里只负责协议：拼请求、发流、归并 tool_calls 分片。
pub async fn stream_chat_turn<F>(
    settings: &AiSettings,
    api_key: &str,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<Value>>,
    temperature: f64,
    on_update: &mut F,
) -> AppResult<StreamChunk>
where
    F: FnMut(&str, &str) -> AppResult<()>,
{
    let mut request = build_chat_request(settings, "", "", temperature);
    request.messages = messages;
    request.tools = tools;
    request.stream = Some(true);
    stream_completion(settings, api_key, request, on_update).await
}

/// 发一次流式请求并读完整个流（含重试与超时窗口）。
async fn stream_completion<F>(
    settings: &AiSettings,
    api_key: &str,
    request: ChatCompletionRequest,
    on_update: &mut F,
) -> AppResult<StreamChunk>
where
    F: FnMut(&str, &str) -> AppResult<()>,
{
    let normalized_base_url = normalize_base_url(&settings.base_url);
    if normalized_base_url.is_empty() {
        return Err(AppError::Validation("请先设置 API Base URL".to_string()));
    }
    if settings.model.trim().is_empty() {
        return Err(AppError::Validation("请先设置模型名称".to_string()));
    }
    let url = format!("{}/chat/completions", normalized_base_url);
    let client = build_client()?;
    let max_attempts = AI_STREAM_MAX_ATTEMPTS;
    let mut last_error = None;

    for attempt in 1..=max_attempts {
        let response = match timeout(
            Duration::from_secs(AI_STREAM_FIRST_CHUNK_TIMEOUT_SECONDS),
            client.post(&url).bearer_auth(api_key).json(&request).send(),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                last_error = Some(AppError::Network(error));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
                continue;
            }
            Err(_) => {
                last_error = Some(AppError::Validation(format!(
                    "流式 AI 请求在 {} 秒内没有建立响应",
                    AI_STREAM_FIRST_CHUNK_TIMEOUT_SECONDS
                )));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = read_response_text_bounded(response).await.unwrap_or_default();
            let compact = compact_error_body(&body);
            // 带工具目录的请求被拒且原因是"不支持 tools"：这是确定性结论，
            // 重试没有意义，交给上层决定是否退回另一套协议。
            if request.tools.is_some() && native_tools_unsupported(status.as_u16(), &body) {
                return Err(AppError::Validation(format!(
                    "{UNSUPPORTED_TOOLS_PREFIX}：HTTP {status} {compact}"
                )));
            }
            last_error = Some(AppError::Validation(format!(
                "AI 请求失败：HTTP {status} {compact}"
            )));
            if should_retry(attempt, max_attempts) {
                sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                continue;
            }
            break;
        }

        let stream_result = match timeout(
            Duration::from_secs(AI_REQUEST_TIMEOUT_SECONDS),
            read_stream_completion(response, on_update),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(AppError::Validation(format!(
                "流式 AI 请求超过 {} 秒仍未完成",
                AI_REQUEST_TIMEOUT_SECONDS
            ))),
        };

        match stream_result {
            Ok(chunk) => return Ok(chunk),
            Err(error) => {
                last_error = Some(error);
                if should_retry(attempt, max_attempts) {
                    on_update("", "")?;
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::Validation(format!("AI 没有返回可用内容；已重试 {} 次", MAX_AI_RETRIES))
    }))
}

pub async fn list_models(settings: &AiSettings, api_key: &str) -> AppResult<Vec<ModelInfo>> {
    let normalized_base_url = normalize_base_url(&settings.base_url);
    if normalized_base_url.is_empty() {
        return Err(AppError::Validation("请先设置 API Base URL".to_string()));
    }

    let url = format!("{}/models", normalized_base_url);
    let client = build_client()?;
    let mut last_error = None;
    let max_attempts = MAX_AI_RETRIES + 1;

    for attempt in 1..=max_attempts {
        let response = match client.get(&url).bearer_auth(api_key).send().await {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(AppError::Network(error));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                    continue;
                }
                break;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = read_response_text_bounded(response).await.unwrap_or_default();
            last_error = Some(AppError::Validation(format!(
                "获取模型列表失败：HTTP {status} {}",
                compact_error_body(&body)
            )));
            if should_retry(attempt, max_attempts) {
                sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                continue;
            }
            break;
        }

        let body = match read_response_text_bounded(response).await {
            Ok(body) => body,
            Err(error) => {
                last_error = Some(error);
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                    continue;
                }
                break;
            }
        };

        match parse_models_response(&body) {
            Ok(models) => return Ok(models),
            Err(error) => {
                last_error = Some(AppError::Validation(format!(
                    "{}；原始响应片段：{}",
                    error,
                    compact_error_body(&body)
                )));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                    continue;
                }
                break;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::Validation(format!("获取模型列表失败；已重试 {} 次", MAX_AI_RETRIES))
    }))
}

pub fn parse_review_issues(raw: &str) -> AppResult<Vec<ReviewIssue>> {
    let trimmed = trim_code_fence(raw);
    let issues = serde_json::from_str::<Vec<ReviewIssue>>(trimmed).map_err(|error| {
        AppError::Validation(format!(
            "试读 Agent 返回的 JSON 不是严格的问题数组：{error}"
        ))
    })?;
    validate_review_issues(&issues)?;
    Ok(issues)
}

/// 校验试读问题的字段完整性。
///
/// JSON 数组解析与 `submit_review_issues` 工具参数两条路径共用同一套规则：
/// 字段不能空、severity 必须是三档之一、引文必须是 8-80 字连续片段。
pub fn validate_review_issues(issues: &[ReviewIssue]) -> AppResult<()> {
    for issue in issues {
        if issue.issue_type.trim().is_empty()
            || issue.location.trim().is_empty()
            || issue.reason.trim().is_empty()
            || issue.suggestion.trim().is_empty()
            || !matches!(issue.severity.as_str(), "minor" | "moderate" | "major")
        {
            return Err(AppError::Validation(
                "试读 Agent 返回的问题字段不完整或 severity 无效".to_string(),
            ));
        }
        let evidence_chars = issue.evidence_quote.trim().chars().count();
        if !(8..=80).contains(&evidence_chars) {
            return Err(AppError::Validation(
                "试读 Agent 返回的问题缺少 8-80 字 evidence_quote".to_string(),
            ));
        }
        if !issue.action_evidence_quote.trim().is_empty()
            && !(8..=80).contains(&issue.action_evidence_quote.trim().chars().count())
        {
            return Err(AppError::Validation(
                "试读 Agent 返回的 action_evidence_quote 必须是 8-80 字".to_string(),
            ));
        }
    }
    Ok(())
}

fn build_client() -> AppResult<reqwest::Client> {
    build_client_with_timeout(AI_REQUEST_TIMEOUT_SECONDS)
}

fn build_client_with_timeout(timeout_seconds: u64) -> AppResult<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .http1_only()
        .default_headers(headers)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .map_err(AppError::from)
}

async fn read_response_text(response: reqwest::Response) -> AppResult<String> {
    let bytes = response.bytes().await?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// 带上限地读取响应体。
///
/// 客户端级超时只覆盖到响应头；上游若在正文中途停住（例如代理把连接挂死），
/// 这里必须再兜一层，否则整轮 Agent 运行会无限等待。
async fn read_response_text_bounded(response: reqwest::Response) -> AppResult<String> {
    timeout(
        Duration::from_secs(AI_REQUEST_TIMEOUT_SECONDS),
        read_response_text(response),
    )
    .await
    .map_err(|_| {
        AppError::Validation(format!(
            "响应体读取超过 {} 秒仍未完成",
            AI_REQUEST_TIMEOUT_SECONDS
        ))
    })?
}

async fn read_stream_completion<F>(
    mut response: reqwest::Response,
    on_update: &mut F,
) -> AppResult<StreamChunk>
where
    F: FnMut(&str, &str) -> AppResult<()>,
{
    let mut buffer = String::new();
    let mut raw_response = String::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    // 工具调用按 index 归并：首片带 id/name，后续片只带 arguments 片段。
    let mut tool_call_ids: Vec<String> = Vec::new();
    let mut tool_call_names: Vec<String> = Vec::new();
    let mut tool_call_arguments: Vec<String> = Vec::new();
    let mut saw_sse_event = false;
    let mut saw_terminal_event = false;

    // A provider can accept the connection but never send an SSE event. Bound the
    // first chunk separately so the shared retry policy can recover visibly.
    let first_chunk = timeout(
        Duration::from_secs(AI_STREAM_FIRST_CHUNK_TIMEOUT_SECONDS),
        response.chunk(),
    )
    .await
    .map_err(|_| {
        AppError::Validation(format!(
            "流式 AI 请求在 {} 秒内没有返回首段内容",
            AI_STREAM_FIRST_CHUNK_TIMEOUT_SECONDS
        ))
    })??;

    let mut next_chunk = first_chunk;
    while let Some(chunk) = next_chunk {
        let text = String::from_utf8_lossy(&chunk);
        raw_response.push_str(&text);
        buffer.push_str(&text);

        while let Some(newline) = buffer.find('\n') {
            let line = buffer[..newline].trim_end_matches('\r').to_string();
            buffer.drain(..=newline);
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            saw_sse_event = true;
            if data == "[DONE]" {
                saw_terminal_event = true;
                continue;
            }
            let value: Value = serde_json::from_str(data)
                .map_err(|err| AppError::Validation(format!("无法解析流式 AI 响应：{err}")))?;
            if let Some(message) = value.get("error").and_then(extract_error_message) {
                return Err(AppError::Validation(format!("AI 返回错误：{message}")));
            }
            if let Some(delta) = extract_stream_delta_reasoning(&value) {
                reasoning.push_str(&delta);
            }
            if let Some(delta) = extract_stream_delta_content(&value) {
                content.push_str(&delta);
            }
            collect_stream_delta_tool_calls(
                &value,
                &mut tool_call_ids,
                &mut tool_call_names,
                &mut tool_call_arguments,
            );
            if !content.is_empty() || !reasoning.is_empty() {
                on_update(&content, &reasoning)?;
            }
            if let Some(reason) = value
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
            {
                if reason == "length" {
                    return Err(AppError::Validation(
                        "AI 输出达到长度上限，已自动重试以避免保存半截正文".to_string(),
                    ));
                }
                saw_terminal_event = true;
            }
        }
        next_chunk = response.chunk().await?;
    }

    if !saw_sse_event {
        return parse_chat_completion(&raw_response);
    }
    if !saw_terminal_event {
        return Err(AppError::Validation(
            "AI 流式响应未正常结束，已自动重试以避免保存半截正文".to_string(),
        ));
    }
    let tool_calls = assemble_stream_tool_calls(
        &tool_call_ids,
        &tool_call_names,
        &tool_call_arguments,
    );
    let normalized = strip_think_blocks(&content);
    if normalized.trim().is_empty() && tool_calls.is_empty() {
        return Err(AppError::Validation("AI 没有返回可用内容".to_string()));
    }
    on_update(&normalized, &reasoning)?;
    Ok(StreamChunk {
        content: normalized,
        tool_calls,
    })
}

/// 归并流式分片里的工具调用：按 index 累积 id、name 与 arguments 片段。
fn collect_stream_delta_tool_calls(
    value: &Value,
    ids: &mut Vec<String>,
    names: &mut Vec<String>,
    arguments: &mut Vec<String>,
) {
    let Some(calls) = value
        .pointer("/choices/0/delta/tool_calls")
        .and_then(Value::as_array)
    else {
        return;
    };
    for call in calls {
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        while ids.len() <= index {
            ids.push(String::new());
            names.push(String::new());
            arguments.push(String::new());
        }
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                ids[index] = id.to_string();
            }
        }
        if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
            if !name.is_empty() {
                names[index] = name.to_string();
            }
        }
        if let Some(chunk) = call.pointer("/function/arguments").and_then(Value::as_str) {
            arguments[index].push_str(chunk);
        }
    }
}

fn assemble_stream_tool_calls(
    ids: &[String],
    names: &[String],
    arguments: &[String],
) -> Vec<StreamToolCall> {
    let mut calls = Vec::new();
    for index in 0..names.len() {
        let name = names[index].trim();
        if name.is_empty() {
            continue;
        }
        calls.push(StreamToolCall {
            id: if ids[index].trim().is_empty() {
                format!("call_{index}")
            } else {
                ids[index].clone()
            },
            name: name.to_string(),
            arguments: arguments[index].clone(),
        });
    }
    calls
}

fn parse_chat_completion(raw: &str) -> AppResult<StreamChunk> {
    let trimmed = trim_code_fence(raw);

    if let Ok(completion) = serde_json::from_str::<ChatCompletionResponse>(trimmed) {
        if let Some(choice) = completion.choices.into_iter().next() {
            let tool_calls = extract_tool_calls_from_message(&choice.message);
            let content = strip_think_blocks(&choice.message.content);
            if !content.trim().is_empty() {
                return Ok(StreamChunk {
                    content,
                    tool_calls,
                });
            }
            if !tool_calls.is_empty() {
                return Ok(StreamChunk {
                    content: String::new(),
                    tool_calls,
                });
            }
        }
    }

    let value: Value = serde_json::from_str(trimmed)?;
    if let Some(message) = value.get("error").and_then(extract_error_message) {
        return Err(AppError::Validation(format!("AI 返回错误：{message}")));
    }

    let tool_calls = value
        .pointer("/choices/0/message")
        .map(extract_message_tool_calls)
        .unwrap_or_default();
    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(extract_message_content)
        .unwrap_or_default();

    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(AppError::Validation("AI 没有返回可用内容".to_string()));
    }

    Ok(StreamChunk {
        content: strip_think_blocks(&content),
        tool_calls,
    })
}

/// 从一条 assistant message 里取出工具调用（非流式兜底路径用）。
fn extract_tool_calls_from_message(message: &ChatMessage) -> Vec<StreamToolCall> {
    let Some(calls) = message.tool_calls.as_ref() else {
        return Vec::new();
    };
    calls
        .iter()
        .filter(|call| !call.function.name.trim().is_empty())
        .map(|call| StreamToolCall {
            id: call.id.clone(),
            name: call.function.name.clone(),
            arguments: call.function.arguments.clone(),
        })
        .collect()
}

/// 从原始 JSON 里取出 assistant message 的工具调用（非流式兜底路径用）。
fn extract_message_tool_calls(message: &Value) -> Vec<StreamToolCall> {
    let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    let mut names = Vec::new();
    let mut arguments = Vec::new();
    for call in calls {
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        while ids.len() <= index {
            ids.push(String::new());
            names.push(String::new());
            arguments.push(String::new());
        }
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            ids[index] = id.to_string();
        }
        if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
            names[index] = name.to_string();
        }
        if let Some(raw) = call.pointer("/function/arguments").and_then(Value::as_str) {
            arguments[index] = raw.to_string();
        }
    }
    assemble_stream_tool_calls(&ids, &names, &arguments)
}

fn provider_max_completion_tokens(base_url: &str) -> Option<u32> {
    let normalized = normalize_base_url(base_url).to_ascii_lowercase();
    if normalized.contains("minimaxi") {
        Some(16_384)
    } else {
        None
    }
}

fn provider_thinking_config(base_url: &str, enabled: bool) -> Option<ThinkingConfig> {
    let normalized = normalize_base_url(base_url).to_ascii_lowercase();
    if normalized.contains("deepseek") {
        return Some(ThinkingConfig {
            kind: if enabled { "enabled" } else { "disabled" }.to_string(),
        });
    }
    if normalized.contains("minimaxi") {
        return Some(ThinkingConfig {
            kind: if enabled { "adaptive" } else { "disabled" }.to_string(),
        });
    }
    if normalized.contains("xiaomimimo") {
        // MiMO 默认开启深度思考，且只有显式下发 type=disabled 才会关闭。
        // 不下发时大提示词请求（例如带工具规划的试读）会在客户端超时窗口内无法返回，
        // 所以这里必须把「关闭思考」真正传下去，而不是退回供应商默认值。
        return Some(ThinkingConfig {
            kind: if enabled { "enabled" } else { "disabled" }.to_string(),
        });
    }
    None
}

fn provider_reasoning_split(base_url: &str, enabled: bool) -> Option<bool> {
    let normalized = normalize_base_url(base_url).to_ascii_lowercase();
    (enabled && normalized.contains("minimaxi")).then_some(true)
}

fn strip_think_blocks(content: &str) -> String {
    let mut output = content.to_string();
    loop {
        let lower = output.to_ascii_lowercase();
        let Some(start) = lower.find("<think>") else {
            break;
        };
        let Some(relative_end) = lower[start..].find("</think>") else {
            break;
        };
        let end = start + relative_end + "</think>".len();
        output.replace_range(start..end, "");
    }
    output.trim().to_string()
}

fn parse_models_response(raw: &str) -> AppResult<Vec<ModelInfo>> {
    let trimmed = trim_code_fence(raw);

    if let Ok(models) = serde_json::from_str::<ModelsResponse>(trimmed) {
        return Ok(models.data);
    }

    if let Ok(models) = serde_json::from_str::<Vec<ModelInfo>>(trimmed) {
        return Ok(models);
    }

    let value: Value = serde_json::from_str(trimmed)?;
    if let Some(message) = value.get("error").and_then(extract_error_message) {
        return Err(AppError::Validation(format!("获取模型列表失败：{message}")));
    }

    Err(AppError::Validation("模型列表响应格式不受支持".to_string()))
}

fn extract_error_message(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => map
            .get("message")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| Some(value.to_string())),
        _ => Some(value.to_string()),
    }
}

fn extract_message_content(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let collected = parts
                .iter()
                .filter_map(|part| match part {
                    Value::String(text) => Some(text.clone()),
                    Value::Object(map) => map
                        .get("text")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| {
                            map.get("content")
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                        }),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            if collected.trim().is_empty() {
                None
            } else {
                Some(collected)
            }
        }
        _ => None,
    }
}

fn extract_stream_delta_content(value: &Value) -> Option<String> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(extract_message_content)
}

fn extract_stream_delta_reasoning(value: &Value) -> Option<String> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| {
            delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
        })
        .and_then(extract_message_content)
}

fn trim_code_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(str::trim)
        .and_then(|inner| inner.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed)
}

fn compact_error_body(raw: &str) -> String {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut snippet = compact.chars().take(320).collect::<String>();
    if compact.chars().count() > 320 {
        snippet.push_str("...");
    }
    snippet
}

fn should_retry(attempt: usize, max_attempts: usize) -> bool {
    attempt < max_attempts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_openai_compatible_request() {
        let settings = AiSettings {
            base_url: "https://example.test/v1".to_string(),
            model: "model-a".to_string(),
            temperature: 0.7,
            thinking_enabled: false,
            thinking_level: "off".to_string(),
            has_api_key: true,
        };
        let request = build_chat_request(&settings, "sys", "user", 0.4);

        assert_eq!(request.model, "model-a");
        assert_eq!(request.temperature, 0.4);
        assert_eq!(request.max_completion_tokens, None);
        assert_eq!(request.thinking, None);
        assert_eq!(request.messages[0].role, "system");
        assert_eq!(request.messages[1].content, "user");
    }

    #[test]
    fn sets_minimax_max_tokens() {
        let settings = AiSettings {
            base_url: "https://api.minimaxi.com/v1".to_string(),
            model: "MiniMax-M3".to_string(),
            temperature: 0.7,
            thinking_enabled: false,
            thinking_level: "off".to_string(),
            has_api_key: true,
        };
        let request = build_chat_request(&settings, "sys", "user", 0.4);

        assert_eq!(request.max_completion_tokens, Some(16_384));
        assert_eq!(
            request.thinking,
            Some(ThinkingConfig {
                kind: "disabled".to_string()
            })
        );
    }

    #[test]
    fn maps_minimax_thinking_when_enabled() {
        let settings = AiSettings {
            base_url: "https://api.minimaxi.com/v1".to_string(),
            model: "MiniMax-M3".to_string(),
            temperature: 0.7,
            thinking_enabled: true,
            thinking_level: "medium".to_string(),
            has_api_key: true,
        };
        let request = build_chat_request(&settings, "sys", "user", 0.4);

        assert_eq!(
            request.thinking,
            Some(ThinkingConfig {
                kind: "adaptive".to_string()
            })
        );
        assert_eq!(request.reasoning_split, Some(true));
    }

    #[test]
    fn maps_deepseek_thinking_when_enabled() {
        let settings = AiSettings {
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            temperature: 0.7,
            thinking_enabled: true,
            thinking_level: "medium".to_string(),
            has_api_key: true,
        };
        let request = build_chat_request(&settings, "sys", "user", 0.4);

        assert_eq!(
            request.thinking,
            Some(ThinkingConfig {
                kind: "enabled".to_string()
            })
        );
        assert_eq!(request.reasoning_split, None);
    }

    #[test]
    fn maps_mimo_thinking_switch_to_explicit_type() {
        let disabled = AiSettings {
            base_url: "https://api.xiaomimimo.com/v1".to_string(),
            model: "mimo-v2.6-flash".to_string(),
            temperature: 0.35,
            thinking_enabled: false,
            thinking_level: "off".to_string(),
            has_api_key: true,
        };
        let request = build_chat_request(&disabled, "sys", "user", 0.35);
        assert_eq!(
            request.thinking,
            Some(ThinkingConfig {
                kind: "disabled".to_string()
            })
        );

        let enabled = AiSettings {
            thinking_enabled: true,
            thinking_level: "medium".to_string(),
            ..disabled
        };
        let request = build_chat_request(&enabled, "sys", "user", 0.35);
        assert_eq!(
            request.thinking,
            Some(ThinkingConfig {
                kind: "enabled".to_string()
            })
        );
    }

    #[test]
    fn unknown_providers_keep_default_thinking_behavior() {
        assert!(provider_thinking_config("https://example.test/v1", false).is_none());
        assert!(provider_thinking_config("https://example.test/v1", true).is_none());
    }

    #[test]
    fn parses_chat_completion_with_string_content() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"你好"}}]}"#;
        assert_eq!(parse_chat_completion(raw).unwrap().content, "你好");
    }

    #[test]
    fn strips_think_blocks_from_chat_completion() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"<think>hidden</think>\n正文"}}]}"#;
        assert_eq!(parse_chat_completion(raw).unwrap().content, "正文");
    }

    #[test]
    fn parses_chat_completion_with_array_content() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": [{"type":"text","text":"第一段"},{"type":"text","text":"第二段"}]
                }
            }]
        }"#;
        assert_eq!(parse_chat_completion(raw).unwrap().content, "第一段\n第二段");
    }

    #[test]
    fn extracts_visible_text_from_streaming_delta() {
        let value: Value = serde_json::json!({
            "choices": [{
                "delta": {
                    "reasoning_content": "只应作为思考过程",
                    "content": "可见正文"
                }
            }]
        });

        assert_eq!(
            extract_stream_delta_content(&value).as_deref(),
            Some("可见正文")
        );
    }

    #[test]
    fn parses_review_issues_wrapped_in_json_code_fence() {
        let raw = r#"```json
[
  {
    "issue_type": "钩子不足",
    "severity": "moderate",
    "location": "章末",
    "reason": "结尾没有形成下一步压力。",
    "suggestion": "推进既有时限。",
    "evidence_quote": "他只剩下两个时辰。",
    "action_evidence_quote": "他只剩下两个时辰。"
  }
]
```"#;

        let issues = parse_review_issues(raw).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, "钩子不足");
        assert_eq!(issues[0].severity, "moderate");
    }

    #[test]
    fn rejects_review_prose_instead_of_silently_returning_no_issues() {
        let error = parse_review_issues("审校结果如下：[]").unwrap_err();
        assert!(error.to_string().contains("严格的问题数组"));
    }

    #[test]
    fn accepts_empty_review_array_as_a_valid_result() {
        assert!(parse_review_issues("[]").unwrap().is_empty());
    }

    #[test]
    fn streaming_request_sets_stream_flag() {
        let settings = AiSettings {
            base_url: "https://example.test/v1".to_string(),
            model: "model-a".to_string(),
            temperature: 0.7,
            thinking_enabled: false,
            thinking_level: "off".to_string(),
            has_api_key: true,
        };
        let mut request = build_chat_request(&settings, "sys", "user", 0.4);
        request.stream = Some(true);

        assert!(serde_json::to_value(request)
            .unwrap()
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap());
    }

    #[test]
    fn parses_models_response_from_array() {
        let raw = r#"[{"id":"deepseek-v4-flash","owned_by":"deepseek"}]"#;
        let models = parse_models_response(raw).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "deepseek-v4-flash");
    }

    #[test]
    fn normalizes_minimax_base_url_variants() {
        assert_eq!(
            normalize_base_url("https://api.minimax.com/v1"),
            "https://api.minimaxi.com/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.minimaxi.com"),
            "https://api.minimaxi.com/v1"
        );
    }

    #[test]
    fn native_tool_fallback_only_accepts_capability_errors() {
        assert!(native_tools_unsupported(
            400,
            r#"{"error":{"message":"unsupported parameter: tools"}}"#
        ));
        assert!(!native_tools_unsupported(
            401,
            r#"{"error":{"message":"unsupported parameter: tools"}}"#
        ));
        assert!(!native_tools_unsupported(
            429,
            r#"{"error":{"message":"tool_calls rate limit"}}"#
        ));
        assert!(!native_tools_unsupported(
            500,
            r#"{"error":{"message":"unknown field `tools`"}}"#
        ));
    }

    /// 流式分片里的工具调用必须按 index 归并：首片带 id/name，后续片只带参数片段。
    #[test]
    fn assembles_stream_tool_calls_from_deltas() {
        let mut ids = Vec::new();
        let mut names = Vec::new();
        let mut arguments = Vec::new();
        let first = serde_json::json!({
            "choices": [{ "delta": { "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": { "name": "submit_review_issues", "arguments": "" }
            }] } }]
        });
        collect_stream_delta_tool_calls(&first, &mut ids, &mut names, &mut arguments);
        let second = serde_json::json!({
            "choices": [{ "delta": { "tool_calls": [{
                "index": 0,
                "id": null,
                "function": { "name": null, "arguments": "{\"issues\":[]}" }
            }] } }]
        });
        collect_stream_delta_tool_calls(&second, &mut ids, &mut names, &mut arguments);

        let calls = assemble_stream_tool_calls(&ids, &names, &arguments);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "submit_review_issues");
        assert_eq!(calls[0].arguments, "{\"issues\":[]}");
    }

    /// 提交型工具的参数必须过与 JSON 数组路径同一套校验。
    #[test]
    fn validate_review_issues_rejects_incomplete_or_unverifiable_findings() {
        let good = ReviewIssue {
            issue_type: "事实越界".to_string(),
            severity: "major".to_string(),
            location: "第 1 段".to_string(),
            reason: "凭空新增".to_string(),
            suggestion: "删掉".to_string(),
            evidence_quote: "这是一段可回查的原文片段".to_string(),
            action_evidence_quote: String::new(),
        };
        assert!(validate_review_issues(&[good.clone()]).is_ok());

        let mut bad_severity = good.clone();
        bad_severity.severity = "严重".to_string();
        assert!(validate_review_issues(&[bad_severity]).is_err());

        let mut short_quote = good;
        short_quote.evidence_quote = "太短".to_string();
        assert!(validate_review_issues(&[short_quote]).is_err());
    }

    #[test]
    fn parses_native_and_structured_tool_calls() {
        let native = parse_native_tool_calls(
            r#"{"choices":[{"message":{"tool_calls":[{"id":"call-1","function":{"name":"history_context","arguments":"{\"query\":\"旧线索\"}"}}]}}]}"#,
        )
        .unwrap();
        assert_eq!(native[0].tool_key, "history_context");
        assert_eq!(native[0].arguments["query"], "旧线索");

        let structured = parse_structured_tool_calls(
            r#"```json
            {"calls":[{"call_id":"s-1","tool_key":"chapter_memory","arguments":{}}]}
            ```"#,
        )
        .unwrap();
        assert_eq!(structured[0].protocol, "structured");
        assert_eq!(structured[0].tool_key, "chapter_memory");
    }
}
