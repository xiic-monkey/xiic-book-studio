use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ai,
    db::AppState,
    error::{AppError, AppResult},
    models::{AiSettings, Chapter, ChapterMemoryEntry, ChapterMemoryRecord, ChapterMemoryView},
};

pub const NORMALIZATION_VERSION: &str = "chapter-text-v8";

pub fn is_enabled() -> bool {
    std::env::var("XIIC_CHAPTER_MEMORY")
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off"
            )
        })
        .unwrap_or(true)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MemoryEntry {
    pub text: String,
    pub evidence_quote: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ChapterMemoryPayload {
    pub summary: Option<MemoryEntry>,
    pub final_scene: Option<MemoryEntry>,
    pub last_action: Option<MemoryEntry>,
    pub state_changes: Vec<MemoryEntry>,
    pub knowledge_changes: Vec<MemoryEntry>,
    pub commitments: Vec<MemoryEntry>,
    pub open_loops: Vec<MemoryEntry>,
    pub immediate_next_intent: Option<MemoryEntry>,
    pub plan_reconciliation: Vec<MemoryEntry>,
}

pub fn normalize_source_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn source_text_hash(text: &str) -> String {
    let digest = Sha256::digest(normalize_source_text(text).as_bytes());
    format!("{digest:x}")
}

pub fn current_memory_for_chapter(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<Option<ChapterMemoryRecord>> {
    let Some(source) = state.latest_approved_chapter_body(project_id, chapter_id)? else {
        return Ok(None);
    };
    let Some(memory) = state.get_chapter_memory(project_id, chapter_id)? else {
        return Ok(None);
    };
    let current_hash = source_text_hash(&source.content);
    if memory.source_artifact_id != source.id
        || memory.source_text_hash != current_hash
        || memory.normalization_version != NORMALIZATION_VERSION
    {
        return Ok(None);
    }
    Ok(Some(memory))
}

pub async fn ensure_predecessor_memory(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    settings: &AiSettings,
    api_key: &str,
    previous_plan: Option<&str>,
    system_prompt: &str,
) -> AppResult<Option<ChapterMemoryRecord>> {
    let chapters = state.list_chapters(project_id)?;
    let Some(current) = chapters
        .iter()
        .find(|chapter| chapter.id == current_chapter_id)
    else {
        return Ok(None);
    };
    let Some(predecessor) = chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < current.chapter_no)
        .max_by_key(|chapter| chapter.chapter_no)
    else {
        return Ok(None);
    };
    if state
        .latest_approved_chapter_body(project_id, predecessor.id)?
        .is_none()
    {
        return Ok(None);
    }
    if let Some(memory) = current_memory_for_chapter(state, project_id, predecessor.id)? {
        return Ok(Some(memory));
    }

    rebuild_chapter_memory(
        state,
        project_id,
        predecessor.id,
        settings,
        api_key,
        previous_plan,
        system_prompt,
    )
    .await
    .map(Some)
}

/// 列出全书与当前已采用正文一致的章节记忆（按章节顺序）。
/// 正文被替换后旧记忆自动不再展示，避免界面显示过期事实。
pub fn list_current_memories(
    state: &AppState,
    project_id: i64,
) -> AppResult<Vec<ChapterMemoryView>> {
    let chapters = state.list_chapters(project_id)?;
    let mut views = Vec::new();
    for chapter in chapters {
        let Some(record) = current_memory_for_chapter(state, project_id, chapter.id)? else {
            continue;
        };
        views.push(build_memory_view(&chapter, &record));
    }
    Ok(views)
}

fn build_memory_view(chapter: &Chapter, record: &ChapterMemoryRecord) -> ChapterMemoryView {
    let payload: ChapterMemoryPayload = serde_json::from_str(&record.content).unwrap_or_default();
    ChapterMemoryView {
        chapter_id: chapter.id,
        chapter_no: chapter.chapter_no,
        chapter_title: chapter.title.clone(),
        source_artifact_id: record.source_artifact_id,
        updated_at: record.updated_at.clone(),
        summary: payload.summary.map(to_view_entry),
        final_scene: payload.final_scene.map(to_view_entry),
        last_action: payload.last_action.map(to_view_entry),
        state_changes: payload
            .state_changes
            .into_iter()
            .map(to_view_entry)
            .collect(),
        knowledge_changes: payload
            .knowledge_changes
            .into_iter()
            .map(to_view_entry)
            .collect(),
        commitments: payload.commitments.into_iter().map(to_view_entry).collect(),
        open_loops: payload.open_loops.into_iter().map(to_view_entry).collect(),
        immediate_next_intent: payload.immediate_next_intent.map(to_view_entry),
        plan_reconciliation: payload
            .plan_reconciliation
            .into_iter()
            .map(to_view_entry)
            .collect(),
    }
}

fn to_view_entry(entry: MemoryEntry) -> ChapterMemoryEntry {
    ChapterMemoryEntry {
        text: entry.text,
        evidence_quote: entry.evidence_quote,
    }
}

pub async fn generate_for_approved_chapter(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<Option<ChapterMemoryRecord>> {
    if !is_enabled() {
        return Ok(None);
    }
    if current_memory_for_chapter(state, project_id, chapter_id)?.is_some() {
        return Ok(None);
    }
    let agent = state.get_agent("chapter_memory")?;
    let settings = agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先为章节记忆 Agent 配置 API Key".to_string()))?;
    rebuild_chapter_memory(
        state,
        project_id,
        chapter_id,
        &settings,
        &api_key,
        None,
        &agent.system_prompt,
    )
    .await
    .map(Some)
}

pub async fn rebuild_chapter_memory(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
    settings: &AiSettings,
    api_key: &str,
    chapter_plan: Option<&str>,
    system_prompt: &str,
) -> AppResult<ChapterMemoryRecord> {
    let chapter = state
        .ensure_chapter(project_id, Some(chapter_id))?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let source = state
        .latest_approved_chapter_body(project_id, chapter_id)?
        .ok_or_else(|| AppError::Validation("章节暂无正文".to_string()))?;
    let expected_hash = source_text_hash(&source.content);

    let prompt = build_extraction_prompt(
        &chapter.title,
        chapter_plan.unwrap_or_default(),
        &normalize_source_text(&source.content),
    );
    let started = Instant::now();
    let run = state.insert_workflow_run(
        project_id,
        Some(chapter_id),
        "chapter_memory",
        &prompt,
        "",
        "running",
        None,
        0,
    )?;

    let raw = match ai::complete_json_chat(settings, api_key, system_prompt, &prompt, 0.1).await {
        Ok(raw) => raw,
        Err(error) => {
            state.update_workflow_run(
                run.id,
                "",
                "failed",
                Some(&error.to_string()),
                started.elapsed().as_millis() as i64,
            )?;
            return Err(error);
        }
    };

    let payload = match parse_and_validate_memory(&raw, &source.content) {
        Ok(payload) => payload,
        Err(error) => {
            state.update_workflow_run(
                run.id,
                &raw,
                "failed",
                Some(&error.to_string()),
                started.elapsed().as_millis() as i64,
            )?;
            return Err(error);
        }
    };
    let content = serde_json::to_string(&payload)?;
    let memory = state.upsert_chapter_memory_cas(
        project_id,
        chapter_id,
        source.id,
        &expected_hash,
        NORMALIZATION_VERSION,
        &content,
    )?;
    state.update_workflow_run(
        run.id,
        &content,
        "success",
        None,
        started.elapsed().as_millis() as i64,
    )?;
    Ok(memory)
}

fn build_extraction_prompt(chapter_title: &str, chapter_plan: &str, chapter_text: &str) -> String {
    format!(
        r#"# 章节标题
{chapter_title}

# 原章节计划
{chapter_plan}

# 正文全文
{chapter_text}"#
    )
}

fn parse_and_validate_memory(raw: &str, source_text: &str) -> AppResult<ChapterMemoryPayload> {
    let json = extract_json_object(raw)
        .ok_or_else(|| AppError::Validation("章节交接记忆没有返回 JSON 对象".to_string()))?;
    let parsed: ChapterMemoryPayload = serde_json::from_str(json)?;
    let validated = validate_payload(parsed, source_text);
    if !validated.has_content() {
        return Err(AppError::Validation(
            "章节交接记忆没有可在正文中验证的内容".to_string(),
        ));
    }
    Ok(validated)
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end >= start).then_some(&raw[start..=end])
}

fn validate_payload(mut payload: ChapterMemoryPayload, source_text: &str) -> ChapterMemoryPayload {
    payload.summary = None;
    payload.final_scene = validate_optional(payload.final_scene, source_text).map(|mut entry| {
        entry.text.clone_from(&entry.evidence_quote);
        entry
    });
    payload.last_action = validate_optional(payload.last_action, source_text);
    payload.immediate_next_intent = validate_optional(payload.immediate_next_intent, source_text);
    validate_entries(&mut payload.state_changes, source_text, 6, false);
    validate_entries(&mut payload.knowledge_changes, source_text, 4, true);
    validate_entries(&mut payload.commitments, source_text, 3, false);
    validate_entries(&mut payload.open_loops, source_text, 4, false);
    payload.plan_reconciliation.clear();
    payload
}

fn validate_optional(entry: Option<MemoryEntry>, source_text: &str) -> Option<MemoryEntry> {
    entry.filter(|entry| entry_is_verifiable(entry, source_text))
}

fn validate_entries(
    entries: &mut Vec<MemoryEntry>,
    source_text: &str,
    limit: usize,
    reject_inferences: bool,
) {
    entries.retain(|entry| {
        entry_is_verifiable(entry, source_text)
            && entry_is_atomic(entry)
            && claim_terms_are_supported(entry)
            && (!reject_inferences
                || (!contains_inference_marker(&entry.text)
                    && knowledge_subject_is_supported(entry)))
    });
    entries.truncate(limit);
}

fn entry_is_atomic(entry: &MemoryEntry) -> bool {
    let text = entry.text.trim();
    !text.contains('；')
        && ![
            "并且", "并将", "并把", "且将", "且把", "同时", "以及", "随后", "后来", "又将", "又把",
        ]
        .iter()
        .any(|marker| text.contains(marker))
}

fn claim_terms_are_supported(entry: &MemoryEntry) -> bool {
    [
        "留下", "交给", "藏在", "属于", "来自", "持有", "拥有", "带人", "杀死", "处理", "截留",
    ]
    .iter()
    .all(|marker| !entry.text.contains(marker) || entry.evidence_quote.contains(marker))
}

fn knowledge_subject_is_supported(entry: &MemoryEntry) -> bool {
    let text = entry.text.trim();
    let quote = entry.evidence_quote.trim();
    let cognition_markers = ["知道", "得知", "确认", "发现", "看见", "听见", "意识到"];
    let Some((subject, _)) = cognition_markers
        .iter()
        .find_map(|marker| text.split_once(marker))
    else {
        return true;
    };
    let subject = subject.trim();
    !subject.is_empty()
        && quote.contains(subject)
        && cognition_markers
            .iter()
            .any(|marker| quote.contains(marker))
}

fn contains_inference_marker(text: &str) -> bool {
    [
        "推测", "推断", "猜测", "可能", "或许", "似乎", "应该", "大概", "疑似",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn entry_is_verifiable(entry: &MemoryEntry, source_text: &str) -> bool {
    if entry.text.trim().is_empty() {
        return false;
    }
    let quote = normalize_evidence(&entry.evidence_quote);
    let quote_chars = quote.chars().count();
    (6..=180).contains(&quote_chars) && normalize_evidence(source_text).contains(&quote)
}

fn normalize_evidence(text: &str) -> String {
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

impl ChapterMemoryPayload {
    fn has_content(&self) -> bool {
        self.summary.is_some()
            || self.final_scene.is_some()
            || self.last_action.is_some()
            || self.immediate_next_intent.is_some()
            || !self.state_changes.is_empty()
            || !self.knowledge_changes.is_empty()
            || !self.commitments.is_empty()
            || !self.open_loops.is_empty()
            || !self.plan_reconciliation.is_empty()
    }
}

pub fn render_memory_context(record: &ChapterMemoryRecord) -> AppResult<String> {
    let payload: ChapterMemoryPayload = serde_json::from_str(&record.content)?;
    let mut lines = vec![format!(
        "来源：正文 artifact {} / SHA-256 {} / {}",
        record.source_artifact_id,
        &record.source_text_hash[..record.source_text_hash.len().min(12)],
        record.normalization_version
    )];
    push_optional(&mut lines, "本章实际摘要", payload.summary.as_ref());
    push_optional(&mut lines, "结尾场景", payload.final_scene.as_ref());
    push_optional(&mut lines, "最后动作", payload.last_action.as_ref());
    push_entries(&mut lines, "状态变化", &payload.state_changes);
    push_entries(&mut lines, "角色认知变化", &payload.knowledge_changes);
    push_entries(&mut lines, "承诺与决定", &payload.commitments);
    push_entries(&mut lines, "未闭环事项", &payload.open_loops);
    push_optional(
        &mut lines,
        "章末下一步意图",
        payload.immediate_next_intent.as_ref(),
    );
    push_entries(&mut lines, "计划正文对账", &payload.plan_reconciliation);
    Ok(lines.join("\n"))
}

fn push_optional(lines: &mut Vec<String>, label: &str, entry: Option<&MemoryEntry>) {
    if let Some(entry) = entry {
        lines.push(format!(
            "- {label}：{}｜证据：“{}”",
            entry.text.trim(),
            entry.evidence_quote.trim()
        ));
    }
}

fn push_entries(lines: &mut Vec<String>, label: &str, entries: &[MemoryEntry]) {
    for entry in entries {
        lines.push(format!(
            "- {label}：{}｜证据：“{}”",
            entry.text.trim(),
            entry.evidence_quote.trim()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_hash_normalizes_line_endings_and_trailing_space() {
        assert_eq!(
            source_text_hash("第一行  \r\n第二行\r\n"),
            source_text_hash("第一行\n第二行")
        );
    }

    #[test]
    fn rejects_memory_entries_without_source_evidence() {
        let raw = r#"{
          "summary":{"text":"陆烬拿走黑牌","evidence_quote":"陆烬把黑牌收进怀里"},
          "state_changes":[
            {"text":"陆烬受伤","evidence_quote":"陆烬左腕裂口又深了一寸"},
            {"text":"韩厉得知秘密","evidence_quote":"韩厉已经知道全部真相"}
          ]
        }"#;
        let source = "陆烬把黑牌收进怀里。陆烬左腕裂口又深了一寸。";
        let memory = parse_and_validate_memory(raw, source).unwrap();
        assert!(memory.summary.is_none());
        assert_eq!(memory.state_changes.len(), 1);
        assert_eq!(memory.state_changes[0].text, "陆烬受伤");
    }

    #[test]
    fn extraction_prompt_requires_atomic_supported_facts() {
        let prompt = build_extraction_prompt("第四章", "夺取赤髓", "陆烬拿到赤髓。\n");
        let contract = crate::prompt_templates::require_default_prompt("chapter_memory").unwrap();

        assert!(contract.contains("每条记录只表达一个原子事实"));
        assert!(contract.contains("命令、计划、传闻必须标明其性质"));
        assert!(contract.contains("物件记录必须区分"));
        assert!(contract.contains("互相冲突的数量或状态"));
        assert!(contract.contains("只删除有争议的字段"));
        assert!(contract.contains("正文最后三分之一倒序检查"));
        assert!(contract.contains("引文必须能够单独证明整条"));
        assert!(contract.contains("遮住完整正文"));
        // 用户 prompt 只承载运行时数据；固定契约必须全部位于 system prompt 模板。
        assert!(prompt.contains("# 章节标题"));
        assert!(prompt.contains("第四章"));
        assert!(prompt.contains("夺取赤髓"));
        assert!(!prompt.contains("每条记录只表达一个原子事实"));
    }

    #[test]
    fn validator_drops_compound_and_inferred_entries() {
        let raw = r#"{
          "summary":{"text":"本章完成全部任务","evidence_quote":"陆烬拿到赤髓"},
          "state_changes":[
            {"text":"陆烬拿到赤髓，并将它藏入怀中","evidence_quote":"陆烬拿到赤髓，并将它藏入怀中。"},
            {"text":"陆烬左腕疼痛减轻三分","evidence_quote":"陆烬左腕疼痛减轻三分。"}
          ],
          "knowledge_changes":[
            {"text":"陆烬推断周三已经遇害","evidence_quote":"陆烬推断周三已经遇害。"},
            {"text":"陆烬听见药房今夜关闭","evidence_quote":"陆烬听见管事说药房今夜关闭。"}
          ],
          "plan_reconciliation":[
            {"text":"计划已经完成","evidence_quote":"陆烬拿到赤髓。"}
          ]
        }"#;
        let source = "陆烬拿到赤髓，并将它藏入怀中。陆烬左腕疼痛减轻三分。陆烬推断周三已经遇害。陆烬听见管事说药房今夜关闭。";

        let memory = parse_and_validate_memory(raw, source).unwrap();

        assert!(memory.summary.is_none());
        assert_eq!(memory.state_changes.len(), 1);
        assert_eq!(memory.knowledge_changes.len(), 1);
        assert!(memory.plan_reconciliation.is_empty());
    }

    #[test]
    fn validator_uses_verbatim_final_scene_and_requires_grounded_knowledge_subject() {
        let raw = r#"{
          "final_scene":{"text":"陆烬去往废料间","evidence_quote":"他走进雾里。身后草棚空了。"},
          "knowledge_changes":[
            {"text":"陆烬知道赵执事要封炉","evidence_quote":"“今天就封炉。”"},
            {"text":"陆烬听见赵执事下令封炉","evidence_quote":"陆烬躲在墙后，听见赵执事下令封炉。"}
          ]
        }"#;
        let source = "“今天就封炉。”陆烬躲在墙后，听见赵执事下令封炉。他走进雾里。身后草棚空了。";

        let memory = parse_and_validate_memory(raw, source).unwrap();

        assert_eq!(
            memory.final_scene.unwrap().text,
            "他走进雾里。身后草棚空了。"
        );
        assert_eq!(memory.knowledge_changes.len(), 1);
        assert_eq!(memory.knowledge_changes[0].text, "陆烬听见赵执事下令封炉");
    }

    #[test]
    fn validator_rejects_unsupported_provenance_claims() {
        let raw = r#"{
          "state_changes":[
            {"text":"陆烬发现周三留下的布片","evidence_quote":"布片上刻着周三的名字。"},
            {"text":"陆烬发现布片上刻着周三的名字","evidence_quote":"布片上刻着周三的名字。"}
          ]
        }"#;
        let source = "布片上刻着周三的名字。";

        let memory = parse_and_validate_memory(raw, source).unwrap();

        assert_eq!(memory.state_changes.len(), 1);
        assert_eq!(memory.state_changes[0].text, "陆烬发现布片上刻着周三的名字");
    }

    #[test]
    fn builds_display_view_from_stored_memory_payload() {
        let chapter = Chapter {
            id: 14,
            project_id: 8,
            chapter_no: 4,
            title: "夜雾入谷".to_string(),
            status: "approved".to_string(),
            current_artifact_id: Some(62),
            created_at: "2026-07-17T00:00:00Z".to_string(),
            updated_at: "2026-07-17T00:00:00Z".to_string(),
        };
        let record = ChapterMemoryRecord {
            id: 3,
            project_id: 8,
            chapter_id: 14,
            source_artifact_id: 62,
            source_text_hash: "hash".to_string(),
            normalization_version: NORMALIZATION_VERSION.to_string(),
            content: r#"{
              "summary":{"text":"陆烬走进夜雾","evidence_quote":"他走进雾里。"},
              "state_changes":[{"text":"陆烬把铜炉装进布袋","evidence_quote":"塞进布袋。"}],
              "open_loops":[{"text":"赵执事下令封炉","evidence_quote":"都闷死在里面。"}]
            }"#
            .to_string(),
            created_at: "2026-07-17T16:45:15Z".to_string(),
            updated_at: "2026-07-17T16:45:15Z".to_string(),
        };

        let view = build_memory_view(&chapter, &record);

        assert_eq!(view.chapter_no, 4);
        assert_eq!(view.chapter_title, "夜雾入谷");
        assert_eq!(view.summary.unwrap().text, "陆烬走进夜雾");
        assert_eq!(view.state_changes.len(), 1);
        assert_eq!(view.open_loops[0].evidence_quote, "都闷死在里面。");
        assert!(view.commitments.is_empty());
    }
}
