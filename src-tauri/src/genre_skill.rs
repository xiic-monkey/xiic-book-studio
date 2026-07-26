use crate::models::Stage;
use std::{env, fs, path::PathBuf};

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

pub fn genre_skill_for_id(skill_id: &str) -> Option<GenreSkillKind> {
    match skill_id {
        "general_serialized" => Some(GenreSkillKind::GeneralSerialized),
        "urban_supernatural" => Some(GenreSkillKind::UrbanSupernatural),
        "mystery" => Some(GenreSkillKind::Mystery),
        "xianxia_power_fantasy" => Some(GenreSkillKind::XianxiaPowerFantasy),
        _ => None,
    }
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
            name: "通用连载写法",
            category: "genre",
            description: "适用于未细分题材的连载节奏、章节功能和修订原则。",
            content: GENERAL_SERIALIZED_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: GenreSkillKind::UrbanSupernatural.skill_id(),
            name: "都市异能",
            category: "genre",
            description: "强调异能边界、现代社会反馈、身份变化和能力成长。",
            content: URBAN_SUPERNATURAL_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: GenreSkillKind::Mystery.skill_id(),
            name: "悬疑",
            category: "genre",
            description: "强调谜面、证据链、知情边界、解释竞争和公平性。",
            content: MYSTERY_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: GenreSkillKind::XianxiaPowerFantasy.skill_id(),
            name: "男频修仙爽文",
            category: "genre",
            description: "强调升级流长线发育、资源收益、压迫链和不同章节模式。",
            content: XIANXIA_POWER_FANTASY_FALLBACK,
        },
        DefaultWritingSkill {
            skill_key: "continuity_and_agency",
            name: "长篇连续性与主动性",
            category: "craft",
            description: "跨题材补充旧角色已知信息、物件状态、主角主动选择和探索章控密度。",
            content: CONTINUITY_AND_AGENCY_FALLBACK,
        },
    ]
}

pub fn render_genre_skill_from_template(
    genre: &str,
    stage: &Stage,
    skill_id: &str,
    template: &str,
) -> String {
    let skill = detect_genre_skill(genre);
    render_stage_scoped_skill(
        "题材 Skill",
        if skill_id.trim().is_empty() {
            skill.skill_id()
        } else {
            skill_id.trim()
        },
        &format!(
            "当前项目题材：{}。以下不是具体某本书的设定，而是这一类题材在当前阶段必须兑现的写法约束。",
            genre.trim()
        ),
        stage,
        template,
    )
}

pub fn render_genre_skill(genre: &str, stage: &Stage) -> String {
    let skill = detect_genre_skill(genre);
    let template = load_skill_template(skill);
    render_genre_skill_from_template(genre, stage, skill.skill_id(), &template)
}

fn load_skill_template(skill: GenreSkillKind) -> String {
    read_runtime_skill(skill).unwrap_or_else(|| skill.fallback_template().to_string())
}

fn read_runtime_skill(skill: GenreSkillKind) -> Option<String> {
    runtime_skill_candidates(skill)
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
}

fn runtime_skill_candidates(skill: GenreSkillKind) -> Vec<PathBuf> {
    let filename = format!("{}.md", skill.skill_id());
    let mut dirs = Vec::new();

    if let Ok(dir) = env::var("XIIC_BOOK_STUDIO_SKILLS_DIR") {
        dirs.push(PathBuf::from(dir).join("genres"));
    }
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        dirs.push(PathBuf::from(manifest_dir).join("skills").join("genres"));
    }
    if let Ok(current_dir) = env::current_dir() {
        dirs.push(current_dir.join("src-tauri").join("skills").join("genres"));
        dirs.push(current_dir.join("skills").join("genres"));
    }

    dirs.into_iter().map(|dir| dir.join(&filename)).collect()
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
        let text = render_genre_skill("男频修仙爽文", &Stage::Draft);
        assert!(text.contains("应用内写作资料库 / xianxia_power_fantasy"));
        assert!(text.contains("男频修仙/升级流"));
        assert!(text.contains("发育修炼章"));
        assert!(text.contains("不要连续堆谜团而不给进度"));
    }
}
