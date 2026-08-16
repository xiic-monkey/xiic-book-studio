use serde_json::Value;

use crate::{
    ai,
    chapter_memory::source_text_hash,
    db::AppState,
    error::{AppError, AppResult},
    models::{
        AgentRunRequest, CanonIssue, ConfirmStoryBibleRequest, ConfirmStoryBibleReviewRequest,
        RunStoryArchitectRequest, StoryBible, StoryBibleReview, StoryBibleReviewRequest,
    },
};

pub(crate) fn build_agent_run_request(
    state: &AppState,
    input: RunStoryArchitectRequest,
) -> AppResult<AgentRunRequest> {
    let stage = input.mode.artifact_stage();
    let arc_context = input
        .arc_id
        .map(|id| {
            state
                .list_story_arcs(input.project_id)?
                .into_iter()
                .find(|arc| arc.id == id)
                .ok_or_else(|| AppError::Validation("故事阶段不存在".to_string()))
        })
        .transpose()?;
    let mut instruction = format!(
        "# 故事架构工作模式\n{}\n\n{}",
        input.mode.label(),
        mode_contract(&input.mode)
    );
    if let Some(arc) = arc_context {
        instruction.push_str(&format!(
            "\n\n# 当前故事阶段\n阶段：{}\n目标：{}\n进入局面：{}\n预期变化：{}\n核心冲突：{}\n涉及角色：{}",
            arc.title, arc.objective, arc.entry_state, arc.exit_change, arc.core_conflict, arc.involved_characters
        ));
    }
    if let Some(hint) = input
        .user_instruction
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        instruction.push_str(&format!("\n\n# 人工追加指令\n{}", hint.trim()));
    }
    Ok(AgentRunRequest {
        project_id: input.project_id,
        stage,
        chapter_id: None,
        user_instruction: Some(instruction),
        source_artifact_id: input.source_artifact_id,
        reference_selection: input.reference_selection,
        prepared_context_id: None,
    })
}

pub fn confirm_story_bible(
    state: &AppState,
    input: ConfirmStoryBibleRequest,
) -> AppResult<StoryBible> {
    for stage in ["setting", "outline", "characters"] {
        if state
            .approved_artifact(input.project_id, stage, None)?
            .is_none()
        {
            return Err(AppError::Validation(format!(
                "确认创作基准前，请先人工通过{}资料",
                stage_label(stage)
            )));
        }
    }
    let setting = state
        .approved_artifact(input.project_id, "setting", None)?
        .ok_or_else(|| AppError::Validation("缺少已通过设定资料".to_string()))?;
    let outline = state
        .approved_artifact(input.project_id, "outline", None)?
        .ok_or_else(|| AppError::Validation("缺少已通过阶段大纲".to_string()))?;
    let bible = state.upsert_story_bible_from_artifact(input.project_id, &setting, "confirmed")?;
    state.ensure_active_story_arc_from_outline(input.project_id, &outline)?;
    state.insert_message(
        input.project_id,
        None,
        "approval_note",
        &format!("人工确认创作基准。{}", input.note.trim()),
    )?;
    Ok(bible)
}

pub async fn review_story_bible(
    state: &AppState,
    input: StoryBibleReviewRequest,
) -> AppResult<StoryBibleReview> {
    let bible = state
        .get_story_bible(input.project_id)?
        .ok_or_else(|| AppError::Validation("请先确认创作基准".to_string()))?;
    if bible.status != "confirmed" {
        return Err(AppError::Validation("创作基准尚未人工确认".to_string()));
    }
    if state.active_story_arc(input.project_id)?.is_none() {
        return Err(AppError::Validation(
            "请先确认一个进行中的故事阶段".to_string(),
        ));
    }
    let snapshot = canonical_snapshot(state, input.project_id)?;
    let fingerprint = source_text_hash(&snapshot);
    let agent = state.get_agent_for_project_stage(input.project_id, "story_architect")?;
    let settings = agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先为当前供应商保存 API Key".to_string()))?;
    let system_prompt = format!(
        "{}\n\n# 当前审校子模式\n你是故事架构 Agent 的一致性审校模式。你不写正文、不新创设定、不替作者做最终决定。只检查已批准 Canon 内部是否能共同成立，并输出可追溯、可定向返工的问题。",
        agent.system_prompt
    );
    let prompt = format!(
        "# 已批准创作基准快照\n{}\n\n# 审校任务\n检查读者承诺、世界规则、能力/资源边界、角色目标与已知信息、阶段大纲因果、时间线、物件状态和伏笔是否一致。\n\n只输出 JSON 对象：{{\"summary\":string,\"issues\":[{{\"domain\":string,\"severity\":\"minor|moderate|major\",\"title\":string,\"conflict\":string,\"impact\":string,\"owner_mode\":\"initialize|refine_canon|plan_current_arc|extend_next_arc|design_characters\",\"rework_instruction\":string,\"evidence_quotes\":[string]}}]}}。\n每条 evidence_quotes 必须逐字来自快照；没有确凿问题时返回空数组。major 只用于真实规则、因果、人物动机/知识或时间线冲突。",
        snapshot
    );
    let output = ai::complete_chat(
        &settings,
        &api_key,
        &system_prompt,
        &prompt,
        agent.temperature,
    )
    .await?;
    let (summary, issues) = parse_review(&output, &snapshot)?;
    let verdict = if issues.iter().any(|issue| issue.severity == "major") {
        "needs_revision"
    } else if issues.is_empty() {
        "strong"
    } else {
        "attention"
    };
    let review = state.insert_story_bible_review(
        input.project_id,
        &fingerprint,
        verdict,
        &summary,
        &serde_json::to_string(&issues)?,
    )?;
    state.insert_message(
        input.project_id,
        None,
        "agent_result",
        &format!("故事架构 Agent 完成一致性审校：{}", review.verdict),
    )?;
    Ok(review)
}

pub fn confirm_story_bible_review(
    state: &AppState,
    input: ConfirmStoryBibleReviewRequest,
) -> AppResult<StoryBibleReview> {
    let review = state
        .latest_story_bible_review(input.project_id)?
        .ok_or_else(|| AppError::Validation("尚无创作基准审校记录".to_string()))?;
    if review.id != input.review_id {
        return Err(AppError::Validation(
            "只能确认最新的创作基准审校".to_string(),
        ));
    }
    let fingerprint = canonical_fingerprint(state, input.project_id)?;
    if review.canon_fingerprint != fingerprint {
        return Err(AppError::Validation(
            "Canon 已变化，请重新运行一致性审校".to_string(),
        ));
    }
    if review.issues.iter().any(|issue| issue.severity == "major") {
        return Err(AppError::Validation(
            "存在 major 一致性问题，不能确认通过".to_string(),
        ));
    }
    let review =
        state.confirm_story_bible_review(input.project_id, input.review_id, &input.note)?;
    state.mark_story_bible_confirmed(input.project_id)?;
    Ok(review)
}

pub fn ensure_ready_for_draft(state: &AppState, project_id: i64) -> AppResult<()> {
    let bible = state
        .get_story_bible(project_id)?
        .ok_or_else(|| AppError::Validation("正文前请先确认创作基准".to_string()))?;
    if bible.status != "confirmed" {
        return Err(AppError::Validation("创作基准尚未确认".to_string()));
    }
    if state.active_story_arc(project_id)?.is_none() {
        return Err(AppError::Validation(
            "正文前请先确认进行中的故事阶段".to_string(),
        ));
    }
    let review = state
        .latest_story_bible_review(project_id)?
        .ok_or_else(|| AppError::Validation("正文前请先运行创作基准一致性审校".to_string()))?;
    if review.canon_fingerprint != canonical_fingerprint(state, project_id)? {
        return Err(AppError::Validation(
            "Canon 已变化，请重新运行一致性审校".to_string(),
        ));
    }
    if review.status != "confirmed" {
        return Err(AppError::Validation(
            "请人工确认最新的创作基准审校".to_string(),
        ));
    }
    if review.issues.iter().any(|issue| issue.severity == "major") {
        return Err(AppError::Validation(
            "存在未解决的 major 一致性问题".to_string(),
        ));
    }
    Ok(())
}

pub fn canonical_fingerprint(state: &AppState, project_id: i64) -> AppResult<String> {
    Ok(source_text_hash(&canonical_snapshot(state, project_id)?))
}

pub fn canonical_snapshot(state: &AppState, project_id: i64) -> AppResult<String> {
    let artifacts = ["setting", "outline", "characters"]
        .into_iter()
        .filter_map(|stage| state.approved_artifact(project_id, stage, None).transpose())
        .collect::<AppResult<Vec<_>>>()?;
    let value = serde_json::json!({
        "story_bible": state.get_story_bible(project_id)?,
        "foundation_artifacts": artifacts,
        "story_arcs": state.list_story_arcs(project_id)?,
        "canon_cards": state.list_knowledge_cards(project_id)?.into_iter().filter(|card| card.status == "approved").collect::<Vec<_>>(),
        "foreshadowings": state.list_foreshadowings(project_id)?.into_iter().filter(|item| matches!(item.status.as_str(), "active" | "ready_for_payoff" | "resolved")).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map_err(AppError::from)
}

fn mode_contract(mode: &crate::models::StoryArchitectMode) -> &'static str {
    match mode {
        crate::models::StoryArchitectMode::Initialize => "建立故事内核、可执行世界规则、远期方向、第一故事阶段和初始角色。远期方向只保留路标，禁止伪造完整章节细节。",
        crate::models::StoryArchitectMode::RefineCanon => "只补充当前 Canon 真正需要的新规则、势力、地点、物件或边界。每项必须说明叙事用途、引入路径和代价。",
        crate::models::StoryArchitectMode::PlanCurrentArc => "细化当前故事阶段：目标、进入局面、核心冲突、退出变化、相关角色和近期章节任务。已通过正式章节只能作为已发生事实被总结，必须从第一章尚无正式正文的章节继续规划；不得回写、改名或用规划版本替代已写内容。为当前阶段补充读者主要期待、推进证据、局部回报、尚未兑现项和对下一阶段形成的新条件；回报可以是理解、情绪、关系、能力、资源或目标变化，不规定固定章数和爽点频率。不要把更远阶段写死。",
        crate::models::StoryArchitectMode::ExtendNextArc => "基于当前阶段结局、正式章节、活跃伏笔与角色状态，提出下一故事阶段的候选方向；已通过正式章节和已经形成的阶段结果不可重写。每个候选阶段说明读者主要期待、可验证的推进证据、局部回报、继续保留的未兑现项和阶段结束后的新条件；不按目标字数平均切块，也不规定固定回报频率。新要素必须说明从何而来。",
        crate::models::StoryArchitectMode::DesignCharacters => "补充或修订角色卡。角色必须有自身目标、限制、知道什么、入场路径，以及如何改变当前阶段的因果。",
    }
}

fn parse_review(raw: &str, corpus: &str) -> AppResult<(String, Vec<CanonIssue>)> {
    let value: Value = serde_json::from_str(trim_json(raw))
        .map_err(|error| AppError::Validation(format!("无法解析创作基准审校：{error}")))?;
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("已完成创作基准审校。")
        .trim()
        .to_string();
    let mut issues: Vec<CanonIssue> = serde_json::from_value(
        value
            .get("issues")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![])),
    )?;
    for issue in &mut issues {
        issue
            .evidence_quotes
            .retain(|quote| !quote.trim().is_empty() && corpus.contains(quote));
        if issue.severity == "major" && issue.evidence_quotes.is_empty() {
            issue.severity = "moderate".to_string();
            issue
                .conflict
                .push_str("（原 major 缺少可验证证据，已降级。）");
        }
        if !matches!(issue.severity.as_str(), "minor" | "moderate" | "major") {
            issue.severity = "moderate".to_string();
        }
        if !matches!(
            issue.owner_mode.as_str(),
            "initialize"
                | "refine_canon"
                | "plan_current_arc"
                | "extend_next_arc"
                | "design_characters"
        ) {
            issue.owner_mode = "refine_canon".to_string();
        }
    }
    Ok((summary, issues))
}

fn trim_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim()
}

fn stage_label(stage: &str) -> &'static str {
    match stage {
        "setting" => "设定",
        "outline" => "阶段大纲",
        "characters" => "角色",
        _ => "基础",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::AppState,
        models::{NewProject, SaveKnowledgeCard, Stage},
    };

    fn state_with_foundation() -> (tempfile::NamedTempFile, AppState, i64) {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "烬骨长生".to_string(),
                genre: "男频修仙".to_string(),
                target_words: 200_000,
                premise: "废徒求生".to_string(),
            })
            .unwrap();
        for (stage, content) in [
            ("setting", "焚炉谷以妖骨换取灵砂，火脉残缺者只能做杂役。"),
            ("outline", "第一阶段：陆烬在焚炉谷活下来并拿到第一份机缘。"),
            ("characters", "陆烬想摆脱杂役身份，底线是不拿同伴换资源。"),
        ] {
            let artifact = state
                .insert_artifact(project.id, None, stage, stage, content, None)
                .unwrap();
            state
                .approve_stage(project.id, stage, artifact.id, "通过")
                .unwrap();
        }
        (file, state, project.id)
    }

    #[test]
    fn foundation_stages_resolve_to_one_story_architect() {
        let (_file, state, _project_id) = state_with_foundation();
        let architect_id = state.get_agent("story_architect").unwrap().id;
        for stage in ["setting", "outline", "characters"] {
            assert_eq!(state.get_agent_for_stage(stage).unwrap().id, architect_id);
            assert!(state.get_agent(stage).is_err());
        }
    }

    #[test]
    fn arc_planning_contract_protects_written_chapters_and_tracks_reader_returns() {
        let contract = mode_contract(&crate::models::StoryArchitectMode::PlanCurrentArc);

        assert!(contract.contains("第一章尚无正式正文的章节"));
        assert!(contract.contains("不得回写、改名"));
        assert!(contract.contains("读者主要期待、推进证据、局部回报、尚未兑现项"));
        assert!(contract.contains("不规定固定章数和爽点频率"));
    }

    #[test]
    fn draft_requires_current_confirmed_story_bible_review() {
        let (_file, state, project_id) = state_with_foundation();
        confirm_story_bible(
            &state,
            ConfirmStoryBibleRequest {
                project_id,
                note: "确认".to_string(),
            },
        )
        .unwrap();
        assert!(ensure_ready_for_draft(&state, project_id).is_err());
        let fingerprint = canonical_fingerprint(&state, project_id).unwrap();
        let review = state
            .insert_story_bible_review(project_id, &fingerprint, "strong", "一致", "[]")
            .unwrap();
        state
            .confirm_story_bible_review(project_id, review.id, "确认")
            .unwrap();
        assert!(ensure_ready_for_draft(&state, project_id).is_ok());
        let extra = state
            .insert_artifact(
                project_id,
                None,
                Stage::Setting.as_str(),
                "补充",
                "新增规则",
                None,
            )
            .unwrap();
        state
            .approve_stage(project_id, "setting", extra.id, "通过")
            .unwrap();
        assert!(ensure_ready_for_draft(&state, project_id).is_err());
    }

    #[test]
    fn approved_manual_canon_change_requires_another_review() {
        let (_file, state, project_id) = state_with_foundation();
        confirm_story_bible(
            &state,
            ConfirmStoryBibleRequest {
                project_id,
                note: "确认".to_string(),
            },
        )
        .unwrap();
        let fingerprint = canonical_fingerprint(&state, project_id).unwrap();
        let review = state
            .insert_story_bible_review(project_id, &fingerprint, "strong", "一致", "[]")
            .unwrap();
        state
            .confirm_story_bible_review(project_id, review.id, "确认")
            .unwrap();
        state.mark_story_bible_confirmed(project_id).unwrap();

        state
            .save_knowledge_card(SaveKnowledgeCard {
                id: None,
                project_id,
                category: "world".to_string(),
                title: "火脉边界".to_string(),
                content: "火脉残缺者不得直接炼化赤焰。".to_string(),
                status: "approved".to_string(),
                source_artifact_id: None,
                source_chapter_id: None,
            })
            .unwrap();

        assert_eq!(
            state.get_story_bible(project_id).unwrap().unwrap().status,
            "needs_review"
        );
        assert!(ensure_ready_for_draft(&state, project_id).is_err());
    }

    #[test]
    fn invalid_review_owner_mode_is_normalized() {
        let (summary, issues) = parse_review(
            r#"{"summary":"存在冲突","issues":[{"domain":"规则","severity":"major","title":"冲突","conflict":"前后矛盾","impact":"影响正文","owner_mode":"unknown_mode","rework_instruction":"补规则","evidence_quotes":["火脉"]}]}"#,
            "火脉残缺者不得直接炼化赤焰。",
        ).unwrap();
        assert_eq!(summary, "存在冲突");
        assert_eq!(issues[0].owner_mode, "refine_canon");
    }
}
