use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ai,
    db::AppState,
    error::{AppError, AppResult},
    models::{AiSettings, ChapterMemoryRecord},
};

pub const NORMALIZATION_VERSION: &str = "chapter-text-v8";

pub fn is_enabled() -> bool {
    std::env::var("XIIC_CHAPTER_MEMORY")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false)
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
) -> AppResult<ChapterMemoryRecord> {
    let chapter = state
        .ensure_chapter(project_id, Some(chapter_id))?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let source = state
        .latest_approved_chapter_body(project_id, chapter_id)?
        .ok_or_else(|| AppError::Validation("章节缺少已通过正文".to_string()))?;
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

    let raw = match ai::complete_json_chat(
        settings,
        api_key,
        "你是长篇小说的事实交接记录员。宁可漏记，也不能推断、合并或补写正文没有明确证明的事实。",
        &prompt,
        0.1,
    )
    .await
    {
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
        r#"# 任务
从完整章节正文中生成供下一章使用的连续性交接记忆。它是可重建的派生资料，不是新设定。

# 首要原则
- 宁缺勿滥。它会被下一章当作事实使用，因此漏掉一条次要信息，好过写入一条不确定信息。
- `text` 必须是 `evidence_quote` 的直接、保守改写。引文必须能够单独证明整条 `text`，不能只证明其中一半。
- 每条记录只表达一个原子事实。禁止在同一条里拼接多个物件、多个动作、多个时间点或多个推论。

# 事实边界
1. 只记录正文已经发生、明确说出或明确决定的内容。人物猜测、威胁、命令、计划、传闻必须标明其性质，不能改写成已经发生的结果。
2. 不得推测人物隐藏动机、物件来源、所有权、使用方式、幕后真相或未来剧情。
3. 物件记录必须区分：谁发现、来源何处、当前由谁持有、存放在哪里、数量多少。正文没有明确写出的字段就不要补。
   - 物件刻有某人的名字，不等于该物件由此人留下；正文没有“留下、交给、藏在”等明确动作时，只写主角发现了什么。
4. 数量、期限、伤势、位置、门禁和物件状态必须采用正文时间顺序中最后一次明确确认的状态。
5. 如果正文对同一事实出现互相冲突的数量或状态，不得自行选择其一，也不得同时写入两种说法。只删除有争议的字段，保留双方都能证明的最低事实。
   - 例如数量一处写六件、一处写七件，但两处都证明角色仍持有该物件，只写“角色仍持有该物件”，不写数量。
   - 如果连“是否持有、是否存活、是否开启”等核心状态也冲突，才省略整条事实。
6. `knowledge_changes` 必须写清楚是谁知道了什么；作者叙述给读者的信息，不等于角色已经知道。
   - 禁止使用“可能、或许、推测、似乎、应该、显然”等推断词补知识。正文只给线索时，不要替角色得出结论。
   - 引文必须同时包含认知主体和“看见、听见、得知、确认、意识到”等认知动作。只引用一段别人说的话，不能证明主角听见或相信了这段话。
7. `commitments` 只记录明确说出口或明确执行的决定。被迫答应、他人下令、暂时打算要如实表述，不能写成自愿承诺。
8. `open_loops` 只记录章末仍未解决的问题。某人下令要做某事，只能写成“某人下令/威胁”，不能写成该结果已经发生。

# 章末锚点
1. `final_scene` 只截取正文结尾能够独立成立的连续原文；`text` 与 `evidence_quote` 必须逐字相同，后端会强制用原文覆盖概括。
   - `final_scene` 不得夹带目的地、计划或下一步意图；这些内容只能放在 `immediate_next_intent`。
2. `last_action` 必须对应正文最后一个已经发生的动作，并直接引用包含该动作的原句。
3. `immediate_next_intent` 仅在章末明确写出下一步行动时填写；只有方向性愿望或推测时返回 null。
4. `summary` 只概括本章已经完成的一个核心变化。若不存在一段能直接支撑它的连续引文，返回 null。

# 引文规则
1. 每个结论必须附一段正文逐字 `evidence_quote`，长度 6-180 字，必须连续出现，不能改写、拼接或省略号连接。
2. 引文中的主语、对象、数量、状态和时间必须足以支撑 `text`；任何一项需要依赖另一段正文才能成立，就缩短 `text` 或不记录。
3. 不要用物件清单证明物件来源，不要用人物台词证明台词内容已经成为事实，不要用计划证明计划已经执行。

# 选择范围
- `summary` 固定返回 null。单段引文通常无法证明整章摘要，不要冒险概括。
- `state_changes` 最多 6 条、`knowledge_changes` 最多 4 条、`commitments` 最多 3 条、`open_loops` 最多 4 条，只保留直接影响下一章的内容。
- `plan_reconciliation` 固定返回空数组。规划对账不进入续写事实。
- 不确定字段使用 null 或空数组，不要为了填满字段而猜测。

# 覆盖检查
按下面顺序通读正文查漏。正文明确发生且下一章仍有影响时，优先记录；某类没有变化可以为空。
1. 先从正文最后三分之一倒序检查人物离场时的最终状态，再回看前文解释变化来源。不得把章中过程状态当成章末库存。
2. 资源与物件：取得、失去、消耗、损坏、藏匿、剩余数量、当前持有者和离场时携带的关键工具。一次取得一组资源可以是一条事实，但不要把“取得”和“随后使用”合并。
   - 取得后已被消耗：优先记录“已经消耗”和实际效果，不再把它写成当前持有。
   - 数量冲突但仍明确存在：记录不带数量的持有、剩余或不可用状态。
   - 正文明确写出角色带着工具或物件离场时，该离场装备优先于早先放在房间、床板或仓库里的中间状态。
3. 人物状态：伤势、修为、体力、暴露程度、关系和身份状态的实际变化。
4. 场景状态：人物当前位置、门禁、封锁、阵法、道路和关键设施的最终状态。
5. 外部压力：已经说出的命令、追查、期限、交易和威胁。命令仍写成命令，不得写成执行结果。
6. 下一步衔接：最后动作、仍未解决的近程问题，以及章末明确准备执行的动作。
7. 明确认知：只有引文同时写出认知主体和认知动作时才进入 `knowledge_changes`。不能满足时宁可不记，不要把该信息从其他分类重复一遍。

`state_changes` 最终自检：如果正文明确写了章末库存或离场装备，至少保留一条资源/物件状态；如果正文明确写了伤势变化，至少保留一条身体状态；同一类最多两条，避免身体状态挤掉资源和场景状态。

# 输出前强制自检
1. 删除任何需要两段以上正文才能证明的条目。
2. 删除任何同时包含两个以上动作或前后状态变化的条目。例如“甲拿到药并把药交给乙”必须拆成两条，各自有直接引文。
   - `text` 出现“并、同时、以及、又”时重新检查；若连接的是两个动作或两个结论，必须拆分或删除。
   - 同一个动作涉及一组物品时可以保留在一条中，例如“甲把丹炉、火石和断刀装进布袋”；物品清单不是多个状态变化。
3. 删除任何把“猜测、命令、威胁、计划”改写成“已经发生”的条目。
4. `evidence_quote` 必须从正文连续复制，保留正文原有的中文引号 `“”`。不要在 JSON 字符串内部使用未转义的 ASCII 双引号。
5. 最后检查输出能被标准 JSON 解析器直接解析。只输出 JSON 对象，不要 Markdown 代码围栏，不要解释。
6. 完成草稿后，在心里逐条遮住完整正文，只看该条 `evidence_quote`：若仍不能证明 `text` 的每个词，删除该条，不要寻找第二段引文补证。
7. `text` 必须写成一个简单句。出现“并、且、同时、以及、又、后来、随后”等连接两个独立动作的词时，拆分；没有各自独立引文就只保留更重要的一条。逗号可用于补充同一状态，顿号可用于同一动作下的物品清单。
8. “留下、交给、藏在、属于、来自、持有、杀死、处理”等来源、所有权和结果词，必须在本条引文中有同义的明确动作。物件上的姓名、刻字和编号不能证明这些关系。

# JSON 结构
{{
  "summary": {{"text":"本章实际进展摘要","evidence_quote":"正文逐字引文"}},
  "final_scene": {{"text":"结尾场景、时间与在场人物","evidence_quote":"正文逐字引文"}},
  "last_action": {{"text":"最后一个已经发生的明确动作","evidence_quote":"正文逐字引文"}},
  "state_changes": [{{"text":"人物、伤势、位置、资源、物件或禁制状态变化","evidence_quote":"正文逐字引文"}}],
  "knowledge_changes": [{{"text":"某角色在本章新知道或确认的信息","evidence_quote":"正文逐字引文"}}],
  "commitments": [{{"text":"角色作出的承诺、交易或明确决定","evidence_quote":"正文逐字引文"}}],
  "open_loops": [{{"text":"尚未解决且已经在正文出现的压力或问题","evidence_quote":"正文逐字引文"}}],
  "immediate_next_intent": {{"text":"章末角色已经准备执行的下一步","evidence_quote":"正文逐字引文"}},
  "plan_reconciliation": [{{"text":"计划完成、未完成或发生偏移的项目","evidence_quote":"正文逐字引文"}}]
}}

# 章节标题
{chapter_title}

# 原章节计划
{chapter_plan}

# 完整正式正文
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
        "来源：正式正文 artifact {} / SHA-256 {} / {}",
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

        assert!(prompt.contains("每条记录只表达一个原子事实"));
        assert!(prompt.contains("命令、计划、传闻必须标明其性质"));
        assert!(prompt.contains("物件记录必须区分"));
        assert!(prompt.contains("互相冲突的数量或状态"));
        assert!(prompt.contains("只删除有争议的字段"));
        assert!(prompt.contains("正文最后三分之一倒序检查"));
        assert!(prompt.contains("引文必须能够单独证明整条"));
        assert!(prompt.contains("遮住完整正文"));
        assert!(prompt.contains("夺取赤髓"));
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
}
