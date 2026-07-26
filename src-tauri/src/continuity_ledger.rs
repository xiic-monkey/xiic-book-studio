use std::{collections::HashMap, time::Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    ai, chapter_memory,
    db::AppState,
    error::{AppError, AppResult},
    models::{
        Artifact, ContinuityLedgerEntry, LedgerContinuityCheckRequest, LedgerContinuityIssue,
        LedgerContinuityReport,
    },
};

pub const NORMALIZATION_VERSION: &str = "continuity-ledger-v1";
const BUILD_STAGE: &str = "continuity_ledger_build";
const CHECK_STAGE: &str = "continuity_ledger_check";
const CHECK_VERSION: &str = "v4";

#[derive(Debug, Deserialize)]
struct ExtractedLedgerEntry {
    entity_kind: String,
    entity_key: String,
    entity_label: String,
    state_kind: String,
    state_value: String,
    evidence_quote: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CandidateClaim {
    entity_key: String,
    #[serde(default)]
    entity_label: String,
    state_kind: String,
    required_state: String,
    claim_mode: String,
    evidence_quote: String,
}

pub async fn check_artifact_continuity(
    state: &AppState,
    input: LedgerContinuityCheckRequest,
) -> AppResult<LedgerContinuityReport> {
    let artifact = state.get_artifact(input.artifact_id)?;
    validate_candidate(&artifact, input.project_id)?;
    let chapter_id = artifact
        .chapter_id
        .ok_or_else(|| AppError::Validation("状态账本只支持章节草稿或修订稿".to_string()))?;
    let chapters = state.list_chapters(input.project_id)?;
    let candidate_chapter = chapters
        .iter()
        .find(|chapter| chapter.id == chapter_id)
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;

    ensure_ledger_current(state, input.project_id).await?;
    let entries = state
        .list_continuity_ledger_entries(input.project_id)?
        .into_iter()
        .filter(|entry| {
            chapters
                .iter()
                .find(|chapter| chapter.id == entry.chapter_id)
                .map(|chapter| chapter.chapter_no < candidate_chapter.chapter_no)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Ok(empty_report(input.project_id, artifact.id));
    }

    let ledger_hash = hash_ledger(&entries);
    let candidate_hash = chapter_memory::source_text_hash(&artifact.content);
    let cache_marker = format!(
        "# continuity-ledger-check={CHECK_VERSION}|artifact={}|candidate_hash={}|ledger_hash={}",
        artifact.id, candidate_hash, ledger_hash,
    );
    if let Some(report) = cached_report(state, input.project_id, &cache_marker)? {
        return Ok(report);
    }

    let active_states = latest_states(&entries);
    let raw = extract_candidate_claims(state, &artifact, &active_states).await?;
    let model_claims = parse_candidate_claims(&raw, &artifact.content)?;
    // The model narrows natural-language references, while this intentionally small
    // exact-match pass protects the hard cases it can still overlook (for example,
    // a sealed named furnace being directly pushed open in a later draft).
    let claims = merge_candidate_claims(
        model_claims,
        explicit_exact_match_claims(&artifact.content, &active_states),
    );
    let issues = compare_claims(&claims, &active_states, &chapters);
    let report = LedgerContinuityReport {
        project_id: input.project_id,
        artifact_id: artifact.id,
        summary: if issues.is_empty() {
            "未发现候选稿对已通过物品、资源、禁制或伤势状态的直接冲突。此核对只覆盖已结构化的明确状态，不替代试读。".to_string()
        } else {
            format!(
                "发现 {} 条与已通过正文状态直接冲突的引用。请先核对两侧原文；人工仍可决定是否修订或通过。",
                issues.len()
            )
        },
        issues,
    };
    state.insert_workflow_run(
        input.project_id,
        artifact.chapter_id,
        CHECK_STAGE,
        &cache_marker,
        &serde_json::to_string(&report)?,
        "success",
        None,
        0,
    )?;
    Ok(report)
}

pub async fn ensure_ledger_current(state: &AppState, project_id: i64) -> AppResult<()> {
    let settings = deterministic_settings(state.get_ai_settings()?);
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先为当前供应商保存 AI API Key".to_string()))?;
    for chapter in state.list_chapters(project_id)? {
        let Some(source) = state.latest_approved_chapter_body(project_id, chapter.id)? else {
            continue;
        };
        let source_hash = chapter_memory::source_text_hash(&source.content);
        if state.continuity_ledger_source_is_current(
            project_id,
            chapter.id,
            source.id,
            &source_hash,
            NORMALIZATION_VERSION,
        )? {
            continue;
        }

        let prompt = extraction_prompt(&chapter.title, &source.content);
        let started = Instant::now();
        let run = state.insert_workflow_run(
            project_id,
            Some(chapter.id),
            BUILD_STAGE,
            &prompt,
            "",
            "running",
            None,
            0,
        )?;
        let raw = match ai::complete_chat(
            &settings,
            &api_key,
            "你是小说连续性状态记录员。只从已通过正文中提取可直接引用的状态变化，绝不补写或推断。",
            &prompt,
            0.0,
        )
        .await
        {
            Ok(output) => output,
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
        let entries = match parse_source_entries(
            &raw,
            project_id,
            chapter.id,
            source.id,
            &source_hash,
            &source.content,
        ) {
            Ok(entries) => entries,
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
        state.replace_continuity_ledger_chapter_cas(
            project_id,
            chapter.id,
            source.id,
            &source_hash,
            NORMALIZATION_VERSION,
            &entries,
        )?;
        state.update_workflow_run(
            run.id,
            &serde_json::to_string(&entries)?,
            "success",
            None,
            started.elapsed().as_millis() as i64,
        )?;
    }
    Ok(())
}

pub fn render_report_for_prompt(report: &LedgerContinuityReport) -> String {
    if report.issues.is_empty() {
        return String::new();
    }
    let mut text = String::from(
        "# 状态账本核对\n以下问题来自已通过正文的带引文状态记录。它们优先于润色，但不能靠编造新设定解释；请删除冲突引用，或补出候选稿中已有的获得、修复、解封、疗伤等明确动作。",
    );
    for issue in report.issues.iter().take(6) {
        text.push_str(&format!(
            "\n- {}（{}）：候选稿“{}”；{}“{}”。{}",
            issue.entity_label,
            issue.severity,
            issue.candidate_quote,
            issue.source_chapter,
            issue.source_quote,
            issue.suggestion
        ));
    }
    text
}

fn validate_candidate(artifact: &Artifact, project_id: i64) -> AppResult<()> {
    if artifact.project_id != project_id
        || artifact.chapter_id.is_none()
        || (artifact.stage != "draft" && artifact.stage != "revision")
    {
        return Err(AppError::Validation(
            "状态账本核对只支持当前项目的章节草稿或修订稿".to_string(),
        ));
    }
    Ok(())
}

fn extraction_prompt(title: &str, content: &str) -> String {
    format!(
        r#"# 任务
从以下已通过正文提取可供后续章节核对的明确状态变化。账本是派生索引，不是新设定；没有可靠状态就输出 []。

# 只允许记录
1. 具名物品或资源：持有、获得、消耗、遗失、耗尽。
2. 具名门、阵法、禁制、令牌、药物等：开启、封闭、激活、失效、受损、可用。
3. 具名伤势或异常状态：存在、痊愈、解除、持续。

# 规则
1. 实体必须是正文中明确出现的具名对象；不要使用“令牌”“丹药”“伤势”“此物”这类泛称。
2. entity_key 只能是实体名称的标准化写法：去掉空白与标点，不加别名、解释或编号推断。
3. evidence_quote 必须是正文连续出现的 8-100 个字符原文。
4. 不记录角色猜测、隐情、未来计划、抽象规则或未确认传闻。
5. 每个实体只写本章真正改变或明确确认的状态；不确定就省略。
6. 只输出 JSON 数组，不要 Markdown。

# JSON
{{
  "entity_kind": "item" | "resource" | "barrier" | "condition",
  "entity_key": "标准化实体名",
  "entity_label": "正文中的实体名",
  "state_kind": "possession" | "availability" | "condition",
  "state_value": "held" | "consumed" | "lost" | "depleted" | "usable" | "sealed" | "active" | "inactive" | "damaged" | "present" | "recovered" | "cleared" | "ongoing",
  "evidence_quote": "正文逐字引文"
}}

# 章节
{title}

# 已通过正文
{content}"#
    )
}

async fn extract_candidate_claims(
    state: &AppState,
    artifact: &Artifact,
    active_states: &HashMap<(String, String), ContinuityLedgerEntry>,
) -> AppResult<String> {
    let settings = deterministic_settings(state.get_ai_settings()?);
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先为当前供应商保存 AI API Key".to_string()))?;
    let states = active_states
        .values()
        .map(|entry| {
            format!(
                "- entity_key={} | 名称={} | 类别={} | 状态类别={} | 最后状态={} | 来源引文={}",
                entry.entity_key,
                entry.entity_label,
                entry.entity_kind,
                entry.state_kind,
                entry.state_value,
                entry.evidence_quote
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        r#"# 任务
检查候选稿是否把下列已通过状态中的对象当作仍可持有、可用或已无伤。只提取“直接引用旧对象但没有写出获得、修复、解封、疗伤等状态变化动作”的句子。

# 规则
1. entity_key 必须逐字使用状态清单中的 entity_key；没有精确命中的名称不要输出。
2. claim_mode 只能是 reference 或 transition。正文明确写出获得、修复、解封、疗伤等动作时填 transition，且不作为冲突候选。
3. required_state：possession 只用 held；availability 只用 usable；condition 只用 recovered。
4. evidence_quote 必须是候选稿连续出现的 8-100 个字符原文。
5. 只输出 JSON 数组，不要 Markdown。

# 已通过状态清单
{states}

# 候选稿
{}"#,
        artifact.content
    );
    ai::complete_chat(
        &settings,
        &api_key,
        "你是严格的小说状态核对器。只识别直接的、可由原文引证的状态引用，不推断。",
        &prompt,
        0.0,
    )
    .await
}

fn parse_source_entries(
    raw: &str,
    project_id: i64,
    chapter_id: i64,
    source_artifact_id: i64,
    source_text_hash: &str,
    source_text: &str,
) -> AppResult<Vec<ContinuityLedgerEntry>> {
    let parsed = parse_json_array::<ExtractedLedgerEntry>(raw, "连续性账本没有返回 JSON 数组")?;
    let mut seen = HashMap::new();
    for item in parsed {
        let entity_key = normalize_entity_key(&item.entity_key);
        if !valid_source_entry(&item, &entity_key, source_text) {
            continue;
        }
        let key = (entity_key.clone(), item.state_kind.clone());
        seen.insert(
            key,
            ContinuityLedgerEntry {
                id: 0,
                project_id,
                chapter_id,
                source_artifact_id,
                source_text_hash: source_text_hash.to_string(),
                normalization_version: NORMALIZATION_VERSION.to_string(),
                entity_kind: item.entity_kind,
                entity_key,
                entity_label: item.entity_label.trim().to_string(),
                state_kind: item.state_kind,
                state_value: item.state_value,
                evidence_quote: item.evidence_quote.trim().to_string(),
                created_at: String::new(),
            },
        );
    }
    Ok(seen.into_values().collect())
}

fn parse_candidate_claims(raw: &str, candidate_text: &str) -> AppResult<Vec<CandidateClaim>> {
    let claims = parse_json_array::<CandidateClaim>(raw, "状态账本核对没有返回 JSON 数组")?;
    Ok(claims
        .into_iter()
        .filter(|claim| {
            matches!(claim.claim_mode.as_str(), "reference" | "transition")
                && matches!(
                    claim.state_kind.as_str(),
                    "possession" | "availability" | "condition"
                )
                && matches!(
                    claim.required_state.as_str(),
                    "held" | "usable" | "recovered"
                )
                && valid_quote(&claim.evidence_quote, candidate_text)
                && normalize_entity_key(&claim.entity_key).chars().count() >= 2
                && normalize_entity_key(&claim.evidence_quote)
                    .contains(&normalize_entity_key(&claim.entity_key))
                && claim_has_explicit_required_state(claim)
        })
        .collect())
}

fn claim_has_explicit_required_state(claim: &CandidateClaim) -> bool {
    if claim.claim_mode == "transition" {
        return has_explicit_state_transition(&claim.evidence_quote);
    }
    action_targets_entity(
        &claim.evidence_quote,
        &claim.entity_key,
        required_state_actions(&claim.required_state),
    )
}

fn merge_candidate_claims(
    mut model_claims: Vec<CandidateClaim>,
    exact_claims: Vec<CandidateClaim>,
) -> Vec<CandidateClaim> {
    for claim in exact_claims {
        let duplicate = model_claims.iter().any(|existing| {
            normalize_entity_key(&existing.entity_key) == normalize_entity_key(&claim.entity_key)
                && existing.state_kind == claim.state_kind
                && existing.required_state == claim.required_state
                && existing.evidence_quote == claim.evidence_quote
        });
        if !duplicate {
            model_claims.push(claim);
        }
    }
    model_claims
}

fn explicit_exact_match_claims(
    candidate_text: &str,
    active_states: &HashMap<(String, String), ContinuityLedgerEntry>,
) -> Vec<CandidateClaim> {
    let mut claims = Vec::new();
    for sentence in candidate_sentences(candidate_text) {
        if sentence.chars().count() > 100 || has_explicit_state_transition(sentence) {
            continue;
        }
        for entry in active_states.values() {
            if !sentence_references_entity(sentence, entry) {
                continue;
            }
            let Some((required_state, actions)) = exact_action_rule(entry) else {
                continue;
            };
            if !action_targets_entity(sentence, &entry.entity_key, actions) {
                continue;
            }
            claims.push(CandidateClaim {
                entity_key: entry.entity_key.clone(),
                entity_label: entry.entity_label.clone(),
                state_kind: entry.state_kind.clone(),
                required_state: required_state.to_string(),
                claim_mode: "reference".to_string(),
                evidence_quote: sentence.trim().to_string(),
            });
        }
    }
    claims
}

fn candidate_sentences(text: &str) -> Vec<&str> {
    text.split_inclusive(['。', '！', '？', '.', '!', '?'])
        .map(str::trim)
        .filter(|sentence| sentence.chars().count() >= 8)
        .collect()
}

fn sentence_references_entity(sentence: &str, entry: &ContinuityLedgerEntry) -> bool {
    let normalized_sentence = normalize_entity_key(sentence);
    normalized_sentence.contains(&entry.entity_key)
        || normalized_sentence.contains(&normalize_entity_key(&entry.entity_label))
}

fn exact_action_rule(
    entry: &ContinuityLedgerEntry,
) -> Option<(&'static str, &'static [&'static str])> {
    match (entry.state_kind.as_str(), entry.state_value.as_str()) {
        ("possession", "consumed" | "lost" | "depleted") => {
            Some(("held", required_state_actions("held")))
        }
        ("availability", "sealed" | "inactive" | "damaged" | "depleted") => {
            Some(("usable", required_state_actions("usable")))
        }
        ("condition", "present" | "ongoing") => {
            Some(("recovered", required_state_actions("recovered")))
        }
        _ => None,
    }
}

fn required_state_actions(required_state: &str) -> &'static [&'static str] {
    match required_state {
        "held" => &[
            "取出", "拿出", "摸出", "掏出", "握住", "攥住", "塞进", "揣进", "服下", "吞下", "吞服",
            "饮下", "喝下", "祭出", "佩戴", "戴上", "插入", "装入",
        ],
        "usable" => &[
            "打开", "推开", "开启", "跨入", "进入", "穿过", "启动", "激活", "催动", "使用",
        ],
        "recovered" => &[
            "痊愈",
            "恢复如初",
            "完好无损",
            "毫发无伤",
            "伤势尽复",
            "再无大碍",
            "已经无碍",
            "已无大碍",
        ],
        _ => &[],
    }
}

fn action_targets_entity(text: &str, entity_key: &str, actions: &[&str]) -> bool {
    let text = normalize_entity_key(text);
    let entity = normalize_entity_key(entity_key);
    if entity.is_empty() {
        return false;
    }
    for (entity_start, _) in text.match_indices(&entity) {
        let entity_end = entity_start + entity.len();
        for action in actions {
            let action = normalize_entity_key(action);
            for (action_start, _) in text.match_indices(&action) {
                let action_end = action_start + action.len();
                let gap = if action_end <= entity_start {
                    text[action_end..entity_start].chars().count()
                } else if entity_end <= action_start {
                    text[entity_end..action_start].chars().count()
                } else {
                    0
                };
                if gap <= 4 {
                    return true;
                }
            }
        }
    }
    false
}

fn has_explicit_state_transition(sentence: &str) -> bool {
    contains_any(
        sentence,
        &[
            "获得",
            "得到了",
            "捡起",
            "找回",
            "修复",
            "解封",
            "疗伤",
            "治愈",
            "炼成",
            "换得",
            "补回",
        ],
    )
}

fn contains_any(text: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn compare_claims(
    claims: &[CandidateClaim],
    active_states: &HashMap<(String, String), ContinuityLedgerEntry>,
    chapters: &[crate::models::Chapter],
) -> Vec<LedgerContinuityIssue> {
    let chapter_labels = chapters
        .iter()
        .map(|chapter| {
            (
                chapter.id,
                format!("第 {} 章《{}》", chapter.chapter_no, chapter.title),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut issues = Vec::new();
    for claim in claims {
        if claim.claim_mode == "transition" {
            continue;
        }
        let key = (
            normalize_entity_key(&claim.entity_key),
            claim.state_kind.clone(),
        );
        let Some(previous) = active_states.get(&key) else {
            continue;
        };
        if !states_conflict(&claim.required_state, &previous.state_value) {
            continue;
        }
        issues.push(LedgerContinuityIssue {
            severity: "major".to_string(),
            entity_label: if claim.entity_label.trim().is_empty() {
                previous.entity_label.clone()
            } else {
                claim.entity_label.trim().to_string()
            },
            entity_kind: previous.entity_kind.clone(),
            state_kind: previous.state_kind.clone(),
            candidate_quote: claim.evidence_quote.trim().to_string(),
            source_chapter: chapter_labels
                .get(&previous.chapter_id)
                .cloned()
                .unwrap_or_else(|| "已通过正文".to_string()),
            source_quote: previous.evidence_quote.clone(),
            reason: format!(
                "候选稿把它当作“{}”，但最后已通过状态是“{}”。",
                required_label(&claim.required_state),
                state_label(&previous.state_value)
            ),
            suggestion: "删除这处直接引用，或在候选稿中写清已发生且有代价的获得、修复、解封或疗伤动作；不要补造旧设定。".to_string(),
        });
    }
    issues.sort_by(|a, b| a.entity_label.cmp(&b.entity_label));
    issues.dedup_by(|a, b| {
        a.entity_label == b.entity_label && a.candidate_quote == b.candidate_quote
    });
    issues
}

fn latest_states(
    entries: &[ContinuityLedgerEntry],
) -> HashMap<(String, String), ContinuityLedgerEntry> {
    let mut states = HashMap::new();
    for entry in entries {
        states.insert(
            (entry.entity_key.clone(), entry.state_kind.clone()),
            entry.clone(),
        );
    }
    states
}

fn states_conflict(required: &str, previous: &str) -> bool {
    match required {
        "held" => matches!(previous, "consumed" | "lost" | "depleted"),
        "usable" => matches!(previous, "sealed" | "inactive" | "damaged" | "depleted"),
        "recovered" => matches!(previous, "present" | "ongoing"),
        _ => false,
    }
}

fn valid_source_entry(item: &ExtractedLedgerEntry, entity_key: &str, source_text: &str) -> bool {
    matches!(
        item.entity_kind.as_str(),
        "item" | "resource" | "barrier" | "condition"
    ) && matches!(
        item.state_kind.as_str(),
        "possession" | "availability" | "condition"
    ) && matches!(
        item.state_value.as_str(),
        "held"
            | "consumed"
            | "lost"
            | "depleted"
            | "usable"
            | "sealed"
            | "active"
            | "inactive"
            | "damaged"
            | "present"
            | "recovered"
            | "cleared"
            | "ongoing"
    ) && entity_key.chars().count() >= 2
        && item.entity_label.trim().chars().count() >= 2
        && valid_quote(&item.evidence_quote, source_text)
}

fn valid_quote(quote: &str, source_text: &str) -> bool {
    let quote = normalize_text(quote);
    let length = quote.chars().count();
    (8..=100).contains(&length) && normalize_text(source_text).contains(&quote)
}

fn normalize_entity_key(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|character| !character.is_whitespace())
        .filter(|character| !"，。！？：；、“”‘’\"'（）()【】[]《》<>".contains(*character))
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_text(text: &str) -> String {
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

fn parse_json_array<T: for<'de> Deserialize<'de>>(raw: &str, error: &str) -> AppResult<Vec<T>> {
    let start = raw
        .find('[')
        .ok_or_else(|| AppError::Validation(error.to_string()))?;
    let end = raw
        .rfind(']')
        .filter(|end| *end >= start)
        .ok_or_else(|| AppError::Validation(error.to_string()))?;
    serde_json::from_str(&raw[start..=end]).map_err(AppError::from)
}

fn hash_ledger(entries: &[ContinuityLedgerEntry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.entity_key.as_bytes());
        hasher.update(entry.state_kind.as_bytes());
        hasher.update(entry.state_value.as_bytes());
        hasher.update(entry.source_text_hash.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn cached_report(
    state: &AppState,
    project_id: i64,
    marker: &str,
) -> AppResult<Option<LedgerContinuityReport>> {
    let report = state
        .list_workflow_runs(project_id)?
        .into_iter()
        .find(|run| {
            run.stage == CHECK_STAGE && run.status == "success" && run.input.starts_with(marker)
        })
        .and_then(|run| serde_json::from_str::<LedgerContinuityReport>(&run.output).ok());
    Ok(report)
}

fn empty_report(project_id: i64, artifact_id: i64) -> LedgerContinuityReport {
    LedgerContinuityReport {
        project_id,
        artifact_id,
        summary:
            "暂无可由已通过正文构成的状态账本；至少需要一章已通过正文且其中存在可验证的具名状态。"
                .to_string(),
        issues: Vec::new(),
    }
}

fn deterministic_settings(mut settings: crate::models::AiSettings) -> crate::models::AiSettings {
    // State extraction is a machine-readable indexing step. Provider thinking adds
    // latency and can wrap the JSON without improving the evidence-bound result.
    settings.thinking_enabled = false;
    settings
}

fn required_label(value: &str) -> String {
    match value {
        "held" => "仍在持有".to_string(),
        "usable" => "可以直接使用".to_string(),
        "recovered" => "已经无伤/解除异常".to_string(),
        _ => value.to_string(),
    }
}

fn state_label(value: &str) -> String {
    match value {
        "consumed" => "已消耗".to_string(),
        "lost" => "已遗失".to_string(),
        "depleted" => "已耗尽".to_string(),
        "sealed" => "已封闭".to_string(),
        "inactive" => "已失效".to_string(),
        "damaged" => "已受损".to_string(),
        "present" => "伤势/异常仍在".to_string(),
        "ongoing" => "伤势/异常持续".to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(state_kind: &str, state_value: &str) -> ContinuityLedgerEntry {
        ContinuityLedgerEntry {
            id: 1,
            project_id: 1,
            chapter_id: 1,
            source_artifact_id: 1,
            source_text_hash: "hash".to_string(),
            normalization_version: NORMALIZATION_VERSION.to_string(),
            entity_kind: "item".to_string(),
            entity_key: "黑牌".to_string(),
            entity_label: "黑牌".to_string(),
            state_kind: state_kind.to_string(),
            state_value: state_value.to_string(),
            evidence_quote: "陆烬将黑牌投入火中，黑牌已经耗尽灵光。".to_string(),
            created_at: String::new(),
        }
    }

    #[test]
    fn consumed_item_conflicts_with_later_direct_use() {
        let mut states = HashMap::new();
        states.insert(
            ("黑牌".to_string(), "possession".to_string()),
            entry("possession", "consumed"),
        );
        let claim = CandidateClaim {
            entity_key: "黑牌".to_string(),
            entity_label: "黑牌".to_string(),
            state_kind: "possession".to_string(),
            required_state: "held".to_string(),
            claim_mode: "reference".to_string(),
            evidence_quote: "陆烬从袖中取出黑牌，按在石门上。".to_string(),
        };
        let chapters = vec![crate::models::Chapter {
            id: 1,
            project_id: 1,
            chapter_no: 1,
            title: "旧章".to_string(),
            status: "approved".to_string(),
            current_artifact_id: Some(1),
            created_at: String::new(),
            updated_at: String::new(),
        }];
        assert_eq!(compare_claims(&[claim], &states, &chapters).len(), 1);
    }

    #[test]
    fn explicit_transition_is_not_a_conflict() {
        let mut states = HashMap::new();
        states.insert(
            ("黑牌".to_string(), "availability".to_string()),
            entry("availability", "damaged"),
        );
        let claim = CandidateClaim {
            entity_key: "黑牌".to_string(),
            entity_label: "黑牌".to_string(),
            state_kind: "availability".to_string(),
            required_state: "usable".to_string(),
            claim_mode: "transition".to_string(),
            evidence_quote: "他以精血修复黑牌裂纹，才将它贴上石门。".to_string(),
        };
        assert!(compare_claims(&[claim], &states, &[]).is_empty());
    }

    #[test]
    fn exact_named_barrier_action_catches_a_model_extraction_miss() {
        let mut states = HashMap::new();
        let mut furnace = entry("availability", "sealed");
        furnace.entity_kind = "barrier".to_string();
        furnace.entity_key = "七号旧炉".to_string();
        furnace.entity_label = "七号旧炉".to_string();
        furnace.evidence_quote = "封炉箱已经钉进七号旧炉门口，朱砂镇火纹仍在发亮。".to_string();
        states.insert(
            ("七号旧炉".to_string(), "availability".to_string()),
            furnace,
        );

        let claims =
            explicit_exact_match_claims("陆烬抬手推开七号旧炉的炉门，径直跨入炉内。", &states);

        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].required_state, "usable");
        assert_eq!(compare_claims(&claims, &states, &[]).len(), 1);
    }

    #[test]
    fn exact_match_pass_does_not_flag_an_explicit_unsealing() {
        let mut states = HashMap::new();
        let mut furnace = entry("availability", "sealed");
        furnace.entity_kind = "barrier".to_string();
        furnace.entity_key = "七号旧炉".to_string();
        furnace.entity_label = "七号旧炉".to_string();
        states.insert(
            ("七号旧炉".to_string(), "availability".to_string()),
            furnace,
        );

        let claims =
            explicit_exact_match_claims("陆烬以精血解封七号旧炉，才推开炉门踏入其中。", &states);

        assert!(claims.is_empty());
    }

    #[test]
    fn consumed_resource_effect_is_not_treated_as_current_possession() {
        let candidate = "赤髓胚修的那层新肉正在往里缩，火脉裂口比刚才宽了一丝。";
        let raw = format!(
            r#"[{{"entity_key":"赤髓胚","entity_label":"赤髓胚","state_kind":"possession","required_state":"held","claim_mode":"reference","evidence_quote":"{candidate}"}}]"#
        );

        assert!(parse_candidate_claims(&raw, candidate).unwrap().is_empty());

        let mut states = HashMap::new();
        let mut resource = entry("possession", "consumed");
        resource.entity_kind = "resource".to_string();
        resource.entity_key = "赤髓胚".to_string();
        resource.entity_label = "赤髓胚".to_string();
        states.insert(("赤髓胚".to_string(), "possession".to_string()), resource);
        assert!(explicit_exact_match_claims(candidate, &states).is_empty());
    }

    #[test]
    fn source_entries_require_a_verifiable_quote() {
        let raw = r#"[{"entity_kind":"item","entity_key":"黑牌","entity_label":"黑牌","state_kind":"availability","state_value":"depleted","evidence_quote":"不存在的引文内容"}]"#;
        assert!(parse_source_entries(raw, 1, 1, 1, "hash", "陆烬收起黑牌。")
            .unwrap()
            .is_empty());
    }
}
