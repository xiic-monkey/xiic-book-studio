use std::collections::HashSet;

use crate::{
    db::AppState,
    error::AppResult,
    models::{StoryContextSearchInput, StoryContextSnippet},
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
