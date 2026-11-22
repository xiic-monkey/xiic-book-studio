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

pub(crate) fn apply_stage_protocol(mut agent: Agent) -> Agent {
    if PROTOCOL_AGENT_KEYS.contains(&agent.stage.as_str()) {
        agent.system_prompt = format!(
            "{}\n\n# 共享工作协议\n{}",
            agent.system_prompt, SHARED_PROTOCOL
        );
    }
    agent
}
