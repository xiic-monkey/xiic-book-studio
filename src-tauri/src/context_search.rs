use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ai,
    db::AppState,
    error::{AppError, AppResult},
    models::{
        AiSettings, ContinuityLedgerEntry, Stage, StoryContextSearchInput, StoryContextSnippet,
    },
    workflow,
};

const PLAN_STAGE: &str = "context_search_plan";
const MAX_SOURCE_CHARS: usize = 24_000;
const MAX_AGENT_SEARCHES: usize = 6;
const MAX_EXECUTED_SEARCHES: usize = 12;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct SearchRequest {
    query: String,
    reason: String,
    evidence_quote: String,
}

#[derive(Debug, Deserialize)]
struct SearchPlan {
    #[serde(default)]
    searches: Vec<SearchRequest>,
}

#[derive(Debug, Clone)]
struct LedgerHit {
    entry: ContinuityLedgerEntry,
    chapter_no: i64,
    chapter_title: String,
}

#[derive(Debug, Clone)]
struct StoryFactHit {
    entity_name: String,
    dimension: String,
    value: String,
    visibility: String,
    chapter_no: i64,
    chapter_title: String,
    source_quote: String,
}

/// Run a provider-neutral tool-planning turn, execute App-owned searches, and
/// render only verified results for the main writing/review/revision turn.
pub async fn prepare_tool_context(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
    stage: &Stage,
    source_material: &str,
    settings: &AiSettings,
    api_key: &str,
) -> AppResult<Option<String>> {
    if source_material.trim().is_empty() {
        return Ok(None);
    }

    // Keep the planner prompt bounded without throwing away the beginning of a
    // long chapter. Deterministic App safeguards below still inspect the full
    // source, so registered entities in the omitted middle remain searchable.
    let planner_source = sample_source(source_material, MAX_SOURCE_CHARS);
    let ledger_catalog = prior_ledger_catalog(state, project_id, chapter_id)?;
    let planner_prompt = build_planner_prompt(stage, &planner_source, &ledger_catalog);
    let started = Instant::now();
    let run = state.insert_workflow_run(
        project_id,
        Some(chapter_id),
        PLAN_STAGE,
        &planner_prompt,
        "",
        "running",
        None,
        0,
    )?;

    let (agent_searches, planner_error) = match ai::complete_json_chat(
        settings,
        api_key,
        "你是长篇小说 Agent 的上下文检索规划器。你只能提出搜索请求，不能续写、试读或修订正文。所有查询都必须由输入原文中的逐字证据触发。",
        &planner_prompt,
        0.0,
    )
    .await
    {
        Ok(raw) => match parse_search_plan(&raw, &planner_source) {
            Ok(searches) => (searches, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        },
        Err(error) => (Vec::new(), Some(error.to_string())),
    };

    let safeguards = fallback_searches(state, project_id, chapter_id, source_material)?;
    let planned = merge_searches(&agent_searches, &safeguards);
    let recorded_plan = serde_json::json!({
        "agent_searches": agent_searches,
        "app_safeguards": safeguards,
        "executed_searches": planned,
    });
    state.update_workflow_run(
        run.id,
        &serde_json::to_string(&recorded_plan)?,
        if planner_error.is_some() {
            "failed"
        } else {
            "success"
        },
        planner_error.as_deref(),
        started.elapsed().as_millis() as i64,
    )?;

    if planned.is_empty() {
        return Ok(None);
    }

    let snippets = execute_history_searches(state, project_id, chapter_id, &planned)?;
    let ledger_hits =
        matching_ledger_hits(state, project_id, chapter_id, source_material, &planned)?;
    let fact_hits =
        matching_story_fact_hits(state, project_id, chapter_id, source_material, &planned)?;
    if snippets.is_empty() && ledger_hits.is_empty() && fact_hits.is_empty() {
        return Ok(Some(render_no_hit_context(stage, &planned)));
    }

    Ok(Some(render_tool_context(
        stage,
        &planned,
        &snippets,
        &ledger_hits,
        &fact_hits,
    )))
}

fn build_planner_prompt(stage: &Stage, source_material: &str, ledger_catalog: &[String]) -> String {
    let stage_task = match stage {
        Stage::Draft => "从本章章纲和人工指令中，找出续写前必须核对历史状态的旧人物、旧物件、旧能力、旧地点、旧事件或旧承诺。",
        Stage::Review => "逐段检查待试读正文，找出其中声称再次出现、仍然持有、已经知道、可以使用、已经发生或延续至今的旧人物、旧物件、旧能力、旧地点、旧事件或旧承诺。",
        Stage::Revision => "从被修订正文、试读意见和人工反馈中，找出修订时必须回查的旧人物、旧物件、旧能力、旧地点、旧事件或旧承诺。",
        _ => "找出需要核对历史正文的既有故事实体或事件。",
    };
    let catalog = if ledger_catalog.is_empty() {
        "（当前没有可用的结构化状态账本目录）".to_string()
    } else {
        ledger_catalog
            .iter()
            .take(40)
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"# 任务
{stage_task}

你正在决定是否调用 App 的 `search_story_context` 工具。只为确实依赖前文状态的对象发起查询；当前章首次出现且不依赖历史的内容不要搜索。最多 {MAX_AGENT_SEARCHES} 次。试读正文超过 1500 字且多次使用前文人物、物件或事件时，通常需要 3-6 个不同查询，不能因为状态账本目录存在就省略正文搜索。

# 搜索规则
1. `query` 使用 2-24 个字的具体名称或短语，例如人物名、物件名、事件名、地点名、承诺对象；不要写“相关历史”“前文伏笔”之类泛词。
2. `evidence_quote` 必须是下方输入中连续出现的 6-100 个字原文，且能证明为什么需要回查。不得改写、拼接或使用省略号。
3. `reason` 说明要核对什么状态，例如角色知情边界、入场路径、物件归属/消耗/封印、伤势、承诺是否兑现、旧事件的实际结果。
4. 试读阶段宁可多查真正的旧引用，也不要只复述章纲。修订阶段必须以被修订正文为主要搜索来源。
5. 不得搜索未来剧情，也不得把候选稿自身当成历史事实。

# 已有状态账本实体目录
目录只用于提醒名称，不代表当前状态；状态仍由 App 查询结果提供。
{catalog}

# 当前输入
{source_material}

# 输出
只输出 JSON 对象：{{"searches":[{{"query":"...","reason":"...","evidence_quote":"..."}}]}}。没有需要回查的旧引用时返回 {{"searches":[]}}。"#,
    )
}

fn parse_search_plan(raw: &str, source_material: &str) -> AppResult<Vec<SearchRequest>> {
    let cleaned = raw
        .trim()
        .strip_prefix("```json")
        .or_else(|| raw.trim().strip_prefix("```"))
        .unwrap_or(raw.trim())
        .trim_end_matches("```")
        .trim();
    let value: Value = serde_json::from_str(cleaned)?;
    let candidates = if value.is_array() {
        serde_json::from_value::<Vec<SearchRequest>>(value)?
    } else {
        serde_json::from_value::<SearchPlan>(value)?.searches
    };

    let mut searches = Vec::new();
    let mut seen = HashSet::new();
    for mut request in candidates {
        request.query = request.query.trim().to_string();
        request.reason = request.reason.trim().to_string();
        request.evidence_quote = request.evidence_quote.trim().to_string();
        let query_chars = request.query.chars().count();
        let quote_chars = request.evidence_quote.chars().count();
        if !(2..=24).contains(&query_chars)
            || !(6..=100).contains(&quote_chars)
            || !source_material.contains(&request.evidence_quote)
            || !source_material.contains(&request.query)
            || request.reason.is_empty()
            || !seen.insert(request.query.clone())
        {
            continue;
        }
        searches.push(request);
        if searches.len() == MAX_AGENT_SEARCHES {
            break;
        }
    }
    Ok(searches)
}

fn fallback_searches(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
    source_material: &str,
) -> AppResult<Vec<SearchRequest>> {
    let labels = known_reference_labels(state, project_id, chapter_id)?;

    let mut searches = Vec::new();
    for label in labels {
        if label.chars().count() < 2 || !source_material.contains(&label) {
            continue;
        }
        let evidence_quote = excerpt_around_term(source_material, &label, 72);
        searches.push(SearchRequest {
            query: label,
            reason: "App 按候选稿中出现的已登记实体补充确定性回查".to_string(),
            evidence_quote,
        });
        if searches.len() == MAX_EXECUTED_SEARCHES {
            break;
        }
    }
    Ok(searches)
}

fn known_reference_labels(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<Vec<String>> {
    let mut weighted = prior_ledger_catalog(state, project_id, chapter_id)?
        .into_iter()
        .map(|label| (5_u8, label))
        .collect::<Vec<_>>();

    if let Some(characters) = state.approved_artifact(project_id, "characters", None)? {
        weighted.extend(markdown_entity_headings(&characters.content).map(|label| (4, label)));
    }
    weighted.extend(
        state
            .list_story_entities(project_id)?
            .into_iter()
            .map(|entity| (5, entity.name)),
    );
    weighted.extend(
        state
            .list_knowledge_cards(project_id)?
            .into_iter()
            .filter(|card| card.status == "approved")
            .map(|card| (4, card.title)),
    );
    weighted.extend(
        state
            .list_foreshadowings(project_id)?
            .into_iter()
            .filter(|item| item.status != "resolved")
            .map(|item| (4, item.title)),
    );
    weighted.extend(
        state
            .list_story_threads(project_id)?
            .into_iter()
            .filter(|thread| thread.label.chars().count() <= 12)
            .map(|thread| (1, thread.label)),
    );
    weighted.sort_by(|(priority_a, label_a), (priority_b, label_b)| {
        priority_b
            .cmp(priority_a)
            .then_with(|| label_b.chars().count().cmp(&label_a.chars().count()))
    });

    let mut labels = Vec::new();
    let mut seen = HashSet::new();
    for (_, label) in weighted {
        let label = label.trim().to_string();
        if (2..=24).contains(&label.chars().count()) && seen.insert(label.clone()) {
            labels.push(label);
        }
    }
    Ok(labels)
}

fn markdown_entity_headings(content: &str) -> impl Iterator<Item = String> + '_ {
    content.lines().filter_map(|line| {
        let heading = line.trim().strip_prefix("## ")?.trim();
        let heading = heading
            .split(['：', ':', '（', '('])
            .next()
            .unwrap_or_default()
            .trim();
        ((2..=16).contains(&heading.chars().count())
            && !["主要角色", "次要角色", "角色关系", "角色设计"].contains(&heading))
        .then(|| heading.to_string())
    })
}

fn merge_searches(agent: &[SearchRequest], safeguards: &[SearchRequest]) -> Vec<SearchRequest> {
    let mut searches = Vec::new();
    let mut seen = HashSet::new();
    for request in agent {
        if seen.insert(request.query.clone()) {
            searches.push(request.clone());
        }
        for safeguard in safeguards
            .iter()
            .filter(|item| request.query.contains(&item.query))
        {
            if seen.insert(safeguard.query.clone()) {
                searches.push(safeguard.clone());
            }
        }
    }
    for request in safeguards {
        if seen.insert(request.query.clone()) {
            searches.push(request.clone());
        }
        if searches.len() == MAX_EXECUTED_SEARCHES {
            break;
        }
    }
    searches.truncate(MAX_EXECUTED_SEARCHES);
    searches
}

fn execute_history_searches(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
    searches: &[SearchRequest],
) -> AppResult<Vec<(SearchRequest, StoryContextSnippet)>> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for search in searches {
        let snippets = workflow::search_story_context(
            state,
            StoryContextSearchInput {
                project_id,
                chapter_id: Some(chapter_id),
                query: search.query.clone(),
                limit: Some(4),
                include_immediate_previous: true,
            },
        )?;
        for snippet in snippets.into_iter().take(2) {
            let key = format!(
                "{}|{}|{}",
                snippet.source_label, snippet.matched_term, snippet.content
            );
            if seen.insert(key) {
                results.push((search.clone(), snippet));
            }
        }
    }
    results.truncate(12);
    Ok(results)
}

fn prior_ledger_catalog(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<Vec<String>> {
    let hits = prior_ledger_entries(state, project_id, chapter_id)?;
    let mut labels = hits
        .into_iter()
        .map(|hit| hit.entry.entity_label)
        .filter(|label| !label.trim().is_empty())
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    Ok(labels)
}

fn matching_ledger_hits(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
    source_material: &str,
    searches: &[SearchRequest],
) -> AppResult<Vec<LedgerHit>> {
    let query_text = searches
        .iter()
        .map(|search| search.query.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut latest = HashMap::<(String, String), LedgerHit>::new();
    for hit in prior_ledger_entries(state, project_id, chapter_id)? {
        let entry = &hit.entry;
        let relevant = source_material.contains(&entry.entity_label)
            || source_material.contains(&entry.entity_key)
            || query_text.contains(&entry.entity_label)
            || query_text.contains(&entry.entity_key);
        if !relevant {
            continue;
        }
        let key = (entry.entity_key.clone(), entry.state_kind.clone());
        let replace = latest
            .get(&key)
            .map(|current| (hit.chapter_no, hit.entry.id) > (current.chapter_no, current.entry.id))
            .unwrap_or(true);
        if replace {
            latest.insert(key, hit);
        }
    }
    let mut hits = latest.into_values().collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.chapter_no
            .cmp(&a.chapter_no)
            .then_with(|| a.entry.entity_label.cmp(&b.entry.entity_label))
    });
    hits.truncate(12);
    Ok(hits)
}

fn prior_ledger_entries(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<Vec<LedgerHit>> {
    let chapters = state.list_chapters(project_id)?;
    let current_no = chapters
        .iter()
        .find(|chapter| chapter.id == chapter_id)
        .map(|chapter| chapter.chapter_no)
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let chapter_lookup = chapters
        .into_iter()
        .map(|chapter| (chapter.id, (chapter.chapter_no, chapter.title)))
        .collect::<HashMap<_, _>>();

    Ok(state
        .list_continuity_ledger_entries(project_id)?
        .into_iter()
        .filter_map(|entry| {
            let (chapter_no, chapter_title) = chapter_lookup.get(&entry.chapter_id)?.clone();
            (chapter_no < current_no).then_some(LedgerHit {
                entry,
                chapter_no,
                chapter_title,
            })
        })
        .collect())
}

fn matching_story_fact_hits(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
    source_material: &str,
    searches: &[SearchRequest],
) -> AppResult<Vec<StoryFactHit>> {
    let chapters = state.list_chapters(project_id)?;
    let current_no = chapters
        .iter()
        .find(|chapter| chapter.id == chapter_id)
        .map(|chapter| chapter.chapter_no)
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let chapter_lookup = chapters
        .into_iter()
        .map(|chapter| (chapter.id, (chapter.chapter_no, chapter.title)))
        .collect::<HashMap<_, _>>();
    let query_text = searches
        .iter()
        .map(|search| search.query.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let entities = state.list_story_entities(project_id)?;
    let relevant_ids = entities
        .into_iter()
        .filter(|entity| {
            source_material.contains(&entity.name) || query_text.contains(&entity.name)
        })
        .map(|entity| (entity.id, entity.name))
        .collect::<HashMap<_, _>>();
    if relevant_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut latest = HashMap::<(i64, String), StoryFactHit>::new();
    for fact in state.list_story_facts(project_id)? {
        let Some(entity_name) = relevant_ids.get(&fact.entity_id) else {
            continue;
        };
        let Some((chapter_no, chapter_title)) =
            chapter_lookup.get(&fact.narrative_chapter_id.unwrap_or_default())
        else {
            continue;
        };
        if *chapter_no >= current_no || fact.status != "active" {
            continue;
        }
        let key = (fact.entity_id, fact.dimension.clone());
        let hit = StoryFactHit {
            entity_name: entity_name.clone(),
            dimension: fact.dimension,
            value: fact.value,
            visibility: fact.visibility,
            chapter_no: *chapter_no,
            chapter_title: chapter_title.clone(),
            source_quote: fact.source_quote,
        };
        let replace = latest
            .get(&key)
            .map(|existing| hit.chapter_no > existing.chapter_no)
            .unwrap_or(true);
        if replace {
            latest.insert(key, hit);
        }
    }
    let mut hits = latest.into_values().collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .chapter_no
            .cmp(&left.chapter_no)
            .then_with(|| left.entity_name.cmp(&right.entity_name))
            .then_with(|| left.dimension.cmp(&right.dimension))
    });
    hits.truncate(12);
    Ok(hits)
}

fn render_tool_context(
    stage: &Stage,
    searches: &[SearchRequest],
    snippets: &[(SearchRequest, StoryContextSnippet)],
    ledger_hits: &[LedgerHit],
    fact_hits: &[StoryFactHit],
) -> String {
    let mut output = String::from("# App 上下文工具执行结果");
    output.push_str("\n以下内容不是模型记忆或候选稿自证，而是 Agent 提出查询后，由 App 从本项目已通过的历史正文、书籍资料和状态账本中读取的结果。只能按引文使用，不得补全引文没有证明的事实。");
    output.push_str("\n\n## Agent 搜索请求");
    for search in searches {
        output.push_str(&format!(
            "\n- `{}`：{} | 触发原文：\"{}\"",
            search.query, search.reason, search.evidence_quote
        ));
    }
    if !snippets.is_empty() {
        output.push_str("\n\n## search_story_context 命中");
        for (search, snippet) in snippets {
            output.push_str(&format!(
                "\n- 查询 `{}` -> [{}] 命中 `{}`：{}",
                search.query, snippet.source_label, snippet.matched_term, snippet.content
            ));
        }
    }
    if !ledger_hits.is_empty() {
        output.push_str("\n\n## 状态账本末态");
        for hit in ledger_hits {
            output.push_str(&format!(
                "\n- {} / {} = {}（第 {} 章《{}》；原文：\"{}\"）",
                hit.entry.entity_label,
                hit.entry.state_kind,
                hit.entry.state_value,
                hit.chapter_no,
                hit.chapter_title,
                hit.entry.evidence_quote
            ));
        }
    }
    if !fact_hits.is_empty() {
        output.push_str("\n\n## 资料索引事实");
        for hit in fact_hits {
            output.push_str(&format!(
                "\n- {} / {} = {}（可见范围：{}；第 {} 章《{}》；原文：\"{}\"）",
                hit.entity_name,
                hit.dimension,
                hit.value,
                hit.visibility,
                hit.chapter_no,
                hit.chapter_title,
                hit.source_quote
            ));
        }
    }
    output.push_str(stage_instruction(stage));
    output
}

fn render_no_hit_context(stage: &Stage, searches: &[SearchRequest]) -> String {
    let mut output = String::from("# App 上下文工具执行结果");
    output.push_str("\nAgent 已调用 App 历史搜索，但以下查询没有在已通过的历史正文、书籍资料或状态账本中找到可引用结果：");
    for search in searches {
        output.push_str(&format!("\n- `{}`：{}", search.query, search.reason));
    }
    output.push_str(
        "\n未命中不等于候选稿正确，也不等于可以编造来源；只能把相关说法视为尚未得到历史证据支持。",
    );
    output.push_str(stage_instruction(stage));
    output
}

fn stage_instruction(stage: &Stage) -> &'static str {
    match stage {
        Stage::Draft => "\n\n写作要求：涉及命中对象时，沿用历史引文和账本末态；若本章要改变状态，正文必须写出导致变化的动作与代价。搜索结果与本章无关时不要硬塞。",
        Stage::Review => "\n\n试读要求：必须把候选稿中的相关说法与工具引文、账本末态逐项比较。人物知情边界、入场路径、旧事件结果、物件归属/消耗/封印、伤势或承诺不一致时，应作为连续性问题报告；未命中时不得自行假定前文存在。",
        Stage::Revision => "\n\n修订要求：先修正工具结果证明的连续性断点，再处理节奏和句子；不得用新增过去事件或新增设定解释冲突。",
        _ => "",
    }
}

fn sample_source(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }

    let head_chars = max_chars / 2;
    let tail_chars = max_chars - head_chars;
    let head = text.chars().take(head_chars).collect::<String>();
    let tail = text.chars().skip(count - tail_chars).collect::<String>();
    format!("{head}\n\n[中间正文已省略；请只引用首尾取样中连续出现的原文]\n\n{tail}")
}

fn excerpt_around_term(text: &str, term: &str, max_chars: usize) -> String {
    let Some(byte_index) = text.find(term) else {
        return term.to_string();
    };
    let chars = text.chars().collect::<Vec<_>>();
    let index = text[..byte_index].chars().count();
    let term_len = term.chars().count();
    let start = index.saturating_sub(max_chars / 2);
    let end = (index + term_len + max_chars / 2).min(chars.len());
    chars[start..end].iter().collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewChapter, NewProject};
    use tempfile::NamedTempFile;

    #[test]
    fn planner_queries_must_be_grounded_in_source() {
        let source = "沈砚重新取出裂纹玉牌，想用它开启炉门。";
        let raw = r#"{"searches":[
            {"query":"裂纹玉牌","reason":"核对归属和状态","evidence_quote":"沈砚重新取出裂纹玉牌，想用它开启炉门。"},
            {"query":"玄铁令","reason":"核对不存在的物件","evidence_quote":"沈砚取出玄铁令"}
        ]}"#;
        let plan = parse_search_plan(raw, source).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].query, "裂纹玉牌");
    }

    #[test]
    fn long_source_sampling_keeps_both_ends_for_planning() {
        let source = format!("开头旧人物{}结尾旧物件", "中间内容".repeat(100));
        let sampled = sample_source(&source, 24);

        assert!(sampled.contains("开头旧人物"));
        assert!(sampled.contains("结尾旧物件"));
        assert!(sampled.contains("中间正文已省略"));
        assert!(sampled.chars().count() > 24);
    }

    #[test]
    fn planner_accepts_top_level_array_for_provider_compatibility() {
        let source = "顾青知道三年前矿洞坍塌的真相。";
        let raw = r#"[{"query":"矿洞坍塌","reason":"核对旧事件结果和知情边界","evidence_quote":"顾青知道三年前矿洞坍塌的真相"}]"#;
        let plan = parse_search_plan(raw, source).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].query, "矿洞坍塌");
    }

    #[test]
    fn duplicate_and_overlong_queries_are_rejected() {
        let source = "他再次看见旧门。";
        let raw = r#"{"searches":[
            {"query":"旧门","reason":"核对状态","evidence_quote":"他再次看见旧门"},
            {"query":"旧门","reason":"重复","evidence_quote":"他再次看见旧门"},
            {"query":"这是一个超过二十四个汉字而且没有必要如此冗长的历史检索查询","reason":"过长","evidence_quote":"他再次看见旧门"}
        ]}"#;
        let plan = parse_search_plan(raw, source).unwrap();
        assert_eq!(plan.len(), 1);
    }

    #[test]
    fn app_expands_compound_agent_query_to_registered_entity() {
        let agent = vec![SearchRequest {
            query: "黑牌背面金纹".to_string(),
            reason: "核对状态".to_string(),
            evidence_quote: "黑牌背面金纹仍在发烫".to_string(),
        }];
        let safeguards = vec![SearchRequest {
            query: "黑牌".to_string(),
            reason: "App 补查".to_string(),
            evidence_quote: "黑牌背面金纹仍在发烫".to_string(),
        }];

        let merged = merge_searches(&agent, &safeguards);
        assert_eq!(merged[0].query, "黑牌背面金纹");
        assert_eq!(merged[1].query, "黑牌");
    }

    #[test]
    fn app_search_reads_only_approved_past_chapters() {
        let temp = NamedTempFile::new().unwrap();
        let state = AppState::from_path(temp.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "历史检索边界".to_string(),
                genre: "奇幻".to_string(),
                target_words: 100_000,
                premise: "验证检索边界".to_string(),
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

        let past = state
            .insert_artifact(
                project.id,
                Some(chapter_one.id),
                "draft",
                "第一章正文",
                "沈砚把裂纹玉牌封入石匣，亲眼看着匣盖合拢。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", past.id, "测试通过")
            .unwrap();
        let pending_past = state
            .insert_artifact(
                project.id,
                Some(chapter_one.id),
                "revision",
                "未通过改稿",
                "裂纹玉牌其实已经被陌生人取走。",
                Some(past.id),
            )
            .unwrap();
        let future = state
            .insert_artifact(
                project.id,
                Some(chapter_three.id),
                "draft",
                "第三章正文",
                "未来章节说裂纹玉牌已经彻底粉碎。",
                None,
            )
            .unwrap();
        state
            .approve_stage(project.id, "draft", future.id, "测试通过")
            .unwrap();

        let searches = vec![SearchRequest {
            query: "裂纹玉牌".to_string(),
            reason: "核对物件末态".to_string(),
            evidence_quote: "他再次取出裂纹玉牌准备开门".to_string(),
        }];
        let results =
            execute_history_searches(&state, project.id, chapter_two.id, &searches).unwrap();

        assert!(results
            .iter()
            .any(|(_, hit)| hit.content.contains("封入石匣")));
        assert!(!results
            .iter()
            .any(|(_, hit)| hit.content.contains("陌生人取走")));
        assert!(!results
            .iter()
            .any(|(_, hit)| hit.content.contains("彻底粉碎")));
        assert_eq!(
            state.get_artifact(pending_past.id).unwrap().status,
            "pending_human_approval"
        );
    }
}
