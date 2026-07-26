use crate::{db::AppState, error::AppResult};

pub fn export_markdown(state: &AppState, project_id: i64) -> AppResult<String> {
    let detail = state.get_detail(project_id)?;
    let mut output = format!(
        "# {}\n\n- 类型：{}\n- 预计总字数（仅供整体节奏规划）：{}\n- 状态：{}\n- 章节数：{}\n\n{}\n\n",
        detail.project.title,
        detail.project.genre,
        detail.project.target_words,
        detail.project.status,
        detail.chapters.len(),
        detail.project.premise
    );

    output.push_str("## 已批准资料\n\n");
    for stage in ["setting", "outline", "characters"] {
        if let Some(artifact) = state.approved_artifact(project_id, stage, None)? {
            output.push_str(&format!(
                "### {}（v{}）\n\n{}\n\n",
                super::stage_label(stage),
                artifact.version,
                artifact.content
            ));
        }
    }

    output.push_str("## 正文\n\n");
    for chapter in &detail.chapters {
        output.push_str(&format!(
            "### {}（第 {} 章）\n\n",
            chapter.title, chapter.chapter_no
        ));
        let chapter_body = chapter
            .current_artifact_id
            .and_then(|id| state.get_artifact(id).ok())
            .or_else(|| {
                state
                    .approved_artifact(project_id, "revision", Some(chapter.id))
                    .ok()
                    .flatten()
            })
            .or_else(|| {
                state
                    .approved_artifact(project_id, "draft", Some(chapter.id))
                    .ok()
                    .flatten()
            });
        if let Some(artifact) = chapter_body {
            output.push_str(&artifact.content);
            output.push_str("\n\n");
        } else {
            output.push_str("_未通过人工确认_\n\n");
        }
    }

    output.push_str("## 章节工作记录\n\n");
    for chapter in &detail.chapters {
        output.push_str(&format!("### {}\n\n", chapter.title));

        let chapter_approvals: Vec<_> = detail
            .approvals
            .iter()
            .filter(|approval| approval.chapter_id == Some(chapter.id))
            .collect();
        if !chapter_approvals.is_empty() {
            output.push_str("#### 人工确认\n\n");
            for approval in chapter_approvals {
                output.push_str(&format!(
                    "- {}：{}（artifact #{})\n",
                    approval.created_at,
                    if approval.note.trim().is_empty() {
                        format!("{} 已通过", super::stage_label(&approval.stage))
                    } else {
                        format!(
                            "{} 已通过，备注：{}",
                            super::stage_label(&approval.stage),
                            approval.note.trim()
                        )
                    },
                    approval.artifact_id
                ));
            }
            output.push('\n');
        }

        let chapter_artifacts: Vec<_> = detail
            .artifacts
            .iter()
            .filter(|artifact| artifact.chapter_id == Some(chapter.id))
            .filter(|artifact| {
                artifact.stage == "draft"
                    || artifact.stage == "review"
                    || artifact.stage == "revision"
            })
            .collect();

        if chapter_artifacts.is_empty() {
            output.push_str("_暂无章节工作记录_\n\n");
            continue;
        }

        for artifact in chapter_artifacts {
            output.push_str(&format!(
                "#### {} v{}\n\n- 创建时间：{}\n- 状态：{}\n",
                super::stage_label(&artifact.stage),
                artifact.version,
                artifact.created_at,
                artifact.status
            ));
            if let Some(parent_id) = artifact.parent_artifact_id {
                output.push_str(&format!("- 基于版本：artifact #{}\n", parent_id));
            }
            output.push('\n');
            output.push_str(&artifact.content);
            output.push_str("\n\n");
        }
    }

    let project_messages: Vec<_> = detail
        .messages
        .iter()
        .filter(|message| message.chapter_id.is_none())
        .collect();
    if !project_messages.is_empty() {
        output.push_str("## 全书协作记录\n\n");
        for message in project_messages {
            output.push_str(&format!(
                "- {} [{}] {}\n",
                message.created_at,
                super::export_role_label(&message.role),
                message.content
            ));
        }
        output.push('\n');
    }

    Ok(output)
}
