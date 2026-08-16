use std::collections::HashSet;

use crate::models::{AgentToolDefinition, ToolKind};
use serde_json::json;

pub const HISTORY_CONTEXT: &str = "history_context";
pub const REFERENCE_MATERIALS: &str = "reference_materials";
pub const CHAPTER_MEMORY: &str = "chapter_memory";
pub const CONTINUITY_CHECK: &str = "continuity_check";
pub const QUALITY_ANALYSIS: &str = "quality_analysis";
pub const CHAPTER_SPLIT: &str = "chapter_split";
pub const WEB_SEARCH: &str = "web_search";
pub const PROPOSE_CREATE_CHAPTER: &str = "propose_create_chapter";
pub const PROPOSE_RENAME_CHAPTER: &str = "propose_rename_chapter";
pub const PROPOSE_ARTIFACT_CANDIDATE: &str = "propose_artifact_candidate";
pub const PROPOSE_KNOWLEDGE_CARD: &str = "propose_knowledge_card";
pub const PROPOSE_UPDATE_KNOWLEDGE_CARD: &str = "propose_update_knowledge_card";
pub const PROPOSE_DELETE_KNOWLEDGE_CARD: &str = "propose_delete_knowledge_card";
pub const PROPOSE_FORESHADOWING: &str = "propose_foreshadowing";

const BOOK_STAGES: &[&str] = &["setting", "outline", "characters"];
const CHAPTER_STAGES: &[&str] = &["draft", "review", "revision"];
const ALL_STAGES: &[&str] = &[
    "setting",
    "outline",
    "characters",
    "draft",
    "review",
    "revision",
];

fn definition(
    key: &str,
    name: &str,
    description: &str,
    category: &str,
    kind: &str,
    supported_stages: &[&str],
    previewable: bool,
    parameters_schema: serde_json::Value,
) -> AgentToolDefinition {
    AgentToolDefinition {
        key: key.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        category: category.to_string(),
        kind: ToolKind::parse(kind),
        supported_stages: supported_stages
            .iter()
            .map(|stage| (*stage).to_string())
            .collect(),
        previewable,
        parameters_schema,
    }
}

pub fn definitions() -> Vec<AgentToolDefinition> {
    vec![
        definition(
            HISTORY_CONTEXT,
            "历史上下文检索",
            "检索已批准章节、故事索引和历史事实，为当前任务补充证据。",
            "上下文",
            "read",
            CHAPTER_STAGES,
            true,
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "minLength": 1, "maxLength": 240},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 12}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        definition(
            REFERENCE_MATERIALS,
            "参考资料",
            "检索当前项目导入并启用的外部参考资料。",
            "上下文",
            "read",
            ALL_STAGES,
            true,
            json!({
                "type": "object",
                "properties": {"query": {"type": "string", "maxLength": 500}},
                "additionalProperties": false
            }),
        ),
        definition(
            CHAPTER_MEMORY,
            "章节记忆",
            "读取上一章的事实交接记忆，减少长篇续写中的状态丢失。",
            "连续性",
            "read",
            CHAPTER_STAGES,
            true,
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
        ),
        definition(
            CONTINUITY_CHECK,
            "连续性检查",
            "使用状态账本核对物件、资源、禁制和伤势等已确认状态。",
            "连续性",
            "read",
            CHAPTER_STAGES,
            true,
            json!({
                "type": "object",
                "properties": {"artifact_id": {"type": "integer", "minimum": 1}},
                "additionalProperties": false
            }),
        ),
        definition(
            QUALITY_ANALYSIS,
            "质量分析",
            "读取候选产物的本地质量信号。",
            "审校",
            "read",
            &["review", "revision"],
            true,
            json!({
                "type": "object",
                "properties": {"artifact_id": {"type": "integer", "minimum": 1}},
                "required": ["artifact_id"],
                "additionalProperties": false
            }),
        ),
        definition(
            CHAPTER_SPLIT,
            "拆章规划",
            "分析超载章节并生成可执行的拆章方案。",
            "规划",
            "read",
            &["draft", "revision"],
            true,
            json!({
                "type": "object",
                "properties": {"artifact_id": {"type": "integer", "minimum": 1}},
                "required": ["artifact_id"],
                "additionalProperties": false
            }),
        ),
        definition(
            WEB_SEARCH,
            "网络搜索",
            "仅在人工明确要求联网时查询现实资料；结果不自动成为 Canon。",
            "研究",
            "read",
            ALL_STAGES,
            true,
            json!({
                "type": "object",
                "properties": {"query": {"type": "string", "minLength": 1, "maxLength": 240}},
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_CREATE_CHAPTER,
            "提议创建章节",
            "创建一个待人工确认的新章节提案，不直接写入章节表。",
            "编辑提案",
            "proposal",
            BOOK_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {"title": {"type": "string", "minLength": 1, "maxLength": 120}},
                "required": ["title"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_RENAME_CHAPTER,
            "提议重命名章节",
            "创建章节重命名提案，不直接修改章节。",
            "编辑提案",
            "proposal",
            ALL_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {
                    "chapter_id": {"type": "integer", "minimum": 1},
                    "title": {"type": "string", "minLength": 1, "maxLength": 120}
                },
                "required": ["chapter_id", "title"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_ARTIFACT_CANDIDATE,
            "提议资料候选版本",
            "为设定、大纲或角色资料创建待人工确认的候选版本。",
            "编辑提案",
            "proposal",
            BOOK_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {
                    "stage": {"type": "string", "enum": ["setting", "outline", "characters"]},
                    "title": {"type": "string", "minLength": 1, "maxLength": 160},
                    "content": {"type": "string", "minLength": 1}
                },
                "required": ["stage", "title", "content"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_KNOWLEDGE_CARD,
            "提议知识卡",
            "创建待人工确认的知识卡提案。",
            "编辑提案",
            "proposal",
            ALL_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string", "minLength": 1, "maxLength": 80},
                    "title": {"type": "string", "minLength": 1, "maxLength": 160},
                    "content": {"type": "string", "minLength": 1}
                },
                "required": ["category", "title", "content"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_UPDATE_KNOWLEDGE_CARD,
            "更新知识卡",
            "更新已存在知识卡的标题、类型或内容，创建待人工确认的提案。",
            "编辑提案",
            "proposal",
            ALL_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {
                    "card_id": {"type": "integer", "minimum": 1},
                    "category": {"type": "string", "minLength": 1, "maxLength": 80},
                    "title": {"type": "string", "minLength": 1, "maxLength": 160},
                    "content": {"type": "string", "minLength": 1}
                },
                "required": ["card_id", "category", "title", "content"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_DELETE_KNOWLEDGE_CARD,
            "删除知识卡",
            "删除已存在的知识卡，创建待人工确认的提案。",
            "编辑提案",
            "proposal",
            ALL_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {
                    "card_id": {"type": "integer", "minimum": 1}
                },
                "required": ["card_id"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_FORESHADOWING,
            "提议伏笔",
            "创建待人工确认的伏笔提案。",
            "编辑提案",
            "proposal",
            ALL_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "minLength": 1, "maxLength": 160},
                    "content": {"type": "string", "minLength": 1},
                    "planned_payoff_note": {"type": "string", "maxLength": 500}
                },
                "required": ["title", "content"],
                "additionalProperties": false
            }),
        ),
    ]
}

pub fn default_keys() -> Vec<String> {
    definitions()
        .into_iter()
        .filter(|tool| tool.kind == ToolKind::Read)
        .map(|tool| tool.key)
        .collect()
}

pub fn normalize_keys(keys: &[String]) -> Vec<String> {
    let selected = keys.iter().map(|key| key.trim()).collect::<HashSet<_>>();
    definitions()
        .into_iter()
        .filter(|tool| selected.contains(tool.key.as_str()))
        .map(|tool| tool.key)
        .collect()
}

pub fn has_tool(keys: &[String], key: &str) -> bool {
    keys.iter().any(|item| item == key)
}

pub fn get(key: &str) -> Option<AgentToolDefinition> {
    definitions().into_iter().find(|tool| tool.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_to_registered_order_and_drops_unknown_keys() {
        let keys = normalize_keys(&vec![
            CHAPTER_SPLIT.to_string(),
            "unknown".to_string(),
            HISTORY_CONTEXT.to_string(),
            HISTORY_CONTEXT.to_string(),
        ]);
        assert_eq!(
            keys,
            vec![HISTORY_CONTEXT.to_string(), CHAPTER_SPLIT.to_string()]
        );
    }

    #[test]
    fn empty_allowlist_is_a_real_disable_state() {
        assert!(!has_tool(&[], HISTORY_CONTEXT));
        assert!(normalize_keys(&[]).is_empty());
    }
}
