use std::time::Instant;

use serde::Deserialize;

use crate::{
    ai,
    chapter_memory::source_text_hash,
    db::AppState,
    error::{AppError, AppResult},
    models::{RebuildStoryIndexRequest, StoryIndexSummary},
};

pub const NORMALIZATION_VERSION: &str = "story-index-v1";
const INDEX_STAGE: &str = "story_index";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IndexedEntity {
    pub kind: String,
    pub name: String,
    pub evidence_quote: String,
}

impl IndexedEntity {
    pub(crate) fn key(&self) -> String {
        format!("{}:{}", self.kind, self.name)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IndexedParticipant {
    pub kind: String,
    pub name: String,
    pub role: String,
}

impl IndexedParticipant {
    pub(crate) fn key(&self) -> String {
        format!("{}:{}", self.kind, self.name)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IndexedEvent {
    pub title: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub story_time: String,
    pub summary: String,
    pub evidence_quote: String,
    #[serde(default)]
    pub participants: Vec<IndexedParticipant>,
}

impl IndexedEvent {
    pub(crate) fn key(&self) -> String {
        self.title.trim().to_string()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IndexedFact {
    pub entity_kind: String,
    pub entity_name: String,
    pub event_title: Option<String>,
    pub dimension: String,
    pub value: String,
    #[serde(default = "default_visibility")]
    pub visibility: String,
    pub evidence_quote: String,
}

impl IndexedFact {
    pub(crate) fn entity_key(&self) -> String {
        format!("{}:{}", self.entity_kind, self.entity_name)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawStoryIndex {
    #[serde(default)]
    entities: Vec<IndexedEntity>,
    #[serde(default)]
    events: Vec<IndexedEvent>,
    #[serde(default)]
    facts: Vec<IndexedFact>,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedChapter {
    pub entities: Vec<IndexedEntity>,
    pub events: Vec<IndexedEvent>,
    pub facts: Vec<IndexedFact>,
}

pub async fn index_approved_chapter(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<StoryIndexSummary> {
    index_approved_chapter_impl(state, project_id, chapter_id, true).await
}

pub(crate) async fn index_approved_chapter_for_job(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<StoryIndexSummary> {
    index_approved_chapter_impl(state, project_id, chapter_id, false).await
}

async fn index_approved_chapter_impl(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
    record_failure: bool,
) -> AppResult<StoryIndexSummary> {
    let chapter = state
        .ensure_chapter(project_id, Some(chapter_id))?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let source = state
        .latest_approved_chapter_body(project_id, chapter_id)?
        .ok_or_else(|| {
            AppError::Validation("章节还没有已通过正文，不能建立资料索引".to_string())
        })?;
    let source_hash = source_text_hash(&source.content);
    if state.story_index_source_is_current(
        project_id,
        chapter_id,
        source.id,
        &source_hash,
        NORMALIZATION_VERSION,
    )? {
        return Ok(StoryIndexSummary {
            project_id,
            chapter_id,
            source_artifact_id: source.id,
            entity_count: 0,
            event_count: 0,
            fact_count: 0,
            status: "already_current".to_string(),
        });
    }

    let settings = deterministic_settings(state.get_ai_settings()?);
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| {
            AppError::Validation("请先为当前供应商保存 AI API Key，才能更新资料索引".to_string())
        })?;
    let prompt = build_index_prompt(&chapter.title, &source.content);
    let started = Instant::now();
    let run = state.insert_workflow_run(
        project_id,
        Some(chapter_id),
        INDEX_STAGE,
        &prompt,
        "",
        "running",
        None,
        0,
    )?;

    let raw = match ai::complete_json_chat(
        &settings,
        &api_key,
        "你是小说连续性资料索引 Agent。只从已通过正文提取可验证的实体、事件和原子事实，不续写、不推断、不修改正文。",
        &prompt,
        0.0,
    )
    .await
    {
        Ok(raw) => raw,
        Err(error) => {
            let message = error.to_string();
            if record_failure {
                state.record_story_index_failure(
                    project_id,
                    chapter_id,
                    source.id,
                    &source_hash,
                    NORMALIZATION_VERSION,
                    &message,
                )?;
            }
            state.update_workflow_run(
                run.id,
                "",
                "failed",
                Some(&message),
                started.elapsed().as_millis() as i64,
            )?;
            return Err(error);
        }
    };

    let indexed = match parse_and_validate_index(&raw, &source.content) {
        Ok(indexed) => indexed,
        Err(error) => {
            let message = error.to_string();
            if record_failure {
                state.record_story_index_failure(
                    project_id,
                    chapter_id,
                    source.id,
                    &source_hash,
                    NORMALIZATION_VERSION,
                    &message,
                )?;
            }
            state.update_workflow_run(
                run.id,
                &raw,
                "failed",
                Some(&message),
                started.elapsed().as_millis() as i64,
            )?;
            return Err(error);
        }
    };

    state.replace_story_index_chapter_cas(
        project_id,
        chapter_id,
        source.id,
        &source_hash,
        NORMALIZATION_VERSION,
        &indexed,
    )?;
    let summary = StoryIndexSummary {
        project_id,
        chapter_id,
        source_artifact_id: source.id,
        entity_count: indexed.entities.len(),
        event_count: indexed.events.len(),
        fact_count: indexed.facts.len(),
        status: "success".to_string(),
    };
    state.update_workflow_run(
        run.id,
        &serde_json::to_string(&summary)?,
        "success",
        None,
        started.elapsed().as_millis() as i64,
    )?;
    Ok(summary)
}

pub async fn rebuild_story_index(
    state: &AppState,
    input: RebuildStoryIndexRequest,
) -> AppResult<Vec<StoryIndexSummary>> {
    state.get_project(input.project_id)?;
    let chapter_ids = if let Some(chapter_id) = input.chapter_id {
        vec![chapter_id]
    } else {
        state
            .list_chapters(input.project_id)?
            .into_iter()
            .map(|chapter| chapter.id)
            .collect()
    };
    let mut summaries = Vec::new();
    for chapter_id in chapter_ids {
        if state
            .latest_approved_chapter_body(input.project_id, chapter_id)?
            .is_some()
        {
            summaries.push(index_approved_chapter(state, input.project_id, chapter_id).await?);
        }
    }
    Ok(summaries)
}

fn build_index_prompt(title: &str, content: &str) -> String {
    format!(
        r#"# 任务
从一章已经人工通过的正式正文中，提取下一章和后续章节需要查询的结构化资料。
这是派生索引，不是新设定。只记录正文明确发生或明确确认的内容。

# 总原则
1. 每条实体、事件、事实都必须有正文中的连续引文。
2. 不把人物猜测、旁白疑问、传闻、计划写成世界真实；可以用 visibility=rumor 保留其性质。
3. 不补写引文没有给出的身份、来源、所有权、时间、动机、数量或因果。
4. 同一章只记录对后续章节有用的实体和变化，不要把普通场景细节全部入库。
5. story_time 可以填写“三日后”“当夜”“大比前”“未知”，不能自行编造日期。
6. 事件的 summary 必须是引文能直接证明的一句话；不确定就省略事件。

# 实体 kind
character、item、resource、location、faction、technique、condition、other。

# 事实 dimension
location、possession、condition、cultivation、injury、ability、knowledge、relationship、objective、resource、availability、status、other。

# visibility
world、character、reader、rumor。角色知道什么必须在 value 中写明角色；不能把读者看到的内容自动当成角色知道。

# JSON 结构
{{
  "entities": [{{"kind":"character","name":"正文中的名称","evidence_quote":"连续原文"}}],
  "events": [{{"title":"事件标题","kind":"conflict|transaction|discovery|travel|training|revelation|other","status":"occurred|ongoing|revealed","story_time":"","summary":"一句可被引文证明的摘要","evidence_quote":"连续原文","participants":[{{"kind":"character","name":"人物名","role":"参与角色"}}]}}],
  "facts": [{{"entity_kind":"item","entity_name":"实体名","event_title":"关联事件标题或 null","dimension":"possession","value":"明确状态","visibility":"world","evidence_quote":"连续原文"}}]
}}

# 当前章节
{title}

# 已通过正式正文
{content}"#,
    )
}

fn parse_and_validate_index(raw: &str, source: &str) -> AppResult<IndexedChapter> {
    let json = extract_json_object(raw)
        .ok_or_else(|| AppError::Validation("资料索引没有返回 JSON 对象".to_string()))?;
    let mut parsed: RawStoryIndex = serde_json::from_str(json)?;

    parsed.entities.retain(|entity| {
        valid_kind(&entity.kind)
            && valid_name(&entity.name)
            && valid_quote(&entity.evidence_quote, source)
            && entity.evidence_quote.contains(entity.name.trim())
    });
    dedup_entities(&mut parsed.entities);

    parsed.events.retain(|event| {
        valid_name(&event.title)
            && valid_event_status(&event.status)
            && valid_quote(&event.evidence_quote, source)
            && event.summary.chars().count() <= 180
    });
    for event in &mut parsed.events {
        event.participants.retain(|participant| {
            valid_kind(&participant.kind)
                && valid_name(&participant.name)
                && !participant.role.trim().is_empty()
                && event.evidence_quote.contains(participant.name.trim())
                && parsed.entities.iter().any(|entity| {
                    entity.kind == participant.kind && entity.name == participant.name
                })
        });
    }

    parsed.facts.retain(|fact| {
        valid_kind(&fact.entity_kind)
            && valid_name(&fact.entity_name)
            && valid_dimension(&fact.dimension)
            && valid_visibility(&fact.visibility)
            && !fact.value.trim().is_empty()
            && fact.value.chars().count() <= 180
            && valid_quote(&fact.evidence_quote, source)
            && fact.evidence_quote.contains(fact.entity_name.trim())
            && parsed
                .entities
                .iter()
                .any(|entity| entity.kind == fact.entity_kind && entity.name == fact.entity_name)
    });

    parsed
        .events
        .retain(|event| !event.participants.is_empty() || !event.summary.trim().is_empty());
    if parsed.entities.len() + parsed.events.len() + parsed.facts.len() > 160 {
        return Err(AppError::Validation(
            "资料索引条目过多，请降低提取范围后重试".to_string(),
        ));
    }
    Ok(IndexedChapter {
        entities: parsed.entities,
        events: parsed.events,
        facts: parsed.facts,
    })
}

fn dedup_entities(entities: &mut Vec<IndexedEntity>) {
    let mut seen = std::collections::HashSet::new();
    entities.retain(|entity| seen.insert(entity.key()));
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end >= start).then_some(&raw[start..=end])
}

fn valid_quote(quote: &str, source: &str) -> bool {
    let length = quote.trim().chars().count();
    (8..=240).contains(&length) && source.contains(quote.trim())
}

fn valid_name(value: &str) -> bool {
    let length = value.trim().chars().count();
    (2..=48).contains(&length) && !value.trim().contains(['\n', '\r'])
}

fn valid_kind(value: &str) -> bool {
    matches!(
        value,
        "character"
            | "item"
            | "resource"
            | "location"
            | "faction"
            | "technique"
            | "condition"
            | "other"
    )
}

fn valid_event_status(value: &str) -> bool {
    matches!(value, "occurred" | "ongoing" | "revealed")
}

fn valid_dimension(value: &str) -> bool {
    matches!(
        value,
        "location"
            | "possession"
            | "condition"
            | "cultivation"
            | "injury"
            | "ability"
            | "knowledge"
            | "relationship"
            | "objective"
            | "resource"
            | "availability"
            | "status"
            | "other"
    )
}

fn valid_visibility(value: &str) -> bool {
    matches!(value, "world" | "character" | "reader" | "rumor")
}

fn default_visibility() -> String {
    "world".to_string()
}

fn deterministic_settings(mut settings: crate::models::AiSettings) -> crate::models::AiSettings {
    settings.thinking_enabled = false;
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unverifiable_index_rows() {
        let raw = r#"{
            "entities":[
                {"kind":"character","name":"沈砚","evidence_quote":"沈砚握住裂纹玉牌。"},
                {"kind":"item","name":"玄铁令","evidence_quote":"没有这句话"}
            ],
            "events":[],
            "facts":[
                {"entity_kind":"character","entity_name":"沈砚","event_title":null,"dimension":"status","value":"握住玉牌","visibility":"world","evidence_quote":"沈砚握住裂纹玉牌。"}
            ]
        }"#;
        let result = parse_and_validate_index(raw, "沈砚握住裂纹玉牌。").unwrap();
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.facts.len(), 1);
    }
}
