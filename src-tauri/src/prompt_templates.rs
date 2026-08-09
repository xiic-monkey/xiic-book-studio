use crate::{
    error::{AppError, AppResult},
    models::Agent,
};

pub const DEFAULT_PROMPT_VERSION: &str = "v2";

pub fn default_prompt(agent_key: &str) -> Option<&'static str> {
    Some(
        match agent_key {
            "story_architect" => include_str!("../prompts/v2/story_architect.md"),
            "draft" => include_str!("../prompts/v2/draft.md"),
            "review" => include_str!("../prompts/v2/review.md"),
            "revision" => include_str!("../prompts/v2/revision.md"),
            "adoption" => include_str!("../prompts/v2/adoption.md"),
            "story_index" => include_str!("../prompts/v2/story_index.md"),
            "chapter_memory" => include_str!("../prompts/v2/chapter_memory.md"),
            "continuity_ledger" => include_str!("../prompts/v2/continuity_ledger.md"),
            "continuity_check" => include_str!("../prompts/v2/continuity_check.md"),
            "context_search_plan" => include_str!("../prompts/v2/context_search_plan.md"),
            "context_search_rerank" => include_str!("../prompts/v2/context_search_rerank.md"),
            "continuity_review" => include_str!("../prompts/v2/continuity_review.md"),
            "chapter_split_plan" => include_str!("../prompts/v2/chapter_split_plan.md"),
            "artifact_revision" => include_str!("../prompts/v2/artifact_revision.md"),
            _ => return None,
        }
        .trim(),
    )
}

pub fn require_default_prompt(agent_key: &str) -> AppResult<&'static str> {
    default_prompt(agent_key)
        .ok_or_else(|| AppError::Validation(format!("Agent {agent_key} 没有可恢复的默认 Prompt")))
}

pub fn reset_agent_prompt(state: &crate::db::AppState, agent_id: i64) -> AppResult<Agent> {
    let agent = state.get_agent_by_id(agent_id)?;
    let prompt = require_default_prompt(&agent.stage)?;
    state.replace_agent_prompt(agent_id, prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_agent_has_a_versioned_prompt() {
        for key in [
            "story_architect",
            "draft",
            "review",
            "revision",
            "adoption",
            "story_index",
            "chapter_memory",
            "continuity_ledger",
            "continuity_check",
            "context_search_plan",
            "context_search_rerank",
            "continuity_review",
            "chapter_split_plan",
            "artifact_revision",
        ] {
            assert!(!require_default_prompt(key).unwrap().is_empty());
        }
        assert_eq!(DEFAULT_PROMPT_VERSION, "v2");
    }
}
