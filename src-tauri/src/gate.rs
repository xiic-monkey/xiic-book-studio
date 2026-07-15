use crate::{
    db::AppState,
    error::{AppError, AppResult},
    models::{
        ChapterGateReport, ChapterGateRequest, ContinuityReport, ContinuityReviewRequest,
        GateBlocker, QualityReport,
    },
    quality, workflow,
};

const MIN_ACCEPTABLE_QUALITY: u8 = 72;

pub async fn analyze_chapter_gate(
    state: &AppState,
    input: ChapterGateRequest,
) -> AppResult<ChapterGateReport> {
    let chapter = state
        .ensure_chapter(input.project_id, Some(input.chapter_id))?
        .ok_or_else(|| AppError::Validation("章节不存在".to_string()))?;
    let artifact = state.get_artifact(input.artifact_id)?;
    if artifact.project_id != input.project_id
        || artifact.chapter_id != Some(input.chapter_id)
        || (artifact.stage != "draft" && artifact.stage != "revision")
    {
        return Err(AppError::Validation(
            "通过前检查只能检查当前章节的草稿或修订稿".to_string(),
        ));
    }

    let quality_report = quality::analyze_artifact(&artifact);
    let mut chapter_ids = state
        .list_chapters(input.project_id)?
        .into_iter()
        .filter(|item| item.chapter_no <= chapter.chapter_no)
        .map(|item| item.id)
        .collect::<Vec<_>>();
    if !chapter_ids.contains(&input.chapter_id) {
        chapter_ids.push(input.chapter_id);
    }
    let continuity_report = if chapter_ids.len() < 2 {
        ContinuityReport {
            project_id: input.project_id,
            chapter_titles: vec![chapter.title.clone()],
            verdict: "strong".to_string(),
            summary: "第一章暂无跨章衔接可审校，已跳过连续性硬阻断。".to_string(),
            issues: Vec::new(),
        }
    } else {
        workflow::review_project_continuity(
            state,
            ContinuityReviewRequest {
                project_id: input.project_id,
                chapter_ids: Some(chapter_ids),
                candidate_artifact_id: Some(artifact.id),
                candidate_artifact_ids: None,
            },
        )
        .await?
    };

    let mut blockers = Vec::new();
    blockers.extend(quality_blockers(&quality_report));
    blockers.extend(
        continuity_report
            .issues
            .iter()
            .filter(|issue| issue.severity == "major")
            .map(|issue| GateBlocker {
                kind: "continuity".to_string(),
                severity: issue.severity.clone(),
                title: issue.issue_type.clone(),
                detail: issue.reason.clone(),
                suggestion: issue.suggestion.clone(),
            }),
    );
    blockers.extend(task_adherence_blockers(&chapter.title, &artifact.content));

    let passed = blockers.is_empty()
        && quality_report.score >= MIN_ACCEPTABLE_QUALITY
        && quality_report.verdict != "needs_revision"
        && quality_report.verdict != "weak"
        && continuity_report.verdict != "needs_revision"
        && continuity_report.verdict != "weak";
    let verdict = if passed {
        if quality_report.verdict == "strong" && continuity_report.verdict == "strong" {
            "strong"
        } else {
            "usable"
        }
    } else if blockers.iter().any(|blocker| blocker.severity == "major") {
        "blocked"
    } else {
        "needs_revision"
    }
    .to_string();

    let (recommended_action, action_reason) =
        recommended_action(passed, &quality_report, &continuity_report, &blockers);

    let summary = if passed {
        format!(
            "候选稿已通过检查：质量 {} / {}，连续性 {}。",
            quality_report.score, quality_report.verdict, continuity_report.verdict
        )
    } else {
        format!(
            "候选稿未通过检查：{} 个阻断项。质量 {} / {}，连续性 {}。",
            blockers.len(),
            quality_report.score,
            quality_report.verdict,
            continuity_report.verdict
        )
    };

    Ok(ChapterGateReport {
        project_id: input.project_id,
        chapter_id: input.chapter_id,
        artifact_id: input.artifact_id,
        passed,
        verdict,
        recommended_action,
        action_reason,
        summary,
        blockers,
        quality: quality_report,
        continuity: continuity_report,
    })
}

fn recommended_action(
    passed: bool,
    quality: &QualityReport,
    continuity: &ContinuityReport,
    blockers: &[GateBlocker],
) -> (String, String) {
    if passed {
        return (
            "approve".to_string(),
            "通过前检查未发现硬阻断，仍保留人工最终判断。".to_string(),
        );
    }

    if should_split_chapter(quality, continuity, blockers) {
        return (
            "split".to_string(),
            "候选稿的信息密度或审校意见显示单章负载过高，继续同章修订可能只会反复压缩，建议先拆章或重规划本章任务。"
                .to_string(),
        );
    }

    (
        "revise".to_string(),
        "候选稿存在可通过同章修订处理的问题。".to_string(),
    )
}

fn should_split_chapter(
    quality: &QualityReport,
    continuity: &ContinuityReport,
    blockers: &[GateBlocker],
) -> bool {
    let reveal_count = quality
        .metrics
        .iter()
        .find(|metric| metric.label == "反转/真相信号")
        .map(|metric| metric.value)
        .unwrap_or(0.0);
    let reveal_density = quality
        .metrics
        .iter()
        .find(|metric| metric.label == "反转/真相密度")
        .map(|metric| metric.value)
        .unwrap_or(0.0);
    let has_dense_reveal_blocker = blockers
        .iter()
        .any(|blocker| blocker.title == "信息反转偏密");
    if has_dense_reveal_blocker
        && (reveal_density >= 4.0 || (reveal_count >= 18.0 && reveal_density >= 3.0))
    {
        return true;
    }

    continuity.issues.iter().any(|issue| {
        let text = format!("{} {} {}", issue.issue_type, issue.reason, issue.suggestion);
        issue.severity == "major"
            && (text.contains("拆成两章")
                || text.contains("至少拆分")
                || text.contains("拆分")
                || text.contains("信息倾泻")
                || text.contains("信息轰炸")
                || text.contains("单章内集中")
                || text.contains("单章负载"))
    })
}

fn quality_blockers(report: &QualityReport) -> Vec<GateBlocker> {
    let mut blockers = Vec::new();
    if report.score < MIN_ACCEPTABLE_QUALITY || report.verdict == "weak" {
        blockers.push(GateBlocker {
            kind: "quality".to_string(),
            severity: "major".to_string(),
            title: "质量分未达可用线".to_string(),
            detail: format!(
                "当前本地质量分为 {}，低于 {}。",
                report.score, MIN_ACCEPTABLE_QUALITY
            ),
            suggestion: "先请求修订，优先处理开篇驱动力、章末钩子、段落重量、解释感和信息密度。"
                .to_string(),
        });
    }

    for warning in &report.warnings {
        let severity = match warning.title.as_str() {
            "正文含标题" => Some("major"),
            "叙事失焦" => Some("moderate"),
            "信息反转偏密" if report.score < 82 => Some("major"),
            _ => None,
        };
        if let Some(severity) = severity {
            blockers.push(GateBlocker {
                kind: "quality".to_string(),
                severity: severity.to_string(),
                title: warning.title.clone(),
                detail: warning.detail.clone(),
                suggestion: warning.suggestion.clone(),
            });
        }
    }
    blockers
}

fn task_adherence_blockers(chapter_title: &str, content: &str) -> Vec<GateBlocker> {
    let mut blockers = Vec::new();
    let checks = [
        (
            "赤髓",
            &["赤髓"][..],
            "章节标题/任务要求“赤髓”，但正文没有实际出现赤髓。",
            "修订时必须让赤髓成为本章关键筹码；如果要改名，先改章节任务而不是正文偷换资源。",
        ),
        (
            "大比",
            &["大比"][..],
            "章节标题/任务要求“大比”，但正文没有实质推进大比。",
            "补出大比前的时限、报名、查验、对手、资源分配或外部压力，避免只写泛泛准备。",
        ),
    ];

    for (trigger, required_terms, detail, suggestion) in checks {
        if chapter_title.contains(trigger)
            && !required_terms.iter().any(|term| content.contains(term))
        {
            blockers.push(GateBlocker {
                kind: "task_adherence".to_string(),
                severity: "major".to_string(),
                title: "章节任务未兑现".to_string(),
                detail: detail.to_string(),
                suggestion: suggestion.to_string(),
            });
        }
    }

    if (chapter_title.contains("炉底") || chapter_title.contains("第三层"))
        && !has_depth_scene_evidence(content)
    {
        blockers.push(GateBlocker {
            kind: "task_adherence".to_string(),
            severity: "major".to_string(),
            title: "章节任务未兑现".to_string(),
            detail: "章节标题/任务要求炉底/第三层，但正文缺少足够的下行探索场景证据。"
                .to_string(),
            suggestion: "把主动作收束回炉底第三层；正文应出现旧炉入口、下行路径、石室/地洞/骨堆/古炉等可感知场景，而不是只泛泛提到调查或准备。"
                .to_string(),
        });
    }

    if chapter_title.contains("赤髓")
        && chapter_title.contains("到手")
        && !contains_near(
            content,
            "赤髓",
            &["到手", "拿到", "得手", "夺得", "收入", "入手", "吞", "服下"],
        )
    {
        blockers.push(GateBlocker {
            kind: "task_adherence".to_string(),
            severity: "major".to_string(),
            title: "章节任务未兑现".to_string(),
            detail: "标题写“赤髓到手”，正文虽提到赤髓，但没有让赤髓实际到手或生效。".to_string(),
            suggestion:
                "要么修订正文让赤髓成为本章实际收益，要么先修改章节任务/标题，不能用青髓等其他资源替代。"
                    .to_string(),
        });
    }

    blockers
}

fn contains_near(text: &str, anchor: &str, terms: &[&str]) -> bool {
    text.split(['。', '！', '？', '\n', '；'])
        .any(|clause| clause.contains(anchor) && terms.iter().any(|term| clause.contains(term)))
}

fn has_depth_scene_evidence(content: &str) -> bool {
    let entry_hits = count_terms(
        content,
        &["七号旧炉", "炉门", "黑牌", "铁链", "火门", "裂缝"],
    );
    let depth_hits = count_terms(
        content,
        &[
            "石阶",
            "往下",
            "石室",
            "地洞",
            "骨堆",
            "铜柱",
            "古炉",
            "暗格",
            "无名炉",
            "炉腹",
        ],
    );
    let explicit_hits = count_terms(content, &["炉底", "第三层"]);

    explicit_hits > 0 || (entry_hits >= 2 && depth_hits >= 2) || depth_hits >= 4
}

fn count_terms(content: &str, terms: &[&str]) -> usize {
    terms.iter().filter(|term| content.contains(**term)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Artifact;

    fn artifact(content: &str) -> Artifact {
        Artifact {
            id: 1,
            project_id: 1,
            chapter_id: Some(1),
            stage: "draft".to_string(),
            title: "第 4 章 赤髓到手".to_string(),
            content: content.to_string(),
            version: 1,
            status: "pending_human_approval".to_string(),
            parent_artifact_id: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn blocks_task_drift_from_chapter_title() {
        let blockers =
            task_adherence_blockers("第 4 章 赤髓到手", &artifact("他拿到青髓。").content);
        assert!(blockers
            .iter()
            .any(|blocker| blocker.kind == "task_adherence"));
    }

    #[test]
    fn blocks_markdown_title_body() {
        let report = quality::analyze_artifact(&artifact("# 第4章\n\n他拿到赤髓。"));
        let blockers = quality_blockers(&report);
        assert!(blockers.iter().any(|blocker| blocker.title == "正文含标题"));
    }

    #[test]
    fn blocks_named_resource_not_actually_obtained() {
        let blockers = task_adherence_blockers(
            "第 4 章 赤髓到手",
            "赵执事说：“赤髓不行。”他把青髓铜瓶推进来。陆烬捡起铜瓶，将青髓膏吞尽。",
        );
        assert!(blockers
            .iter()
            .any(|blocker| blocker.detail.contains("没有让赤髓实际到手")));
    }

    #[test]
    fn accepts_named_resource_obtained_near_anchor() {
        let blockers = task_adherence_blockers(
            "第 4 章 赤髓到手",
            "赵执事把铜瓶推到门缝里。陆烬伸手拿到赤髓，先用封纹验过，再收入袖中。",
        );
        assert!(!blockers
            .iter()
            .any(|blocker| blocker.detail.contains("没有让赤髓实际到手")));
    }

    #[test]
    fn accepts_third_layer_scene_evidence_without_literal_title_terms() {
        let blockers = task_adherence_blockers(
            "第 3 章 炉底第三层",
            "陆烬把黑牌按进七号旧炉的凹纹，铁链从里面松开。他沿石阶往下，穿过半塌砖门，落进一间石室。三根铜柱围着青铜古炉，旁边地洞里铺满骨堆。",
        );
        assert!(!blockers
            .iter()
            .any(|blocker| blocker.detail.contains("下行探索场景证据")));
    }

    #[test]
    fn blocks_third_layer_title_without_depth_scene_evidence() {
        let blockers = task_adherence_blockers(
            "第 3 章 炉底第三层",
            "陆烬在草棚里整理黑牌，推演明日大比该如何藏住底牌。",
        );
        assert!(blockers
            .iter()
            .any(|blocker| blocker.detail.contains("下行探索场景证据")));
    }

    #[test]
    fn recommends_split_for_dense_reveal_with_major_signal() {
        let quality = QualityReport {
            artifact_id: 1,
            stage: "draft".to_string(),
            verdict: "needs_revision".to_string(),
            score: 76,
            summary: String::new(),
            metrics: vec![
                crate::models::QualityMetric {
                    label: "反转/真相信号".to_string(),
                    value: 24.0,
                    unit: "count".to_string(),
                    target: Some(4.0),
                },
                crate::models::QualityMetric {
                    label: "反转/真相密度".to_string(),
                    value: 6.4,
                    unit: "ratio".to_string(),
                    target: Some(1.6),
                },
            ],
            warnings: vec![],
        };
        let continuity = ContinuityReport {
            project_id: 1,
            chapter_titles: vec!["第 3 章".to_string()],
            verdict: "strong".to_string(),
            summary: String::new(),
            issues: vec![],
        };
        let blockers = vec![GateBlocker {
            kind: "quality".to_string(),
            severity: "major".to_string(),
            title: "信息反转偏密".to_string(),
            detail: String::new(),
            suggestion: String::new(),
        }];

        let (action, _) = recommended_action(false, &quality, &continuity, &blockers);
        assert_eq!(action, "split");
    }

    #[test]
    fn recommends_revise_for_ordinary_blockers() {
        let quality = QualityReport {
            artifact_id: 1,
            stage: "draft".to_string(),
            verdict: "needs_revision".to_string(),
            score: 66,
            summary: String::new(),
            metrics: vec![
                crate::models::QualityMetric {
                    label: "反转/真相信号".to_string(),
                    value: 6.0,
                    unit: "count".to_string(),
                    target: Some(4.0),
                },
                crate::models::QualityMetric {
                    label: "反转/真相密度".to_string(),
                    value: 1.4,
                    unit: "ratio".to_string(),
                    target: Some(1.6),
                },
            ],
            warnings: vec![],
        };
        let continuity = ContinuityReport {
            project_id: 1,
            chapter_titles: vec!["第 1 章".to_string()],
            verdict: "strong".to_string(),
            summary: String::new(),
            issues: vec![],
        };
        let blockers = vec![GateBlocker {
            kind: "quality".to_string(),
            severity: "major".to_string(),
            title: "质量分未达可用线".to_string(),
            detail: String::new(),
            suggestion: String::new(),
        }];

        let (action, _) = recommended_action(false, &quality, &continuity, &blockers);
        assert_eq!(action, "revise");
    }
}
