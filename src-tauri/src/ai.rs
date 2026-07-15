use reqwest::header::{HeaderMap, HeaderValue, ACCEPT_ENCODING};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use crate::{
    error::{AppError, AppResult},
    models::{AiSettings, ModelInfo, ModelsResponse, ReviewIssue},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingConfig {
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
const AI_RETRY_DELAY_MILLIS: u64 = 1_000;

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
    ChatCompletionRequest {
        model: settings.model.clone(),
        temperature,
        stream: None,
        max_completion_tokens: provider_max_completion_tokens(&settings.base_url),
        thinking: provider_thinking_config(&settings.base_url, settings.thinking_enabled),
        reasoning_split: provider_reasoning_split(&settings.base_url, settings.thinking_enabled),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
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
    let normalized_base_url = normalize_base_url(&settings.base_url);
    if normalized_base_url.is_empty() {
        return Err(AppError::Validation("请先设置 API Base URL".to_string()));
    }
    if settings.model.trim().is_empty() {
        return Err(AppError::Validation("请先设置模型名称".to_string()));
    }

    let request = build_chat_request(settings, system_prompt, user_prompt, temperature);
    let url = format!("{}/chat/completions", normalized_base_url);
    let client = build_client();
    let mut last_error = None;
    let max_attempts = MAX_AI_RETRIES + 1;

    for attempt in 1..=max_attempts {
        let response = match client
            .post(&url)
            .bearer_auth(api_key)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(AppError::Network(error));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = read_response_text(response).await.unwrap_or_default();
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

        let body = match read_response_text(response).await {
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
            Ok(content) => return Ok(content),
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
        AppError::Validation(format!("AI 没有返回可用内容；已重试 {} 次", MAX_AI_RETRIES))
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
    F: FnMut(&str) -> AppResult<()>,
{
    let normalized_base_url = normalize_base_url(&settings.base_url);
    if normalized_base_url.is_empty() {
        return Err(AppError::Validation("请先设置 API Base URL".to_string()));
    }
    if settings.model.trim().is_empty() {
        return Err(AppError::Validation("请先设置模型名称".to_string()));
    }

    let mut request = build_chat_request(settings, system_prompt, user_prompt, temperature);
    request.stream = Some(true);
    let url = format!("{}/chat/completions", normalized_base_url);
    let client = build_client();
    let max_attempts = MAX_AI_RETRIES + 1;
    let mut last_error = None;

    for attempt in 1..=max_attempts {
        let response = match client
            .post(&url)
            .bearer_auth(api_key)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                last_error = Some(AppError::Network(error));
                if should_retry(attempt, max_attempts) {
                    sleep(Duration::from_millis(AI_RETRY_DELAY_MILLIS)).await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = read_response_text(response).await.unwrap_or_default();
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

        let stream_result = match timeout(
            Duration::from_secs(AI_REQUEST_TIMEOUT_SECONDS),
            read_stream_completion(response, &mut on_update),
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
            Ok(content) => return Ok(content),
            Err(error) => {
                last_error = Some(error);
                if should_retry(attempt, max_attempts) {
                    on_update("")?;
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
    let client = build_client();
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
            let body = read_response_text(response).await.unwrap_or_default();
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

        let body = match read_response_text(response).await {
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

pub fn parse_review_issues(raw: &str) -> Vec<ReviewIssue> {
    let trimmed = raw.trim();
    if let Ok(json) = serde_json::from_str::<Vec<ReviewIssue>>(trimmed) {
        return json;
    }

    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            let slice = &trimmed[start..=end];
            if let Ok(json) = serde_json::from_str::<Vec<ReviewIssue>>(slice) {
                return json;
            }
        }
    }

    Vec::new()
}

fn build_client() -> reqwest::Client {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

    reqwest::Client::builder()
        .timeout(Duration::from_secs(AI_REQUEST_TIMEOUT_SECONDS))
        .http1_only()
        .default_headers(headers)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .expect("failed to build reqwest client")
}

async fn read_response_text(response: reqwest::Response) -> AppResult<String> {
    let bytes = response.bytes().await?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

async fn read_stream_completion<F>(
    mut response: reqwest::Response,
    on_update: &mut F,
) -> AppResult<String>
where
    F: FnMut(&str) -> AppResult<()>,
{
    let mut buffer = String::new();
    let mut raw_response = String::new();
    let mut content = String::new();
    let mut saw_sse_event = false;

    while let Some(chunk) = response.chunk().await? {
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
                continue;
            }
            let value: Value = serde_json::from_str(data)
                .map_err(|err| AppError::Validation(format!("无法解析流式 AI 响应：{err}")))?;
            if let Some(message) = value.get("error").and_then(extract_error_message) {
                return Err(AppError::Validation(format!("AI 返回错误：{message}")));
            }
            if let Some(delta) = extract_stream_delta_content(&value) {
                content.push_str(&delta);
                on_update(&content)?;
            }
        }
    }

    if !saw_sse_event {
        return parse_chat_completion(&raw_response);
    }
    let normalized = strip_think_blocks(&content);
    if normalized.trim().is_empty() {
        return Err(AppError::Validation("AI 没有返回可用内容".to_string()));
    }
    on_update(&normalized)?;
    Ok(normalized)
}

fn parse_chat_completion(raw: &str) -> AppResult<String> {
    let trimmed = trim_code_fence(raw);

    if let Ok(completion) = serde_json::from_str::<ChatCompletionResponse>(trimmed) {
        if let Some(content) = completion
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .filter(|content| !content.trim().is_empty())
        {
            return Ok(strip_think_blocks(&content));
        }
    }

    let value: Value = serde_json::from_str(trimmed)?;
    if let Some(message) = value.get("error").and_then(extract_error_message) {
        return Err(AppError::Validation(format!("AI 返回错误：{message}")));
    }

    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(extract_message_content)
        .ok_or_else(|| AppError::Validation("AI 没有返回可用内容".to_string()))?;

    if content.trim().is_empty() {
        return Err(AppError::Validation("AI 没有返回可用内容".to_string()));
    }

    Ok(strip_think_blocks(&content))
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
    fn parses_chat_completion_with_string_content() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"你好"}}]}"#;
        assert_eq!(parse_chat_completion(raw).unwrap(), "你好");
    }

    #[test]
    fn strips_think_blocks_from_chat_completion() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"<think>hidden</think>\n正文"}}]}"#;
        assert_eq!(parse_chat_completion(raw).unwrap(), "正文");
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
        assert_eq!(parse_chat_completion(raw).unwrap(), "第一段\n第二段");
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
    fn streaming_request_sets_stream_flag() {
        let settings = AiSettings {
            base_url: "https://example.test/v1".to_string(),
            model: "model-a".to_string(),
            temperature: 0.7,
            thinking_enabled: false,
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
}
