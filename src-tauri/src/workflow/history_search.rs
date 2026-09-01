use std::collections::HashSet;

use crate::{
    db::AppState,
    error::AppResult,
    models::{StoryContextSearchInput, StoryContextSnippet, StoryFactSearchResult},
};

use super::{
    approved_outline_section_for_chapter, export_role_label, stage_label,
    text::{excerpt_around, is_noise_term, score_term, split_query_tokens},
};

pub(super) fn build_history_query(
    state: &AppState,
    project_id: i64,
    chapter_no: i64,
    user_instruction: Option<&str>,
) -> AppResult<Option<String>> {
    let mut parts = Vec::new();
    if let Some(section) = approved_outline_section_for_chapter(state, project_id, chapter_no)? {
        parts.push(section);
    }
    if let Some(instruction) = user_instruction {
        if !instruction.trim().is_empty() {
            parts.push(instruction.trim().to_string());
        }
    }
    let messages = state.list_messages(project_id)?;
    for message in messages.iter().take(8) {
        if message.role == "human_instruction" || message.role == "revision_feedback" {
            parts.push(message.content.clone());
        }
    }

    let joined = parts.join("\n");
    if joined.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(joined))
    }
}

pub fn search_story(
    state: &AppState,
    input: StoryContextSearchInput,
) -> AppResult<Vec<StoryContextSnippet>> {
    search_story_context(state, input)
}

pub fn search_story_context(
    state: &AppState,
    input: StoryContextSearchInput,
) -> AppResult<Vec<StoryContextSnippet>> {
    state.get_project(input.project_id)?;
    let query = input.query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let snippets = retrieve_history_snippets(
        state,
        input.project_id,
        input.chapter_id.unwrap_or_default(),
        query,
        input.include_immediate_previous,
        true,
    )?;
    let limit = input.limit.unwrap_or(6).clamp(1, 12);
    Ok(snippets.into_iter().take(limit).collect())
}

/// Search only structured, evidence-backed story state. This intentionally does
/// not fall back to prose or message search, so callers can distinguish facts
/// from narrative context.
pub fn search_story_facts(
    state: &AppState,
    input: StoryContextSearchInput,
) -> AppResult<Vec<StoryFactSearchResult>> {
    state.get_project(input.project_id)?;
    let query = input.query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let terms = split_query_tokens(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let limit = input.limit.unwrap_or(8).clamp(1, 12);
    let entities = state
        .list_story_entities(input.project_id)?
        .into_iter()
        .map(|entity| (entity.id, entity.name))
        .collect::<std::collections::HashMap<_, _>>();
    let chapters = state
        .list_chapters(input.project_id)?
        .into_iter()
        .map(|chapter| (chapter.id, (chapter.chapter_no, chapter.title)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut results = Vec::new();

    for fact in state.list_story_facts(input.project_id)? {
        if fact.status != "active" {
            continue;
        }
        let entity_label = entities.get(&fact.entity_id).cloned();
        let haystack = format!(
            "{} {} {} {}",
            entity_label.as_deref().unwrap_or(""),
            fact.dimension,
            fact.value,
            fact.source_quote
        );
        if let Some(score) = fact_match_score(&haystack, &terms) {
            let source_label = fact
                .narrative_chapter_id
                .and_then(|id| {
                    chapters
                        .get(&id)
                        .map(|(_, title)| format!("第{}章 {}", chapters[&id].0, title))
                })
                .unwrap_or_else(|| "结构化故事事实".to_string());
            results.push(StoryFactSearchResult {
                fact_type: "story_fact".to_string(),
                source_label,
                chapter_id: fact.narrative_chapter_id,
                entity_label,
                dimension: fact.dimension,
                value: fact.value,
                status: fact.status,
                evidence_quote: fact.source_quote,
                score,
            });
        }
    }

    for entry in state.list_continuity_ledger_entries(input.project_id)? {
        let Some((_, title)) = chapters.get(&entry.chapter_id) else {
            continue;
        };
        let Some(source) =
            state.latest_approved_chapter_body(input.project_id, entry.chapter_id)?
        else {
            continue;
        };
        if crate::chapter_memory::source_text_hash(&source.content) != entry.source_text_hash {
            continue;
        }
        let haystack = format!(
            "{} {} {} {}",
            entry.entity_label, entry.state_kind, entry.state_value, entry.evidence_quote
        );
        if let Some(score) = fact_match_score(&haystack, &terms) {
            results.push(StoryFactSearchResult {
                fact_type: "continuity_ledger".to_string(),
                source_label: format!("第{}章 {}", chapters[&entry.chapter_id].0, title),
                chapter_id: Some(entry.chapter_id),
                entity_label: Some(entry.entity_label),
                dimension: entry.state_kind,
                value: entry.state_value,
                status: "active".to_string(),
                evidence_quote: entry.evidence_quote,
                score,
            });
        }
    }

    for chapter_id in chapters.keys().copied() {
        let Some(memory) =
            crate::chapter_memory::current_memory_for_chapter(state, input.project_id, chapter_id)?
        else {
            continue;
        };
        let Ok(payload) =
            serde_json::from_str::<crate::chapter_memory::ChapterMemoryPayload>(&memory.content)
        else {
            continue;
        };
        let Some((chapter_no, title)) = chapters.get(&chapter_id) else {
            continue;
        };
        let source_label = format!("第{}章 {}（章节记忆）", chapter_no, title);
        let groups = [
            ("state_change", payload.state_changes),
            ("knowledge_change", payload.knowledge_changes),
            ("commitment", payload.commitments),
            ("open_loop", payload.open_loops),
        ];
        for (fact_type, entries) in groups {
            for item in entries {
                let haystack = format!("{} {}", item.text, item.evidence_quote);
                if let Some(score) = fact_match_score(&haystack, &terms) {
                    results.push(StoryFactSearchResult {
                        fact_type: fact_type.to_string(),
                        source_label: source_label.clone(),
                        chapter_id: Some(chapter_id),
                        entity_label: None,
                        dimension: fact_type.to_string(),
                        value: item.text,
                        status: "active".to_string(),
                        evidence_quote: item.evidence_quote,
                        score,
                    });
                }
            }
        }
    }

    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.source_label.cmp(&right.source_label))
    });
    results.truncate(limit);
    Ok(results)
}

fn fact_match_score(text: &str, terms: &[String]) -> Option<usize> {
    let normalized = text.to_lowercase();
    let matched = terms
        .iter()
        .filter(|term| normalized.contains(&term.to_lowercase()))
        .count();
    (matched > 0).then_some(matched * 100 + text.chars().count().min(80))
}

pub(super) fn retrieve_history_snippets(
    state: &AppState,
    project_id: i64,
    current_chapter_id: i64,
    query: &str,
    include_immediate_previous: bool,
    include_messages: bool,
) -> AppResult<Vec<StoryContextSnippet>> {
    let indexed = crate::story_search::search_story_context(
        state,
        &StoryContextSearchInput {
            project_id,
            chapter_id: (current_chapter_id > 0).then_some(current_chapter_id),
            query: query.to_string(),
            limit: Some(6),
            include_immediate_previous,
        },
    )?;
    if !indexed.is_empty() {
        return Ok(indexed);
    }

    let terms = extract_history_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let chapters = state.list_chapters(project_id)?;
    let current_chapter_no = chapters
        .iter()
        .find(|chapter| chapter.id == current_chapter_id)
        .map(|chapter| chapter.chapter_no)
        .unwrap_or_default();

    let mut candidates = Vec::new();
    let chapter_upper_bound = if include_immediate_previous {
        current_chapter_no
    } else {
        current_chapter_no.saturating_sub(1)
    };
    for chapter in chapters
        .iter()
        .filter(|chapter| chapter.chapter_no < chapter_upper_bound)
    {
        if let Some(artifact) = state.latest_approved_chapter_body(project_id, chapter.id)? {
            if let Some(mut snippet) = best_snippet_for_text(
                &artifact.content,
                &terms,
                &format!("第 {} 章 {}", chapter.chapter_no, chapter.title),
            ) {
                // Exact-term relevance is usually tied across chapters. Prefer the
                // latest canonical occurrence before static cards or older prose.
                snippet.score += 1_000 + chapter.chapter_no.max(0) as usize;
                candidates.push(snippet);
            }
        }
    }

    for stage in ["setting", "outline", "characters"] {
        if let Some(artifact) = state.approved_artifact(project_id, stage, None)? {
            if let Some(snippet) =
                best_snippet_for_text(&artifact.content, &terms, stage_label(stage))
            {
                candidates.push(snippet);
            }
        }
    }

    if include_messages {
        for message in state.list_messages(project_id)?.into_iter().take(40) {
            if let Some(snippet) =
                best_snippet_for_text(&message.content, &terms, export_role_label(&message.role))
            {
                candidates.push(snippet);
            }
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.content.len().cmp(&b.content.len()))
    });
    candidates
        .dedup_by(|a, b| a.source_label == b.source_label && a.matched_term == b.matched_term);

    Ok(candidates.into_iter().take(6).collect())
}

fn best_snippet_for_text(
    text: &str,
    terms: &[String],
    source_label: &str,
) -> Option<StoryContextSnippet> {
    let mut best: Option<StoryContextSnippet> = None;
    for term in terms {
        if let Some(index) = text.find(term) {
            let score = score_term(term);
            let snippet = StoryContextSnippet {
                source_label: source_label.to_string(),
                matched_term: term.clone(),
                content: excerpt_around(text, index, term.chars().count(), 180),
                score,
            };
            if best
                .as_ref()
                .map(|current| snippet.score > current.score)
                .unwrap_or(true)
            {
                best = Some(snippet);
            }
        }
    }
    best
}

pub(super) fn extract_history_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();

    for token in split_query_tokens(query) {
        let trimmed = token.trim();
        if trimmed.is_empty() || is_noise_term(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            terms.push(trimmed.to_string());
        }
    }

    terms.sort_by(|a, b| {
        score_term(b)
            .cmp(&score_term(a))
            .then_with(|| b.len().cmp(&a.len()))
    });
    terms.truncate(14);
    terms
}
