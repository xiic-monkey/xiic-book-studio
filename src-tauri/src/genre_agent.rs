use crate::{
    error::{AppError, AppResult},
    genre_skill::{self, GenreSkillKind},
    models::{Agent, GenreAgentProfile},
};

const GENERAL_PROMPT: &str = "你是通用连载小说专属 Agent。你长期负责没有归入更窄类型的商业连载项目，核心判断标准是读者承诺、人物欲望、章节推进和稳定追读。你可以使用通用连载方法，但不得擅自借用都市异能、悬疑或修仙类型的专属套路。";
const URBAN_SUPERNATURAL_PROMPT: &str = "你是都市异能小说专属 Agent。你长期负责超常能力进入现代社会后产生的身份、资源、组织、职业与现实关系变化。所有阶段都要维护能力合同、成长代价、暴露风险和社会反馈；不得默认把故事写成案件调查，也不能为了过关临时扩充能力。";
const MYSTERY_PROMPT: &str = "你是悬疑小说专属 Agent。你长期负责谜面、证据链、知情差、嫌疑变化与解释竞争。所有阶段都要维护线索来源、角色知情边界、推理可追溯性和公平性；不得默认存在异能体系，也不能用无来源的新证据制造反转。";
const XIANXIA_PROMPT: &str = "你是男频修仙/玄幻升级流专属 Agent。你长期负责资源循环、修炼成长、压迫链、能力合同和阶段性收益驱动的项目。所有阶段都要维护实力边界、资源来源、升级代价和爽点兑现；不能把一次性触发偷换成永久能力。";

const BASE_AGENT_PROTOCOL: &str = r#"这是所有 Agent 共享、优先级最高的工作协议：
1. 事实优先级依次为：已通过正式正文与带来源引文的状态记录；已通过创作基准、设定、大纲和角色资料；本次人工指令；题材与写作 Skill。Skill 只能提供方法，不能补充本书事实。
2. 未经人工通过的候选稿、试读意见和模型推测都不是 Canon。你只能生成候选产物，不能替人工确认，也不能宣称已经写入正式资料。
3. 资料缺失时保留未知；资料冲突时明确指出冲突或采用不越界的最低事实，不能静默发明解释。
4. 只完成当前阶段职责。故事架构 Agent 不代写正文，写作 Agent 不修改 Canon，试读 Agent 不编造修法，修订 Agent 不引入来源中不存在的新事实。
5. 章节结构由本章模式和实际内容决定。允许修炼、恢复、交易、关系、探索和过渡章节；不强制固定场景数、固定中段反转或危险式章末钩子。章节仍需产生可感知的状态变化，并形成自然的结束或延续。"#;

pub fn default_genre_agents() -> Vec<GenreAgentProfile> {
    vec![
        profile(
            "general_serialized",
            "通用连载 Agent",
            "负责未细分题材的商业连载创作",
            GENERAL_PROMPT,
            GenreSkillKind::GeneralSerialized,
        ),
        profile(
            "urban_supernatural",
            "都市异能 Agent",
            "专注能力边界、成长代价与现代社会反馈",
            URBAN_SUPERNATURAL_PROMPT,
            GenreSkillKind::UrbanSupernatural,
        ),
        profile(
            "mystery",
            "悬疑 Agent",
            "专注谜面、证据链、知情边界与公平推理",
            MYSTERY_PROMPT,
            GenreSkillKind::Mystery,
        ),
        profile(
            "xianxia_power_fantasy",
            "男频修仙/玄幻 Agent",
            "专注升级、资源、能力边界与阶段收益",
            XIANXIA_PROMPT,
            GenreSkillKind::XianxiaPowerFantasy,
        ),
    ]
}

fn profile(
    agent_key: &str,
    name: &str,
    role: &str,
    system_prompt: &str,
    genre_skill: GenreSkillKind,
) -> GenreAgentProfile {
    GenreAgentProfile {
        agent_key: agent_key.to_string(),
        name: name.to_string(),
        role: role.to_string(),
        system_prompt: system_prompt.to_string(),
        primary_skill_key: genre_skill.skill_id().to_string(),
        allowed_skill_keys: vec![
            genre_skill.skill_id().to_string(),
            "continuity_and_agency".to_string(),
        ],
    }
}

pub fn detect_genre_agent(genre: &str) -> AppResult<GenreAgentProfile> {
    profile_for_key(genre_skill::detect_genre_skill(genre).skill_id())
        .ok_or_else(|| AppError::Validation(format!("未找到题材 '{genre}' 对应的 Agent 配置")))
}

pub fn profile_for_key(agent_key: &str) -> Option<GenreAgentProfile> {
    default_genre_agents()
        .into_iter()
        .find(|profile| profile.agent_key == agent_key)
}

pub fn compose_stage_agent(mut stage_agent: Agent, profile: &GenreAgentProfile) -> Agent {
    let effective_skill_keys = stage_agent
        .allowed_skill_keys
        .iter()
        .filter(|key| {
            profile
                .allowed_skill_keys
                .iter()
                .any(|allowed| allowed == *key)
        })
        .cloned()
        .collect::<Vec<_>>();
    stage_agent.system_prompt = format!(
        "# 共享 Agent 协议\n{}\n\n# 题材专属 Agent 身份\n{}\n\n你当前绑定的主类型 Skill 是 `{}`。只允许使用白名单 Skill：{}。即使技能库中存在其他题材规则，也不能调用、混合或模仿它们。\n\n# 当前工作模式\n{}",
        BASE_AGENT_PROTOCOL,
        profile.system_prompt,
        profile.primary_skill_key,
        if effective_skill_keys.is_empty() {
            "（无辅助 Skill）".to_string()
        } else {
            effective_skill_keys.join("、")
        },
        stage_agent.system_prompt
    );
    stage_agent.role = format!("{}；当前工作模式：{}", profile.role, stage_agent.role);
    stage_agent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_supported_genres_to_specialists() {
        assert_eq!(
            detect_genre_agent("都市异能").unwrap().agent_key,
            "urban_supernatural"
        );
        assert_eq!(detect_genre_agent("悬疑").unwrap().agent_key, "mystery");
        assert_eq!(
            detect_genre_agent("男频修仙").unwrap().agent_key,
            "xianxia_power_fantasy"
        );
        assert_eq!(
            detect_genre_agent("历史").unwrap().agent_key,
            "general_serialized"
        );
    }

    #[test]
    fn specialist_exposes_only_its_genre_skill_and_shared_allowlist() {
        let profile = detect_genre_agent("悬疑").unwrap();
        assert!(profile.allowed_skill_keys.contains(&"mystery".to_string()));
        assert!(!profile
            .allowed_skill_keys
            .contains(&"urban_supernatural".to_string()));
        assert!(!profile
            .allowed_skill_keys
            .contains(&"xianxia_power_fantasy".to_string()));
    }

    #[test]
    fn composed_agent_separates_fact_protocol_from_genre_and_stage_roles() {
        let profile = detect_genre_agent("男频修仙").unwrap();
        let agent = compose_stage_agent(
            Agent {
                id: 1,
                stage: "draft".to_string(),
                name: "写作 Agent".to_string(),
                role: "写正文".to_string(),
                editable_role: "写正文".to_string(),
                system_prompt: "只输出正文。".to_string(),
                editable_system_prompt: "只输出正文。".to_string(),
                temperature: 0.7,
                provider_base_url: "https://api.example.com".to_string(),
                model: "example-model".to_string(),
                thinking_enabled: false,
                thinking_level: "off".to_string(),
                uses_global_runtime_settings: false,
                enabled_tool_keys: crate::agent_tools::default_keys(),
                allowed_skill_keys: vec!["continuity_and_agency".to_string()],
            },
            &profile,
        );

        assert!(agent.system_prompt.contains("# 共享 Agent 协议"));
        assert!(agent
            .system_prompt
            .contains("Skill 只能提供方法，不能补充本书事实"));
        assert!(agent
            .system_prompt
            .contains("不强制固定场景数、固定中段反转"));
        assert!(agent.system_prompt.contains("# 题材专属 Agent 身份"));
        assert!(agent.system_prompt.contains("# 当前工作模式"));
        assert!(agent
            .system_prompt
            .contains("男频修仙/玄幻升级流专属 Agent"));
        assert!(agent.role.contains("专注升级、资源、能力边界与阶段收益"));
        assert!(agent.role.contains("写正文"));
        assert!(!agent.editable_role.contains("男频修仙/玄幻升级流"));
        assert_eq!(agent.editable_role, "写正文");
    }
}
