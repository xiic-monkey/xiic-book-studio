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
    "canon_align",
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
            "canon_align" => include_str!("../prompts/v2/canon_align.md"),
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
    fn builtin_agent_registry_has_sixteen_unique_keys() {
        let keys = BUILTIN_AGENT_KEYS
            .iter()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(BUILTIN_AGENT_KEYS.len(), 16);
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

    /// 边界规则：.md 模板 = 人格 + 行为准则 + 输出契约；Rust 侧 user prompt
    /// 只注入运行时数据。下表为每个内置 Agent 在 .md 中的契约锚点。
    #[test]
    fn builtin_prompt_contracts_live_in_prompt_templates() {
        let anchors: &[(&str, &[&str])] = &[
            ("orchestrator", &["只输出一个 JSON 对象", "kind"]),
            ("story_architect", &["Canon"]),
            ("draft", &["只输出正文", "资料汇总"]),
            (
                "review",
                &[
                    "submit_review_issues",
                    "issue_type",
                    "evidence_quote",
                    "severity",
                ],
            ),
            (
                "revision",
                &["输出修订后的完整章节正文", "修订不可突破的事实边界"],
            ),
            ("adoption", &["只返回一个 JSON 对象", "evidence_quote"]),
            ("story_index", &["entities", "events", "facts"]),
            ("canon_align", &["updates", "archive_ids", "writing_notes"]),
            (
                "chapter_memory",
                &["state_changes", "knowledge_changes", "evidence_quote"],
            ),
            (
                "continuity_ledger",
                &["entity_kind", "state_value", "evidence_quote"],
            ),
            (
                "continuity_check",
                &["claim_mode", "required_state", "evidence_quote"],
            ),
            ("context_search_plan", &["searches", "evidence_quote"]),
            (
                "context_search_rerank",
                &["selections", "candidate_id", "core"],
            ),
            (
                "continuity_review",
                &["issues", "issue_type", "evidence_quote", "同场景衔接"],
            ),
            (
                "chapter_split_plan",
                &["suggested_current_title", "next_chapter_instruction"],
            ),
            ("artifact_revision", &["只输出", "替换后的新片段"]),
        ];
        for (key, fragments) in anchors {
            let prompt = require_default_prompt(key).unwrap();
            for fragment in *fragments {
                assert!(
                    prompt.contains(fragment),
                    "agent {key} prompt 模板缺少契约片段: {fragment}"
                );
            }
        }
    }

    #[test]
    fn stage_output_contracts_live_in_prompt_templates() {
        let draft = require_default_prompt("draft").unwrap();
        let revision = require_default_prompt("revision").unwrap();
        let artifact_revision = require_default_prompt("artifact_revision").unwrap();

        assert!(draft.contains("只输出正文"));
        assert!(revision.contains("输出修订后的完整章节正文"));
        assert!(artifact_revision.contains("只输出"));
        // 运行时 user prompt 不再重复输出契约。
        assert!(!draft.contains("本章柔性任务契约"));
    }

    /// 通用层提示词面向全部题材服务，规则只能陈述在状态类别层面；
    /// 具象题材道具词汇只允许出现在题材 skill 或运行时数据里。
    #[test]
    fn builtin_prompts_stay_free_of_genre_specific_props() {
        let genre_props = [
            "信物", "封印", "阵法", "丹炉", "丹药", "禁制", "灵物", "炼化", "血脉", "修炼",
            "炼药", "令牌", "修为",
        ];
        for key in BUILTIN_AGENT_KEYS {
            let prompt = require_default_prompt(key).unwrap();
            for prop in genre_props {
                assert!(
                    !prompt.contains(prop),
                    "agent {key} 的内置提示词出现题材道具词汇: {prop}"
                );
            }
        }
    }
}
