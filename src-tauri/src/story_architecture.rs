use serde_json::Value;

use crate::{
    ai,
    chapter_memory::source_text_hash,
    db::AppState,
    error::{AppError, AppResult},
    models::{
        AgentRunRequest, CanonIssue, ConfirmCurrentPlanRequest, ConfirmStoryBibleRequest,
        ConfirmStoryBibleReviewRequest, CurrentPlanConfirmationResult,
        CurrentPlanConfirmationStatus, RunStoryArchitectRequest, StoryBible, StoryBibleReview,
        StoryBibleReviewRequest,
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
        let has_approved_artifact = state
            .approved_artifact(input.project_id, stage, None)?
            .is_some();
        let has_approved_card = state
            .list_canon_entries(input.project_id)?
            .into_iter()
            .any(|card| {
                card.status == "approved"
                    && match stage {
                        "setting" => matches!(
                            card.category.as_str(),
                            "world" | "cultivation" | "map" | "faction" | "taboo" | "item" | "rule"
                        ),
                        "outline" => matches!(card.category.as_str(), "outline" | "chapter_plan"),
                        "characters" => card.category == "character",
                        _ => false,
                    }
            });
        let has_approved_plan = stage == "outline"
            && state
                .list_chapter_plans(input.project_id)?
                .into_iter()
                .any(|plan| plan.status == "approved" && !plan.content.trim().is_empty());
        if !has_approved_artifact && !has_approved_card && !has_approved_plan {
            return Err(AppError::Validation(format!(
                "确认创作基准前，请先人工通过{}资料",
                stage_label(stage)
            )));
        }
    }
    state.confirm_story_bible_atomic(input.project_id, &input.note)
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
    generate_story_bible_review(state, input.project_id).await
}

pub async fn confirm_current_plan(
    state: &AppState,
    input: ConfirmCurrentPlanRequest,
) -> AppResult<CurrentPlanConfirmationResult> {
    state.get_project(input.project_id)?;
    let initial_story_bible = state.get_story_bible(input.project_id)?;
    let initial_review = state.latest_story_bible_review(input.project_id)?;
    if let (Some(review), Ok(fingerprint)) = (
        initial_review.as_ref(),
        canonical_fingerprint(state, input.project_id),
    ) {
        let blockers = major_review_blockers(review);
        if review.canon_fingerprint == fingerprint && !blockers.is_empty() {
            return Ok(CurrentPlanConfirmationResult {
                status: CurrentPlanConfirmationStatus::Blocked,
                approved_card_count: 0,
                approved_plan_count: 0,
                story_bible: initial_story_bible,
                review: initial_review,
                blockers,
            });
        }
    }
    let data = state.confirm_current_plan_data_atomic(input.project_id, &input.note)?;
    let story_bible = data
        .story_bible
        .clone()
        .or(state.get_story_bible(input.project_id)?);
    let latest_review = state.latest_story_bible_review(input.project_id)?;

    if !data.blockers.is_empty() {
        return Ok(CurrentPlanConfirmationResult {
            status: CurrentPlanConfirmationStatus::Blocked,
            approved_card_count: data.approved_card_count,
            approved_plan_count: data.approved_plan_count,
            story_bible,
            review: latest_review,
            blockers: data.blockers,
        });
    }

    let fingerprint = canonical_fingerprint(state, input.project_id)?;
    let review_is_current = latest_review
        .as_ref()
        .is_some_and(|review| review.canon_fingerprint == fingerprint);
    if !review_is_current {
        let review = generate_story_bible_review(state, input.project_id).await?;
        return Ok(CurrentPlanConfirmationResult {
            status: CurrentPlanConfirmationStatus::AwaitingReviewConfirmation,
            approved_card_count: data.approved_card_count,
            approved_plan_count: data.approved_plan_count,
            story_bible: state.get_story_bible(input.project_id)?,
            review: Some(review),
            blockers: Vec::new(),
        });
    }

    let review = latest_review.expect("review_is_current implies a review exists");
    let blockers = major_review_blockers(&review);
    if !blockers.is_empty() {
        return Ok(CurrentPlanConfirmationResult {
            status: CurrentPlanConfirmationStatus::Blocked,
            approved_card_count: data.approved_card_count,
            approved_plan_count: data.approved_plan_count,
            story_bible,
            review: Some(review),
            blockers,
        });
    }

    if review.status == "confirmed"
        && story_bible
            .as_ref()
            .is_some_and(|bible| bible.status == "confirmed")
    {
        return Ok(CurrentPlanConfirmationResult {
            status: CurrentPlanConfirmationStatus::Confirmed,
            approved_card_count: data.approved_card_count,
            approved_plan_count: data.approved_plan_count,
            story_bible,
            review: Some(review),
            blockers: Vec::new(),
        });
    }

    let (story_bible, review) =
        state.confirm_current_plan_review_atomic(input.project_id, review.id, &input.note)?;
    Ok(CurrentPlanConfirmationResult {
        status: CurrentPlanConfirmationStatus::Confirmed,
        approved_card_count: data.approved_card_count,
        approved_plan_count: data.approved_plan_count,
        story_bible: Some(story_bible),
        review: Some(review),
        blockers: Vec::new(),
    })
}

async fn generate_story_bible_review(
    state: &AppState,
    project_id: i64,
) -> AppResult<StoryBibleReview> {
    let snapshot = canonical_snapshot(state, project_id)?;
    let fingerprint = source_text_hash(&snapshot);
    let agent = state.get_agent_for_project_stage(project_id, "story_architect")?;
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
        project_id,
        &fingerprint,
        verdict,
        &summary,
        &serde_json::to_string(&issues)?,
    )?;
    state.insert_message(
        project_id,
        None,
        "agent_result",
        &format!("故事架构 Agent 完成一致性审校：{}", review.verdict),
    )?;
    Ok(review)
}

fn major_review_blockers(review: &StoryBibleReview) -> Vec<String> {
    review
        .issues
        .iter()
        .filter(|issue| issue.severity == "major")
        .map(|issue| {
            format!(
                "{}：{} 影响：{} 修复要求：{}",
                issue.title, issue.conflict, issue.impact, issue.rework_instruction
            )
        })
        .collect()
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
    // A Canon fingerprint must represent story content, not workflow metadata.
    // In particular, confirming a review changes the story bible status and
    // updated_at, but does not change the material that was reviewed.
    let artifacts = ["setting", "outline", "characters"]
        .into_iter()
        .filter_map(|stage| state.approved_artifact(project_id, stage, None).transpose())
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .map(|artifact| {
            serde_json::json!({
                "id": artifact.id,
                "chapter_id": artifact.chapter_id,
                "stage": artifact.stage,
                "title": artifact.title,
                "content": artifact.content,
                "version": artifact.version,
                "parent_artifact_id": artifact.parent_artifact_id,
            })
        })
        .collect::<Vec<_>>();
    let story_bible = state.get_story_bible(project_id)?.map(|bible| {
        serde_json::json!({
            "id": bible.id,
            "reader_promise": bible.reader_promise,
            "protagonist_engine": bible.protagonist_engine,
            "core_conflict": bible.core_conflict,
            "endgame_direction": bible.endgame_direction,
            "immutable_rules": bible.immutable_rules,
            "canon_version": bible.canon_version,
            "source_artifact_id": bible.source_artifact_id,
        })
    });
    let story_arcs = state
        .list_story_arcs(project_id)?
        .into_iter()
        .map(|arc| {
            serde_json::json!({
                "id": arc.id,
                "arc_no": arc.arc_no,
                "title": arc.title,
                "objective": arc.objective,
                "entry_state": arc.entry_state,
                "exit_change": arc.exit_change,
                "core_conflict": arc.core_conflict,
                "involved_characters": arc.involved_characters,
                "chapter_start": arc.chapter_start,
                "chapter_end": arc.chapter_end,
                "status": arc.status,
                "source_artifact_id": arc.source_artifact_id,
            })
        })
        .collect::<Vec<_>>();
    let canon_cards = state
        .list_canon_entries(project_id)?
        .into_iter()
        .filter(|card| card.status == "approved")
        .map(|card| {
            serde_json::json!({
                "id": card.id,
                "category": card.category,
                "title": card.title,
                "content": card.content,
                "source_artifact_id": card.source_artifact_id,
                "source_chapter_id": card.source_chapter_id,
            })
        })
        .collect::<Vec<_>>();
    let chapter_plans = state
        .list_chapter_plans(project_id)?
        .into_iter()
        .filter(|plan| plan.status == "approved")
        .map(|plan| {
            serde_json::json!({
                "id": plan.id,
                "chapter_no": plan.chapter_no,
                "title": plan.title,
                "content": plan.content,
                "story_arc_id": plan.story_arc_id,
                "chapter_id": plan.chapter_id,
                "source_artifact_id": plan.source_artifact_id,
            })
        })
        .collect::<Vec<_>>();
    let foreshadowings = state
        .list_foreshadowings(project_id)?
        .into_iter()
        .filter(|item| {
            matches!(
                item.status.as_str(),
                "active" | "ready_for_payoff" | "resolved"
            )
        })
        .map(|item| {
            serde_json::json!({
                "id": item.id,
                "title": item.title,
                "content": item.content,
                "status": item.status,
                "planted_chapter_id": item.planted_chapter_id,
                "planned_payoff_chapter_id": item.planned_payoff_chapter_id,
                "planned_payoff_note": item.planned_payoff_note,
                "source_artifact_id": item.source_artifact_id,
            })
        })
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "story_bible": story_bible,
        "foundation_artifacts": artifacts,
        "story_arcs": story_arcs,
        "canon_cards": canon_cards,
        "chapter_plans": chapter_plans,
        "foreshadowings": foreshadowings,
    });
    serde_json::to_string_pretty(&value).map_err(AppError::from)
}

fn mode_contract(mode: &crate::models::StoryArchitectMode) -> &'static str {
    match mode {
        crate::models::StoryArchitectMode::Initialize => "建立基础世界模型、少量核心角色基础和故事方向，但按资料类别分流写入：世界观只描述世界长期如何运行，角色只写角色资料，故事方向只保留高层路标。禁止在世界观资料中写第一章、主角下一步、压迫链、资源循环、首次收益、升级路线或章节任务。远期方向只保留路标，禁止伪造完整章节细节。对需要沉淀的资料必须逐条调用资料写入工具（save_canon_entry / update_canon_entry）；不要把 Markdown 作为主要交付物。",
        crate::models::StoryArchitectMode::RefineCanon => "只补充当前 Canon 真正需要的长期世界规则、势力、地点、物件或边界。压迫链、资源循环、阶段目标和章节任务属于大纲，不属于世界观；角色变化属于角色资料。每项必须说明其稳定定义，并逐条调用资料写入工具（save_canon_entry / update_canon_entry）；不要把 Markdown 作为主要交付物。",
        crate::models::StoryArchitectMode::PlanCurrentArc => "细化当前故事阶段：目标、进入局面、核心冲突、退出变化、相关角色和近期章节计划。已通过正式章节只能作为已发生事实被总结，必须从第一章尚无正式正文的章节继续规划；不得回写、改名或用规划版本替代已写内容。对每个近期章节逐条调用创建章节计划或更新章节计划，提供 chapter_no、标题、目标、主要阻力、关键行动、必须发生的变化和离开状态；章节计划一律走章节计划工具，不要混入资料条目。为当前阶段补充读者主要期待、推进证据、局部回报、尚未兑现项和对下一阶段形成的新条件；回报可以是理解、情绪、关系、能力、资源或目标变化，不规定固定章数和爽点频率。不要把更远阶段写死。",
        crate::models::StoryArchitectMode::ExtendNextArc => "基于当前阶段结局、正式章节、活跃伏笔与角色状态，提出下一故事阶段的候选方向；已通过正式章节和已经形成的阶段结果不可重写。每个候选阶段说明读者主要期待、可验证的推进证据、局部回报、继续保留的未兑现项和阶段结束后的新条件；不按目标字数平均切块，也不规定固定回报频率。新要素必须说明从何而来。",
        crate::models::StoryArchitectMode::DesignCharacters => "只补充或修订角色信息。角色必须有自身身份、目标、限制、已知信息、关系和长期变化条件；不要把世界规则、压迫链或章节任务写进角色资料。",
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
        models::{NewProject, SaveCanonEntry, Stage},
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
    fn approved_cards_can_confirm_story_bible_without_foundation_artifacts() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let state = AppState::from_path(file.path().to_path_buf()).unwrap();
        let project = state
            .create_project(NewProject {
                title: "卡片项目".to_string(),
                genre: "奇幻".to_string(),
                target_words: 100_000,
                premise: "用卡片维护故事".to_string(),
            })
            .unwrap();
        for (category, title, content) in [
            ("world", "世界规则", "火脉残缺者不能直接炼化赤焰。"),
            ("outline", "第一阶段", "主角先在焚炉谷活下来。"),
            ("character", "陆烬", "陆烬想摆脱杂役身份。"),
        ] {
            state
                .save_canon_entry(SaveCanonEntry {
                    id: None,
                    project_id: project.id,
                    category: category.to_string(),
                    title: title.to_string(),
                    content: content.to_string(),
                    status: "approved".to_string(),
                    source_artifact_id: None,
                    source_chapter_id: None,
                })
                .unwrap();
        }
        let bible = confirm_story_bible(
            &state,
            ConfirmStoryBibleRequest {
                project_id: project.id,
                note: "确认卡片 Canon".to_string(),
            },
        )
        .unwrap();
        assert_eq!(bible.status, "confirmed");
        assert!(state
            .approved_artifact(project.id, "setting", None)
            .unwrap()
            .is_none());
        assert!(state.active_story_arc(project.id).unwrap().is_some());
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
    fn foundation_contracts_keep_world_model_separate_from_plot_planning() {
        let setting = mode_contract(&crate::models::StoryArchitectMode::Initialize);
        let refine = mode_contract(&crate::models::StoryArchitectMode::RefineCanon);
        let outline = mode_contract(&crate::models::StoryArchitectMode::PlanCurrentArc);
        assert!(setting.contains("世界长期如何运行"));
        assert!(setting.contains("压迫链"));
        assert!(refine.contains("压迫链、资源循环、阶段目标和章节任务属于大纲"));
        assert!(outline.contains("核心冲突"));
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
    fn confirming_a_review_does_not_change_the_canonical_fingerprint() {
        let (_file, state, project_id) = state_with_foundation();
        confirm_story_bible(
            &state,
            ConfirmStoryBibleRequest {
                project_id,
                note: "确认".to_string(),
            },
        )
        .unwrap();
        let before = canonical_fingerprint(&state, project_id).unwrap();
        let review = state
            .insert_story_bible_review(project_id, &before, "strong", "一致", "[]")
            .unwrap();

        confirm_story_bible_review(
            &state,
            ConfirmStoryBibleReviewRequest {
                project_id,
                review_id: review.id,
                note: "确认审校".to_string(),
            },
        )
        .unwrap();

        assert_eq!(before, canonical_fingerprint(&state, project_id).unwrap());
        assert!(ensure_ready_for_draft(&state, project_id).is_ok());
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
            .save_canon_entry(SaveCanonEntry {
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
