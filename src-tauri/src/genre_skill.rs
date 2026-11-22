use crate::{
    db::AppState,
    error::AppResult,
    models::{Project, Stage},
};

const CRAFT_KEY: &str = "continuity_and_agency";

/// 运行时真正会注入的辅助 Skill key。
///
/// 许可集合（`agents.allowed_skill_keys`）必须与这里一致：留着运行时不会
/// 注入的 key，只会让界面显示出实际上不生效的白名单。
pub(crate) fn supported_auxiliary_skill_keys() -> &'static [&'static str] {
    &[CRAFT_KEY]
}

const GENERAL_SERIALIZED_FALLBACK: &str = include_str!("../skills/genres/general_serialized.md");
const URBAN_SUPERNATURAL_FALLBACK: &str = include_str!("../skills/genres/urban_supernatural.md");
const MYSTERY_FALLBACK: &str = include_str!("../skills/genres/mystery.md");
const XIANXIA_POWER_FANTASY_FALLBACK: &str =
    include_str!("../skills/genres/xianxia_power_fantasy.md");
const CONTINUITY_AND_AGENCY_FALLBACK: &str =
    include_str!("../skills/craft/continuity_and_agency.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenreSkillKind {
    GeneralSerialized,
    UrbanSupernatural,
    Mystery,
    XianxiaPowerFantasy,
}

impl GenreSkillKind {
    pub fn skill_id(self) -> &'static str {
        match self {
            GenreSkillKind::GeneralSerialized => "general_serialized",
            GenreSkillKind::UrbanSupernatural => "urban_supernatural",
            GenreSkillKind::Mystery => "mystery",
            GenreSkillKind::XianxiaPowerFantasy => "xianxia_power_fantasy",
        }
    }

    pub fn fallback_template(self) -> &'static str {
        match self {
            GenreSkillKind::GeneralSerialized => GENERAL_SERIALIZED_FALLBACK,
            GenreSkillKind::UrbanSupernatural => URBAN_SUPERNATURAL_FALLBACK,
            GenreSkillKind::Mystery => MYSTERY_FALLBACK,
            GenreSkillKind::XianxiaPowerFantasy => XIANXIA_POWER_FANTASY_FALLBACK,
        }
    }

    pub fn default_name(self) -> &'static str {
        match self {
            Self::GeneralSerialized => "通用连载写法",
            Self::UrbanSupernatural => "都市异能",
            Self::Mystery => "悬疑",
            Self::XianxiaPowerFantasy => "男频修仙爽文",
        }
    }
}

pub fn detect_genre_skill(genre: &str) -> GenreSkillKind {
    let lowered = genre.to_lowercase();
    if lowered.contains("修仙")
        || lowered.contains("玄幻")
        || lowered.contains("仙侠")
        || lowered.contains("升级")
        || lowered.contains("男频")
    {
        GenreSkillKind::XianxiaPowerFantasy
    } else if lowered.contains("悬疑") || lowered.contains("怪谈") || lowered.contains("推理")
    {
        GenreSkillKind::Mystery
    } else if lowered.contains("异能") || lowered.contains("超能") || lowered.contains("灵气复苏")
    {
        GenreSkillKind::UrbanSupernatural
    } else {
        GenreSkillKind::GeneralSerialized
    }
}

#[derive(serde::Serialize)]
pub(crate) struct StageSkillContext {
    pub selected_genre_key: &'static str,
    pub genre_rules: String,
    pub craft_rules: String,
}

impl StageSkillContext {
    pub fn append_to(&self, prompt: &mut String) {
        prompt.push_str(&self.genre_rules);
        prompt.push_str(&self.craft_rules);
    }
}

pub(crate) fn load_stage_skills(
    state: &AppState,
    project: &Project,
    stage: &Stage,
    allowed_skill_keys: &[String],
) -> AppResult<StageSkillContext> {
    let skill = detect_genre_skill(&project.genre);
    let selected_genre_key = skill.skill_id();
    let genre_rules = state
        .effective_genre_skill_content(skill)?
        .map(|template| {
            render_genre_skill_from_template(&project.genre, stage, selected_genre_key, &template)
        })
        .unwrap_or_default();
    let craft_rules = if allowed_skill_keys.iter().any(|key| key == CRAFT_KEY) {
        state
            .get_writing_skill_by_key(CRAFT_KEY)?
            .filter(|record| record.enabled && record.category == "craft")
            .map(|record| {
                render_stage_scoped_skill(
                    "写作 Craft Skill",
                    CRAFT_KEY,
                    &format!(
                        "技能名称：{}。以下是跨题材复用的写作工艺约束。",
                        record.name
                    ),
                    stage,
                    &record.content,
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(StageSkillContext {
        selected_genre_key,
        genre_rules,
        craft_rules,
    })
}

pub struct DefaultWritingSkill {
    pub skill_key: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub content: &'static str,
}

pub fn default_writing_skills() -> Vec<DefaultWritingSkill> {
    vec![
        DefaultWritingSkill {
            skill_key: GenreSkillKind::GeneralSerialized.skill_id(),
            name: GenreSkillKind::GeneralSerialized.default_name(),
            category: "genre",
            description: "适用于未细分题材的连载节奏、章节功能和修订原则。",
            content: GENERAL_SERIALIZED_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: GenreSkillKind::UrbanSupernatural.skill_id(),
            name: GenreSkillKind::UrbanSupernatural.default_name(),
            category: "genre",
            description: "强调异能边界、现代社会反馈、身份变化和能力成长。",
            content: URBAN_SUPERNATURAL_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: GenreSkillKind::Mystery.skill_id(),
            name: GenreSkillKind::Mystery.default_name(),
            category: "genre",
            description: "强调谜面、证据链、知情边界、解释竞争和公平性。",
            content: MYSTERY_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: GenreSkillKind::XianxiaPowerFantasy.skill_id(),
            name: GenreSkillKind::XianxiaPowerFantasy.default_name(),
            category: "genre",
            description: "强调升级流长线发育、资源收益、压迫链和不同章节模式。",
            content: XIANXIA_POWER_FANTASY_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: CRAFT_KEY,
            name: "长篇连续性与主动性",
            category: "craft",
            description: "跨题材补充旧角色已知信息、物件状态、主角主动选择和探索章控密度。",
            content: CONTINUITY_AND_AGENCY_FALLBACK,
        },
    ]
}

fn render_genre_skill_from_template(
    genre: &str,
    stage: &Stage,
    skill_id: &str,
    template: &str,
) -> String {
    render_stage_scoped_skill(
        "题材 Skill",
        skill_id,
        &format!(
            "当前项目题材：{}。以下不是具体某本书的设定，而是这一类题材在当前阶段必须兑现的写法约束。",
            genre.trim()
        ),
        stage,
        template,
    )
}

fn section(template: &str, heading: &str) -> String {
    let marker = format!("## {heading}");
    let mut collecting = false;
    let mut lines = Vec::new();

    for line in template.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if collecting {
                break;
            }
            collecting = trimmed == marker;
            continue;
        }
        if collecting {
            lines.push(line);
        }
    }

    let body = lines.join("\n").trim().to_string();
    if body.is_empty() {
        String::new()
    } else {
        format!("\n{body}")
    }
}

pub fn render_stage_scoped_skill(
    title: &str,
    source_label: &str,
    intro: &str,
    stage: &Stage,
    template: &str,
) -> String {
    let always = section(template, "Always");
    let stage_rules = section(template, &format!("Stage: {}", stage.as_str()));
    if always.is_empty() && stage_rules.is_empty() {
        return String::new();
    }

    let mut output = format!(
        "\n\n# {}\nSkill 来源：应用内写作资料库 / {}\n{}",
        title,
        source_label.trim(),
        intro.trim()
    );
    if !always.trim().is_empty() {
        output.push_str("\n\n## 通用规则");
        output.push_str(&always);
    }
    if !stage_rules.trim().is_empty() {
        output.push_str(&format!("\n\n## {} 阶段规则", stage.title()));
        output.push_str(&stage_rules);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_urban_supernatural_and_mystery_skills() {
        assert_eq!(
            detect_genre_skill("都市异能"),
            GenreSkillKind::UrbanSupernatural
        );
        assert_eq!(detect_genre_skill("悬疑"), GenreSkillKind::Mystery);
        assert_eq!(detect_genre_skill("都市悬疑"), GenreSkillKind::Mystery);
        assert_eq!(
            detect_genre_skill("都市生活"),
            GenreSkillKind::GeneralSerialized
        );
        assert_eq!(
            detect_genre_skill("科幻"),
            GenreSkillKind::GeneralSerialized
        );
        assert_eq!(
            detect_genre_skill("奇幻"),
            GenreSkillKind::GeneralSerialized
        );
        assert_eq!(
            detect_genre_skill("历史"),
            GenreSkillKind::GeneralSerialized
        );
    }

    #[test]
    fn more_specific_genre_markers_win_when_genre_is_composite() {
        assert_eq!(detect_genre_skill("都市悬疑"), GenreSkillKind::Mystery);
        assert_eq!(detect_genre_skill("都市异能悬疑"), GenreSkillKind::Mystery);
        assert_eq!(
            detect_genre_skill("都市异能升级流"),
            GenreSkillKind::XianxiaPowerFantasy
        );
    }

    #[test]
    fn detects_xianxia_skill() {
        assert_eq!(
            detect_genre_skill("男频修仙爽文"),
            GenreSkillKind::XianxiaPowerFantasy
        );
    }

    #[test]
    fn renders_stage_specific_xianxia_guidance() {
        let text = render_genre_skill_from_template(
            "男频修仙爽文",
            &Stage::Draft,
            GenreSkillKind::XianxiaPowerFantasy.skill_id(),
            XIANXIA_POWER_FANTASY_FALLBACK,
        );
        assert!(text.contains("应用内写作资料库 / xianxia_power_fantasy"));
        assert!(text.contains("男频修仙/升级流"));
        assert!(text.contains("发育修炼章"));
        assert!(text.contains("不要连续堆谜团而不给进度"));
    }

    #[test]
    fn stage_skills_render_only_nonempty_matching_sections() {
        let render = |template| {
            render_stage_scoped_skill(
                "题材 Skill",
                "mystery",
                "题材规则",
                &Stage::Review,
                template,
            )
        };
        assert!(render("").is_empty());
        assert!(render("## Always\n \n## Stage: draft\n仅写作使用").is_empty());
        assert!(render("## Always\n通用标记").contains("通用标记"));
        let stage_only = render("## Stage: review\n试读标记\n## Stage: revision\n修订标记");
        assert!(stage_only.contains("试读标记"));
        assert!(!stage_only.contains("修订标记"));
        assert!(!stage_only.contains("## 通用规则"));
    }

    #[test]
    fn stage_skills_respect_craft_enablement_and_genre_empty_content() {
        let database = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(database.path().to_path_buf()).unwrap();
        let project = state
            .create_project(crate::models::NewProject {
                title: "题材与 Craft 开关".to_string(),
                genre: "悬疑".to_string(),
                target_words: 100_000,
                premise: "验证独立开关".to_string(),
            })
            .unwrap();
        let allowed = [CRAFT_KEY.to_string()];
        let load = || load_stage_skills(&state, &project, &Stage::Review, &allowed).unwrap();
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE writing_skills SET content = ' ' WHERE skill_key = 'mystery'",
                    [],
                )?;
                conn.execute(
                    "UPDATE writing_skills SET content = ?1 WHERE skill_key = ?2",
                    ("## Stage: review\n自定义 Craft 规则", CRAFT_KEY),
                )?;
                Ok(())
            })
            .unwrap();
        let skills = load();
        assert_eq!(skills.selected_genre_key, "mystery");
        assert!(skills.genre_rules.is_empty());
        assert!(skills.craft_rules.contains("自定义 Craft 规则"));
        state
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE writing_skills SET enabled = 0 WHERE skill_key = ?1",
                    [CRAFT_KEY],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(load().craft_rules.is_empty());
    }

    #[test]
    fn every_skill_template_covers_all_stages() {
        let templates = [
            ("general_serialized", GENERAL_SERIALIZED_FALLBACK),
            ("urban_supernatural", URBAN_SUPERNATURAL_FALLBACK),
            ("mystery", MYSTERY_FALLBACK),
            ("xianxia_power_fantasy", XIANXIA_POWER_FANTASY_FALLBACK),
            ("continuity_and_agency", CONTINUITY_AND_AGENCY_FALLBACK),
        ];
        let stages = [
            "setting",
            "outline",
            "characters",
            "draft",
            "review",
            "revision",
        ];
        for (key, template) in templates {
            assert!(
                !section(template, "Always").trim().is_empty(),
                "skill {key} 缺少 Always 段"
            );
            for stage in stages {
                assert!(
                    !section(template, &format!("Stage: {stage}"))
                        .trim()
                        .is_empty(),
                    "skill {key} 缺少 Stage: {stage} 段（会被静默丢弃）"
                );
            }
        }
    }

    /// craft skill 面向全部题材，规则只能陈述在状态类别层面；具象题材
    /// 道具词汇只允许出现在 genres/ 下的题材 skill 里。
    #[test]
    fn craft_skill_stays_free_of_genre_specific_props() {
        let genre_props = [
            "信物", "封印", "阵法", "丹炉", "丹药", "禁制", "灵物", "炼化", "血脉", "修炼",
            "炼药", "令牌", "修为",
        ];
        for prop in genre_props {
            assert!(
                !CONTINUITY_AND_AGENCY_FALLBACK.contains(prop),
                "craft skill 出现题材道具词汇: {prop}"
            );
        }
    }
}
