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
            suggestion: "先请求修订，优先处理章节功能、结尾落点、段落重量、解释感和信息密度。"
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
    fn blocks_markdown_title_body() {
        let report = quality::analyze_artifact(&artifact("# 第4章\n\n他拿到赤髓。"));
        let blockers = quality_blockers(&report);
        assert!(blockers.iter().any(|blocker| blocker.title == "正文含标题"));
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
