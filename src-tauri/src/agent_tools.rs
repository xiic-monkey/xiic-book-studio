use std::collections::HashSet;

use crate::models::{AgentToolDefinition, ToolKind};
use serde_json::json;

pub const HISTORY_CONTEXT: &str = "history_context";
pub const SEARCH_STORY: &str = "search_story";
pub const SEARCH_STORY_FACTS: &str = "search_story_facts";
pub const REFERENCE_MATERIALS: &str = "reference_materials";
pub const CHAPTER_MEMORY: &str = "chapter_memory";
pub const REQUEST_CHAPTER_MEMORY: &str = "request_chapter_memory";
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
pub const REPLACE_TEXT: &str = "replace_text";
pub const INSERT_AFTER: &str = "insert_after";
pub const DELETE_RANGE: &str = "delete_range";
pub const APPLY_PATCH: &str = "apply_patch";

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
            SEARCH_STORY,
            "故事原文检索",
            "检索已生效的章节正文、设定、提纲和故事原文片段；需要查找事件经过或人物出现位置时使用。",
            "上下文",
            "read",
            CHAPTER_STAGES,
            true,
            json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":240},"limit":{"type":"integer","minimum":1,"maximum":12}},"required":["query"],"additionalProperties":false}),
        ),
        definition(
            SEARCH_STORY_FACTS,
            "故事事实检索",
            "只检索已确认的结构化事实、连续性状态和有效章节记忆，不替代正文检索。",
            "连续性",
            "read",
            CHAPTER_STAGES,
            true,
            json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":240},"limit":{"type":"integer","minimum":1,"maximum":12}},"required":["query"],"additionalProperties":false}),
        ),
        definition(
            HISTORY_CONTEXT,
            "历史上下文检索（兼容）",
            "兼容旧 Agent 配置，等价于故事原文检索。",
            "上下文",
            "read",
            CHAPTER_STAGES,
            true,
            json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":240},"limit":{"type":"integer","minimum":1,"maximum":12}},"required":["query"],"additionalProperties":false}),
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
            REQUEST_CHAPTER_MEMORY,
            "委托生成章节记忆",
            "委托章节记忆 Agent 从已生效正文提取事实交接记忆；主 Agent 不直接写入记忆。",
            "连续性",
            "delegate",
            CHAPTER_STAGES,
            false,
            json!({
                "type": "object",
                "properties": {"chapter_id": {"type": "integer", "minimum": 1}},
                "required": ["chapter_id"],
                "additionalProperties": false
            }),
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
            REPLACE_TEXT,
            "替换文本",
            "在当前章节候选稿中，按唯一原文锚点替换一处文本；生成章节修订候选，不直接覆盖正文。",
            "章节修订",
            "proposal",
            &["draft", "revision"],
            false,
            json!({
                "type": "object",
                "properties": {
                    "artifact_id": {"type": "integer", "minimum": 1},
                    "find_text": {"type": "string", "minLength": 1},
                    "replace_text": {"type": "string"},
                    "note": {"type": "string", "maxLength": 500}
                },
                "required": ["artifact_id", "find_text", "replace_text"],
                "additionalProperties": false
            }),
        ),
        definition(
            INSERT_AFTER,
            "插入文本",
            "在当前章节候选稿的唯一原文锚点后插入文本；生成章节修订候选。",
            "章节修订",
            "proposal",
            &["draft", "revision"],
            false,
            json!({
                "type": "object",
                "properties": {
                    "artifact_id": {"type": "integer", "minimum": 1},
                    "anchor_text": {"type": "string", "minLength": 1},
                    "insert_text": {"type": "string", "minLength": 1},
                    "note": {"type": "string", "maxLength": 500}
                },
                "required": ["artifact_id", "anchor_text", "insert_text"],
                "additionalProperties": false
            }),
        ),
        definition(
            DELETE_RANGE,
            "删除范围",
            "删除当前章节候选稿中唯一匹配的原文范围；生成章节修订候选。",
            "章节修订",
            "proposal",
            &["draft", "revision"],
            false,
            json!({
                "type": "object",
                "properties": {
                    "artifact_id": {"type": "integer", "minimum": 1},
                    "find_text": {"type": "string", "minLength": 1},
                    "note": {"type": "string", "maxLength": 500}
                },
                "required": ["artifact_id", "find_text"],
                "additionalProperties": false
            }),
        ),
        definition(
            APPLY_PATCH,
            "应用多处修订",
            "按顺序应用多条基于唯一原文锚点的章节修订；生成一个章节修订候选。",
            "章节修订",
            "proposal",
            &["draft", "revision"],
            false,
            json!({
                "type": "object",
                "properties": {
                    "artifact_id": {"type": "integer", "minimum": 1},
                    "operations": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "type": {"type": "string", "enum": ["replace_text", "insert_after", "delete_range"]},
                                "find_text": {"type": "string"},
                                "anchor_text": {"type": "string"},
                                "replace_text": {"type": "string"},
                                "insert_text": {"type": "string"}
                            },
                            "required": ["type"],
                            "additionalProperties": false
                        }
                    },
                    "note": {"type": "string", "maxLength": 500}
                },
                "required": ["artifact_id", "operations"],
                "additionalProperties": false
            }),
        ),
        definition(
            PROPOSE_CREATE_CHAPTER,
            "提议创建章节",
            "创建一个待人工确认的新章节提案，不直接写入章节表。",
            "章节创建",
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
            "章节修订",
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
        .filter(|tool| matches!(tool.kind, ToolKind::Read | ToolKind::Delegate))
        .filter(|tool| tool.key != HISTORY_CONTEXT)
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
