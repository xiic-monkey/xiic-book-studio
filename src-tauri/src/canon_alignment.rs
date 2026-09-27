//! 章节转正后的资料对齐：对照新正文，把该章来源的资料分为
//! 需要更新（重置待确认）、已失效（自动归档，可人工恢复）、正文新增可沉淀
//! 三类，另提取写作笔记（语气、母题、编号规律等写作约束）。
//!
//! 对齐是提议而非覆盖：除归档外全部以 pending_human_approval 落地，
//! 人工在资料库确认。失败写入 workflow_runs（stage = canon_align），不做静默降级。

use std::time::Instant;

use serde::Deserialize;

use crate::{
    ai,
    db::AppState,
    error::{AppError, AppResult},
    models::CanonEntry,
};

const BUILD_STAGE: &str = "canon_align";

#[derive(Deserialize)]
struct AlignmentUpdate {
    entry_id: i64,
    new_content: String,
}

#[derive(Deserialize)]
struct AlignmentAddition {
    category: String,
    title: String,
    content: String,
}

#[derive(Deserialize)]
struct AlignmentWritingNote {
    title: String,
    content: String,
}

#[derive(Deserialize)]
struct AlignmentReport {
    #[serde(default)]
    updates: Vec<AlignmentUpdate>,
    #[serde(default)]
    archive_ids: Vec<i64>,
    #[serde(default)]
    additions: Vec<AlignmentAddition>,
    #[serde(default)]
    writing_notes: Vec<AlignmentWritingNote>,
}

fn deterministic_settings(mut settings: crate::models::AiSettings) -> crate::models::AiSettings {
    // 对齐是机器可读的资料整理步骤，关闭深度思考换取稳定的 JSON 输出与低延迟。
    settings.thinking_enabled = false;
    settings
}

/// 章节转正后调用：对照新正文对齐该章来源的资料条目。
pub async fn align_chapter_after_approval(
    state: &AppState,
    project_id: i64,
    chapter_id: i64,
) -> AppResult<()> {
    let entries = state.list_canon_entries_for_chapter(project_id, chapter_id)?;
    if entries.is_empty() {
        return Ok(());
    }
    let Some(source) = state.latest_approved_chapter_body(project_id, chapter_id)? else {
        return Ok(());
    };
    let agent = state.get_agent("canon_align")?;
    let settings = deterministic_settings(agent.ai_settings());
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先为资料对齐 Agent 配置 API Key".to_string()))?;

    let prompt = build_alignment_prompt(&source.content, &entries);
    let started = Instant::now();
    let run = state.insert_workflow_run(
        project_id,
        Some(chapter_id),
        BUILD_STAGE,
        &prompt,
        "",
        "running",
        None,
        0,
    )?;

    let raw = match ai::complete_json_chat(&settings, &api_key, &agent.system_prompt, &prompt, 0.0)
        .await
    {
        Ok(raw) => raw,
        Err(error) => {
            state.update_workflow_run(
                run.id,
                "",
                "failed",
                Some(&error.to_string()),
                started.elapsed().as_millis() as i64,
            )?;
            return Err(error);
        }
    };

    let known_ids: Vec<i64> = entries.iter().map(|entry| entry.id).collect();
    let report = match parse_alignment_report(&raw, &known_ids) {
        Ok(report) => report,
        Err(error) => {
            state.update_workflow_run(
                run.id,
                "",
                "failed",
                Some(&error.to_string()),
                started.elapsed().as_millis() as i64,
            )?;
            return Err(error);
        }
    };

    let additions: Vec<(String, String, String)> = report
        .additions
        .iter()
        .map(|addition| (addition.category.clone(), addition.title.clone(), addition.content.clone()))
        .chain(report.writing_notes.iter().map(|note| {
            (
                "rule".to_string(),
                if note.title.starts_with("写作笔记") {
                    note.title.clone()
                } else {
                    format!("写作笔记：{}", note.title)
                },
                note.content.clone(),
            )
        }))
        .collect();
    let (updated, archived, added) = state.apply_canon_alignment(
        project_id,
        chapter_id,
        source.id,
        &report.updates.iter().map(|u| (u.entry_id, u.new_content.clone())).collect::<Vec<_>>(),
        &report.archive_ids,
        &additions,
    )?;
    crate::index_jobs::enqueue_project_search_job(state, project_id)?;
    state.update_workflow_run(
        run.id,
        &format!(
            "更新 {} 条、归档 {} 条、新增 {} 条（含写作笔记 {} 条）。",
            updated, archived, added, report.writing_notes.len()
        ),
        "success",
        None,
        started.elapsed().as_millis() as i64,
    )?;
    Ok(())
}

fn build_alignment_prompt(body: &str, entries: &[CanonEntry]) -> String {
    let mut entry_lines = String::new();
    for entry in entries {
        entry_lines.push_str(&format!(
            "\n[id={}] [category={}] [status={}] {}\n---\n{}\n",
            entry.id, entry.category, entry.status, entry.title, entry.content
        ));
    }
    format!(
        "# 任务\n本章正文刚刚定稿（转正）。请对照正文逐条审视下方资料条目：它们产生于写作之前，可能与定稿矛盾、重复或已过时。\n\n# 定稿正文\n{body}\n\n# 待审视资料条目\n{entry_lines}\n\n# 输出协议\n只输出 JSON 对象：\n{{\"updates\":[{{\"entry_id\":1,\"reason\":\"与正文冲突的原因\",\"new_content\":\"修订后的完整内容\"}}],\"archive_ids\":[2],\"additions\":[{{\"category\":\"world\",\"title\":\"...\",\"content\":\"...\"}}],\"writing_notes\":[{{\"title\":\"某角色的说话方式\",\"content\":\"从正文提炼的写作约束\"}}]}}\n\n规则：\n1. updates：条目内容与正文事实冲突或已过时；new_content 必须引用正文的句子逐字摘自正文；修订后 status 会重置为待人工确认。\n2. archive_ids：条目描述与正文明确矛盾且无法通过更新挽救，或与另一条目完全重复。归档不删除，可人工恢复。\n3. additions：正文新出现的、值得长期沉淀的设定。category 只能是 world/cultivation/map/faction/taboo/item/rule/character/outline。\n4. writing_notes：从正文提炼的写作约束——角色说话方式、重复出现的母题、编号/命名规律、叙事纪律。title 写成「写作笔记：XX 的说话方式」这类形式。\n5. 与正文一致且仍有价值的条目不要动。没有把握就不要输出。禁止输出 Markdown。"
    )
}

fn parse_alignment_report(raw: &str, known_ids: &[i64]) -> AppResult<AlignmentReport> {
    let trimmed = raw.trim();
    let json_text = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        let start = trimmed.find('{').ok_or_else(|| {
            AppError::Validation("资料对齐响应缺少 JSON 对象".to_string())
        })?;
        let end = trimmed.rfind('}').ok_or_else(|| {
            AppError::Validation("资料对齐响应缺少 JSON 对象".to_string())
        })?;
        trimmed[start..=end].to_string()
    };
    let value: serde_json::Value = serde_json::from_str(&json_text)
        .map_err(|error| AppError::Validation(format!("资料对齐响应不是有效 JSON：{error}")))?;
    let mut report: AlignmentReport = serde_json::from_value(value)
        .map_err(|error| AppError::Validation(format!("资料对齐响应字段不符合协议：{error}")))?;
    let known = known_ids;
    report.updates.retain(|update| known.contains(&update.entry_id));
    report.archive_ids.retain(|id| known.contains(id));
    report.updates.truncate(12);
    report.archive_ids.truncate(12);
    report.additions.truncate(8);
    report.writing_notes.truncate(6);
    Ok(report)
}
