use crate::models::Agent;

const PROTOCOL_AGENT_KEYS: &[&str] = &[
    "story_architect",
    "draft",
    "review",
    "revision",
    "orchestrator",
];

const SHARED_PROTOCOL: &str = r#"1. 事实优先级依次为：正文与带来源引文的状态记录；已通过创作基准、设定、大纲和角色资料；本次人工指令；题材与写作 Skill。Skill 只能提供方法，不能补充本书事实。
2. 未经人工通过的候选稿、试读意见和模型推测都不是 Canon。你只能生成候选产物，不能替人工确认，也不能宣称已经写入正式资料。
3. 资料缺失时保留未知；资料冲突时明确指出冲突或采用不越界的最低事实，不能静默发明解释。
4. 只完成当前阶段职责。故事架构 Agent 不代写正文，写作 Agent 不修改 Canon，试读 Agent 不编造修法，修订 Agent 不引入来源中不存在的新事实。
"#;

/// 共享协议段的标题分隔串。`apply_stage_protocol` 用它追加，`review_protocol_prompt`
/// 用它摘出再拼回，避免两处硬编码的标题不一致。
const SHARED_PROTOCOL_HEADING: &str = "\n\n# 共享工作协议\n";

pub(crate) fn apply_stage_protocol(mut agent: Agent) -> Agent {
    if PROTOCOL_AGENT_KEYS.contains(&agent.stage.as_str()) {
        agent.system_prompt = format!(
            "{}{}{}",
            agent.system_prompt, SHARED_PROTOCOL_HEADING, SHARED_PROTOCOL
        );
    }
    agent
}

/// 试读输出协议的段标记。
///
/// 两套协议都写在 `prompts/v2/review.md` 里，运行时只按供应商是否支持原生
/// 工具调用选一段，避免在 Rust 侧复制一份输出契约。
const REVIEW_TOOL_PROTOCOL_MARKER: &str = "<!-- protocol:tool -->";
const REVIEW_JSON_PROTOCOL_MARKER: &str = "<!-- protocol:json -->";

/// 按供应商工具能力挑选试读 Agent 的输出协议段。
///
/// 支持原生工具调用时用 `submit_review_issues` 提交，否则退回 JSON 数组输出。
/// 用户自定义过的 Prompt 没有协议标记，原样保留。
pub(crate) fn apply_review_protocol(mut agent: Agent, use_tool_protocol: bool) -> Agent {
    agent.system_prompt = review_protocol_prompt(&agent.system_prompt, use_tool_protocol);
    agent
}

fn review_protocol_prompt(prompt: &str, use_tool_protocol: bool) -> String {
    // 共享协议由 apply_stage_protocol 追加在末尾，处在 json 段之后。切分时先摘出，
    // 拼好选中的协议段后再拼回，否则工具分支会整段丢掉它；JSON 回退用的裸 Agent
    // 也要在调用本函数前经过 apply_stage_protocol 才有这段协议。
    let (body, shared) = match prompt.find(SHARED_PROTOCOL_HEADING) {
        Some(index) => (
            &prompt[..index],
            Some(prompt[index + SHARED_PROTOCOL_HEADING.len()..].trim().to_string()),
        ),
        None => (prompt, None),
    };
    let Some(tool_start) = body.find(REVIEW_TOOL_PROTOCOL_MARKER) else {
        return prompt.to_string();
    };
    let Some(json_start) = body.find(REVIEW_JSON_PROTOCOL_MARKER) else {
        return prompt.to_string();
    };
    if json_start < tool_start {
        return prompt.to_string();
    }
    let common = body[..tool_start].trim_end();
    let section = if use_tool_protocol {
        body[tool_start + REVIEW_TOOL_PROTOCOL_MARKER.len()..json_start].trim()
    } else {
        body[json_start + REVIEW_JSON_PROTOCOL_MARKER.len()..].trim()
    };
    let mut result = if section.is_empty() {
        common.to_string()
    } else {
        format!("{common}\n\n{section}")
    };
    if let Some(shared) = shared {
        if !shared.is_empty() {
            result.push_str(SHARED_PROTOCOL_HEADING);
            result.push_str(&shared);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_prompt_splits_into_tool_and_json_protocols() {
        let prompt =
            "公共部分\n\n<!-- protocol:tool -->\n工具段\n\n<!-- protocol:json -->\nJSON 段";

        let tool = review_protocol_prompt(prompt, true);
        assert!(tool.contains("公共部分"));
        assert!(tool.contains("工具段"));
        assert!(!tool.contains("JSON 段"));

        let json = review_protocol_prompt(prompt, false);
        assert!(json.contains("公共部分"));
        assert!(json.contains("JSON 段"));
        assert!(!json.contains("工具段"));
    }

    /// 用户自定义过的试读 Prompt 没有协议标记，任何协议选择都不能改写它。
    #[test]
    fn custom_review_prompt_without_markers_is_untouched() {
        let prompt = "我自己写的试读提示词";
        assert_eq!(review_protocol_prompt(prompt, true), prompt);
        assert_eq!(review_protocol_prompt(prompt, false), prompt);
    }

    /// 内置模板必须真的带两套协议，且问题字段规则在两套里都在。
    #[test]
    fn builtin_review_prompt_exposes_both_protocols() {
        let template = crate::prompt_templates::require_default_prompt("review").unwrap();

        let tool = review_protocol_prompt(template, true);
        assert!(tool.contains("用 `submit_review_issues` 工具提交"));
        // 工具版仍保留「没有工具就输出 JSON 数组」的兜底说明。
        assert!(tool.contains("只输出 JSON 数组"));
        assert!(tool.contains("evidence_quote"));

        let json = review_protocol_prompt(template, false);
        assert!(json.contains("只输出 JSON 数组"));
        assert!(!json.contains("submit_review_issues"));
        assert!(json.contains("evidence_quote"));
    }

    /// 共享协议追加在末尾、位于 json 段之后。切分时必须把它摘出再拼回，
    /// 不能因为落在 json 段之后就跟着工具版一起丢掉。用内置真实模板验证。
    #[test]
    fn review_protocol_keeps_shared_protocol_in_both_variants() {
        let template = crate::prompt_templates::require_default_prompt("review").unwrap();
        let prompt = format!("{template}{SHARED_PROTOCOL_HEADING}{SHARED_PROTOCOL}");

        for use_tool in [true, false] {
            let result = review_protocol_prompt(&prompt, use_tool);
            assert!(
                result.contains("# 共享工作协议"),
                "use_tool={use_tool}: 共享协议标题丢失"
            );
            assert!(
                result.contains("事实优先级依次为"),
                "use_tool={use_tool}: 共享协议正文丢失"
            );
        }
        let tool = review_protocol_prompt(&prompt, true);
        assert!(!tool.contains("# 输出格式"));
        let json = review_protocol_prompt(&prompt, false);
        assert!(!json.contains("# 提交方式"));
    }
}
