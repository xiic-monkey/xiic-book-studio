use crate::{
    error::{AppError, AppResult},
    models::Agent,
};

pub const DEFAULT_PROMPT_VERSION: &str = "v2";

pub const BUILTIN_AGENT_KEYS: &[&str] = &[
    "orchestrator",
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
];

pub fn default_prompt(agent_key: &str) -> Option<&'static str> {
    Some(
        match agent_key {
            "orchestrator" => include_str!("../prompts/v2/orchestrator.md"),
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
        for key in BUILTIN_AGENT_KEYS {
            assert!(!require_default_prompt(key).unwrap().is_empty());
        }
        assert_eq!(DEFAULT_PROMPT_VERSION, "v2");
    }

    #[test]
    fn builtin_agent_registry_has_fifteen_unique_keys() {
        let keys = BUILTIN_AGENT_KEYS
            .iter()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(BUILTIN_AGENT_KEYS.len(), 15);
        assert_eq!(keys.len(), BUILTIN_AGENT_KEYS.len());
    }

    #[test]
    fn adoption_prompt_declares_machine_readable_contract() {
        let prompt = require_default_prompt("adoption").unwrap();
        for fragment in [
            "只返回一个 JSON 对象",
            "items",
            "没有可安全采纳的候选时返回 `{\"items\":[]}`",
            "target_kind",
            "target_id",
            "operation",
            "data",
            "canon_entry` 允许 `category`、`title`、`content`、`source_chapter_id",
            "foreshadowing` 允许 `title`、`content`、`planted_chapter_id",
            "evidence_quote",
            "canon_entry",
            "foreshadowing",
            "create",
            "update",
        ] {
            assert!(
                prompt.contains(fragment),
                "missing adoption prompt fragment: {fragment}"
            );
        }
    }

    #[test]
    fn stage_output_contracts_live_in_runtime_prompts() {
        let draft = require_default_prompt("draft").unwrap();
        let revision = require_default_prompt("revision").unwrap();
        let artifact_revision = require_default_prompt("artifact_revision").unwrap();

        assert!(!draft.contains("只输出章节正文"));
        assert!(!revision.contains("只输出修订后的完整正文"));
        assert!(!artifact_revision.contains("只输出替换后的新片段正文"));
    }
}
