use std::collections::{HashMap, HashSet};

use chrono::Utc;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    ai,
    db::AppState,
    error::{AppError, AppResult},
    index_jobs,
    models::{
        AdoptionBatchResult, AdoptionProposal, Artifact, DecideAdoptionProposalsRequest,
        Foreshadowing, KnowledgeCard, SaveForeshadowing, SaveKnowledgeCard,
        UpdateAdoptionProposalRequest,
    },
};

const KNOWLEDGE_CARD: &str = "knowledge_card";
const FORESHADOWING: &str = "foreshadowing";
const PENDING: &str = "pending";

#[derive(Debug, Deserialize)]
struct ExtractedCandidate {
    #[serde(alias = "target")]
    target_kind: String,
    #[serde(default)]
    target_id: Option<i64>,
    #[serde(default = "default_operation")]
    operation: String,
    data: Value,
    evidence_quote: String,
}

fn default_operation() -> String {
    "create".to_string()
}

#[derive(Debug)]
struct ResolvedCandidate {
    target_kind: String,
    target_id: Option<i64>,
    operation: String,
    data: Value,
    evidence_quote: String,
    target_snapshot: Option<String>,
    validation_error: Option<String>,
}

pub async fn prepare_artifact_adoptions(
    state: &AppState,
    project_id: i64,
    artifact_id: i64,
) -> AppResult<Vec<AdoptionProposal>> {
    let artifact = approved_source_artifact(state, project_id, artifact_id)?;
    let agent = state.get_agent("adoption")?;
    let settings = agent.ai_settings();
    let api_key = state
        .get_api_key_for_base_url(&settings.base_url)?
        .ok_or_else(|| AppError::Validation("请先为当前供应商保存 API Key".to_string()))?;

    let current_library = json!({
        "knowledge_cards": state
            .list_knowledge_cards(project_id)?
            .into_iter()
            .filter(|item| item.status == "approved")
            .collect::<Vec<_>>(),
        "foreshadowings": state
            .list_foreshadowings(project_id)?
            .into_iter()
            .filter(|item| matches!(item.status.as_str(), "active" | "ready_for_payoff"))
            .collect::<Vec<_>>(),
        "chapters": state.list_chapters(project_id)?,
    });
    let user_prompt = format!(
        "# 当前正式资料\n{}\n\n# 来源产物\nartifact_id: {}\nstage: {}\ntitle: {}\n\n{}",
        serde_json::to_string_pretty(&current_library)?,
        artifact.id,
        artifact.stage,
        artifact.title,
        artifact.content
    );
    let output = ai::complete_chat(
        &settings,
        &api_key,
        &agent.system_prompt,
        &user_prompt,
        agent.temperature,
    )
    .await?;
    let extracted = parse_extracted_candidates(&output)?;
    replace_pending_proposals(state, &artifact, extracted)
}

pub fn list_adoption_proposals(
    state: &AppState,
    project_id: i64,
    artifact_id: Option<i64>,
) -> AppResult<Vec<AdoptionProposal>> {
    state.get_project(project_id)?;
    state.with_conn(|conn| {
        let sql = if artifact_id.is_some() {
            "SELECT id, project_id, source_artifact_id, target_kind, target_id, operation,
                    data_json, evidence_quote, target_snapshot, status, validation_error,
                    decision_note, created_at, updated_at
             FROM adoption_proposals
             WHERE project_id = ?1 AND source_artifact_id = ?2
             ORDER BY CASE status WHEN 'pending' THEN 0 WHEN 'stale' THEN 1 ELSE 2 END, id DESC"
        } else {
            "SELECT id, project_id, source_artifact_id, target_kind, target_id, operation,
                    data_json, evidence_quote, target_snapshot, status, validation_error,
                    decision_note, created_at, updated_at
             FROM adoption_proposals
             WHERE project_id = ?1
             ORDER BY CASE status WHEN 'pending' THEN 0 WHEN 'stale' THEN 1 ELSE 2 END, id DESC"
        };
        let mut stmt = conn.prepare(sql)?;
        let mut values = Vec::new();
        if let Some(artifact_id) = artifact_id {
            let rows = stmt.query_map(params![project_id, artifact_id], map_proposal)?;
            for row in rows {
                values.push(row?);
            }
        } else {
            let rows = stmt.query_map(params![project_id], map_proposal)?;
            for row in rows {
                values.push(row?);
            }
        }
        Ok(values)
    })
}

pub fn update_adoption_proposal(
    state: &AppState,
    input: UpdateAdoptionProposalRequest,
) -> AppResult<AdoptionProposal> {
    state.get_project(input.project_id)?;
    let existing = get_proposal(state, input.proposal_id)?;
    if existing.project_id != input.project_id {
        return Err(AppError::Validation("候选不属于当前项目".to_string()));
    }
    if existing.status != PENDING {
        return Err(AppError::Validation("只有待确认候选可以编辑".to_string()));
    }
    let artifact =
        approved_source_artifact(state, existing.project_id, existing.source_artifact_id)?;
    let extracted = ExtractedCandidate {
        target_kind: existing.target_kind,
        target_id: existing.target_id,
        operation: existing.operation,
        data: input.data,
        evidence_quote: existing.evidence_quote,
    };
    let resolved = resolve_candidate(state, &artifact, extracted)?;
    let now = now();
    state.with_conn(|conn| {
        conn.execute(
            "UPDATE adoption_proposals
             SET target_kind = ?1, target_id = ?2, operation = ?3, data_json = ?4,
                 target_snapshot = ?5, validation_error = ?6, updated_at = ?7
             WHERE id = ?8 AND status = 'pending'",
            params![
                resolved.target_kind,
                resolved.target_id,
                resolved.operation,
                serde_json::to_string(&resolved.data)?,
                resolved.target_snapshot,
                resolved.validation_error,
                now,
                input.proposal_id
            ],
        )?;
        query_proposal(conn, input.proposal_id)
    })
}

pub fn apply_adoption_proposals(
    state: &AppState,
    input: DecideAdoptionProposalsRequest,
) -> AppResult<AdoptionBatchResult> {
    let ids = normalized_ids(&input.proposal_ids)?;
    state.get_project(input.project_id)?;
    let now = now();

    let preflight_failure = state.with_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        let mut proposals = Vec::new();
        for id in &ids {
            let proposal = query_proposal(&tx, *id)?;
            if proposal.project_id != input.project_id || proposal.status != PENDING {
                return Err(AppError::Validation(format!(
                    "候选 #{id} 不属于当前项目或已被处理"
                )));
            }
            if let Err(error) = preflight_proposal(&tx, &proposal) {
                let message = error.to_string();
                if message.contains("目标资料已变化") || message.contains("对应的资料已经存在") {
                    tx.execute(
                        "UPDATE adoption_proposals
                         SET status = 'stale', validation_error = ?1, updated_at = ?2
                         WHERE id = ?3",
                        params![message, now, proposal.id],
                    )?;
                    tx.commit()?;
                    return Ok(Some(message));
                }
                return Err(error);
            }
            proposals.push(proposal);
        }

        for proposal in &proposals {
            apply_proposal(&tx, proposal, &now)?;
            tx.execute(
                "UPDATE adoption_proposals
                 SET status = 'applied', decision_note = ?1, validation_error = NULL, updated_at = ?2
                 WHERE id = ?3",
                params![input.note.trim(), now, proposal.id],
            )?;
        }
        tx.execute(
            "INSERT INTO messages (project_id, chapter_id, role, content, created_at)
             VALUES (?1, NULL, 'approval_note', ?2, ?3)",
            params![
                input.project_id,
                format!("人工采纳 {} 条资料变更。{}", proposals.len(), input.note.trim()),
                now
            ],
        )?;
        tx.commit()?;
        Ok(None)
    })?;

    if let Some(message) = preflight_failure {
        return Err(AppError::Validation(message));
    }

    state.mark_story_bible_changed(input.project_id)?;
    index_jobs::enqueue_project_search_job(state, input.project_id)?;

    Ok(AdoptionBatchResult {
        proposals: ids
            .into_iter()
            .map(|id| get_proposal(state, id))
            .collect::<AppResult<Vec<_>>>()?,
    })
}

pub fn reject_adoption_proposals(
    state: &AppState,
    input: DecideAdoptionProposalsRequest,
) -> AppResult<AdoptionBatchResult> {
    let ids = normalized_ids(&input.proposal_ids)?;
    let now = now();
    state.with_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        for id in &ids {
            let proposal = query_proposal(&tx, *id)?;
            if proposal.project_id != input.project_id || proposal.status != PENDING {
                return Err(AppError::Validation(format!(
                    "候选 #{id} 不属于当前项目或已被处理"
                )));
            }
        }
        for id in &ids {
            tx.execute(
                "UPDATE adoption_proposals
                 SET status = 'rejected', decision_note = ?1, updated_at = ?2
                 WHERE id = ?3",
                params![input.note.trim(), now, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    })?;
    Ok(AdoptionBatchResult {
        proposals: ids
            .into_iter()
            .map(|id| get_proposal(state, id))
            .collect::<AppResult<Vec<_>>>()?,
    })
}

pub fn save_human_knowledge_card(
    state: &AppState,
    input: SaveKnowledgeCard,
) -> AppResult<KnowledgeCard> {
    state.get_project(input.project_id)?;
    validate_optional_chapter(state, input.project_id, input.source_chapter_id)?;
    validate_optional_artifact(state, input.project_id, input.source_artifact_id)?;
    let data = normalize_data(
        KNOWLEDGE_CARD,
        json!({
            "category": input.category,
            "title": input.title,
            "content": input.content,
            "source_chapter_id": input.source_chapter_id,
        }),
    )?;
    let status = match input.status.trim() {
        "pending_human_approval" | "approved" | "archived" => input.status.trim(),
        _ => return Err(AppError::Validation("资料卡状态无效".to_string())),
    };
    let category = string_field(&data, "category")?;
    let title = string_field(&data, "title")?;
    let content = string_field(&data, "content")?;
    let source_chapter_id = optional_i64_field(&data, "source_chapter_id")?;
    let now = now();
    state.with_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        let was_canonical = if let Some(id) = input.id {
            tx.query_row(
                "SELECT status FROM knowledge_cards WHERE id = ?1 AND project_id = ?2",
                params![id, input.project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some_and(|value| value == "approved")
        } else {
            false
        };
        let id = if let Some(id) = input.id {
            let changed = tx.execute(
                "UPDATE knowledge_cards SET category = ?1, title = ?2, content = ?3,
                    status = ?4, source_artifact_id = ?5, source_chapter_id = ?6, updated_at = ?7
                 WHERE id = ?8 AND project_id = ?9",
                params![category, title, content, status, input.source_artifact_id, source_chapter_id, now, id, input.project_id],
            )?;
            if changed == 0 {
                return Err(AppError::Validation("资料卡不存在".to_string()));
            }
            id
        } else {
            tx.execute(
                "INSERT INTO knowledge_cards
                    (project_id, category, title, content, status, source_artifact_id, source_chapter_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![input.project_id, category, title, content, status, input.source_artifact_id, source_chapter_id, now],
            )?;
            tx.last_insert_rowid()
        };
        tx.execute(
            "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
            params![now, input.project_id],
        )?;
        if was_canonical || status == "approved" {
            crate::db::mark_story_bible_changed_tx(&tx, input.project_id, &now)?;
        }
        let card = query_knowledge_card(&tx, id)?;
        tx.commit()?;
        Ok(card)
    })
}

pub fn save_human_foreshadowing(
    state: &AppState,
    input: SaveForeshadowing,
) -> AppResult<Foreshadowing> {
    state.get_project(input.project_id)?;
    validate_optional_chapter(state, input.project_id, input.planted_chapter_id)?;
    validate_optional_chapter(state, input.project_id, input.planned_payoff_chapter_id)?;
    validate_optional_artifact(state, input.project_id, input.source_artifact_id)?;
    let data = normalize_data(
        FORESHADOWING,
        json!({
            "title": input.title,
            "content": input.content,
            "planted_chapter_id": input.planted_chapter_id,
            "planned_payoff_chapter_id": input.planned_payoff_chapter_id,
            "planned_payoff_note": input.planned_payoff_note,
        }),
    )?;
    let status = match input.status.trim() {
        "pending_human_approval" | "active" | "ready_for_payoff" | "resolved" | "archived" => {
            input.status.trim()
        }
        _ => return Err(AppError::Validation("伏笔状态无效".to_string())),
    };
    let title = string_field(&data, "title")?;
    let content = string_field(&data, "content")?;
    let planted = optional_i64_field(&data, "planted_chapter_id")?;
    let payoff = optional_i64_field(&data, "planned_payoff_chapter_id")?;
    let payoff_note = string_field(&data, "planned_payoff_note")?;
    let now = now();
    state.with_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        let was_canonical = if let Some(id) = input.id {
            tx.query_row(
                "SELECT status FROM foreshadowings WHERE id = ?1 AND project_id = ?2",
                params![id, input.project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some_and(|value| is_canonical_foreshadowing_status(&value))
        } else {
            false
        };
        let id = if let Some(id) = input.id {
            let changed = tx.execute(
                "UPDATE foreshadowings SET title = ?1, content = ?2, status = ?3,
                    planted_chapter_id = ?4, planned_payoff_chapter_id = ?5,
                    planned_payoff_note = ?6, source_artifact_id = ?7, updated_at = ?8
                 WHERE id = ?9 AND project_id = ?10",
                params![title, content, status, planted, payoff, payoff_note, input.source_artifact_id, now, id, input.project_id],
            )?;
            if changed == 0 {
                return Err(AppError::Validation("伏笔不存在".to_string()));
            }
            id
        } else {
            tx.execute(
                "INSERT INTO foreshadowings
                    (project_id, title, content, status, planted_chapter_id, planned_payoff_chapter_id,
                     planned_payoff_note, source_artifact_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                params![input.project_id, title, content, status, planted, payoff, payoff_note, input.source_artifact_id, now],
            )?;
            tx.last_insert_rowid()
        };
        tx.execute(
            "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
            params![now, input.project_id],
        )?;
        if was_canonical || is_canonical_foreshadowing_status(status) {
            crate::db::mark_story_bible_changed_tx(&tx, input.project_id, &now)?;
        }
        let foreshadowing = query_foreshadowing(&tx, id)?;
        tx.commit()?;
        Ok(foreshadowing)
    })
}

fn is_canonical_foreshadowing_status(status: &str) -> bool {
    matches!(status, "active" | "ready_for_payoff" | "resolved")
}

impl AppState {
    pub fn list_adoption_proposals(
        &self,
        project_id: i64,
        artifact_id: Option<i64>,
    ) -> AppResult<Vec<AdoptionProposal>> {
        list_adoption_proposals(self, project_id, artifact_id)
    }
}

fn replace_pending_proposals(
    state: &AppState,
    artifact: &Artifact,
    extracted: Vec<ExtractedCandidate>,
) -> AppResult<Vec<AdoptionProposal>> {
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();
    for candidate in extracted {
        let candidate = resolve_candidate(state, artifact, candidate)?;
        let identity = format!(
            "{}:{}:{}",
            candidate.target_kind,
            candidate.target_id.unwrap_or_default(),
            normalized_identity(&candidate.target_kind, &candidate.data)
        );
        if seen.insert(identity) {
            resolved.push(candidate);
        }
    }
    let now = now();
    state
        .with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE adoption_proposals
             SET status = 'rejected', decision_note = '被新一次整理替代', updated_at = ?1
             WHERE project_id = ?2 AND source_artifact_id = ?3 AND status = 'pending'",
                params![now, artifact.project_id, artifact.id],
            )?;
            let mut ids = Vec::new();
            for candidate in resolved {
                tx.execute(
                    "INSERT INTO adoption_proposals
                    (project_id, source_artifact_id, target_kind, target_id, operation, data_json,
                     evidence_quote, target_snapshot, status, validation_error, decision_note,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9, '', ?10, ?10)",
                    params![
                        artifact.project_id,
                        artifact.id,
                        candidate.target_kind,
                        candidate.target_id,
                        candidate.operation,
                        serde_json::to_string(&candidate.data)?,
                        candidate.evidence_quote,
                        candidate.target_snapshot,
                        candidate.validation_error,
                        now
                    ],
                )?;
                ids.push(tx.last_insert_rowid());
            }
            tx.commit()?;
            Ok(ids)
        })?
        .into_iter()
        .map(|id| get_proposal(state, id))
        .collect()
}

fn resolve_candidate(
    state: &AppState,
    artifact: &Artifact,
    candidate: ExtractedCandidate,
) -> AppResult<ResolvedCandidate> {
    let target_kind = normalize_target(&candidate.target_kind);
    let evidence_quote = candidate.evidence_quote.trim().to_string();
    let mut errors = Vec::new();
    if !matches!(target_kind.as_str(), KNOWLEDGE_CARD | FORESHADOWING) {
        errors.push(format!("未知采纳目标：{}", candidate.target_kind));
    }
    if evidence_quote.is_empty() || !artifact.content.contains(&evidence_quote) {
        errors.push("证据引文不是来源产物中的逐字连续文本".to_string());
    }
    if !matches!(candidate.operation.trim(), "create" | "update") {
        errors.push("operation 只能是 create 或 update".to_string());
    }
    let data = match normalize_data(&target_kind, candidate.data) {
        Ok(data) => data,
        Err(error) => {
            errors.push(error.to_string());
            Value::Object(Map::new())
        }
    };
    if let Err(error) = validate_candidate_foreign_keys(state, artifact.project_id, &data) {
        errors.push(error.to_string());
    }

    let mut target_id = candidate.target_id;
    if let Some(id) = target_id {
        if !target_belongs_to_project(state, artifact.project_id, &target_kind, id)? {
            errors.push("更新目标不存在或不属于当前项目".to_string());
            target_id = None;
        }
    }
    if target_id.is_none() && errors.is_empty() {
        target_id = find_identity_target(state, artifact.project_id, &target_kind, &data)?;
    }
    let operation = if target_id.is_some() {
        "update"
    } else {
        "create"
    }
    .to_string();
    let target_snapshot = target_id
        .map(|id| target_snapshot(state, artifact.project_id, &target_kind, id))
        .transpose()?;

    Ok(ResolvedCandidate {
        target_kind,
        target_id,
        operation,
        data,
        evidence_quote,
        target_snapshot,
        validation_error: (!errors.is_empty()).then(|| errors.join("；")),
    })
}

fn preflight_proposal(tx: &Transaction<'_>, proposal: &AdoptionProposal) -> AppResult<()> {
    if let Some(error) = &proposal.validation_error {
        return Err(AppError::Validation(format!(
            "候选 #{} 校验失败：{}",
            proposal.id, error
        )));
    }
    let source: (i64, String, String) = tx.query_row(
        "SELECT project_id, status, content FROM artifacts WHERE id = ?1",
        params![proposal.source_artifact_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if source.0 != proposal.project_id || source.1 != "approved" {
        return Err(AppError::Validation(format!(
            "候选 #{} 的来源产物不再是已批准状态",
            proposal.id
        )));
    }
    if proposal.evidence_quote.trim().is_empty() || !source.2.contains(&proposal.evidence_quote) {
        return Err(AppError::Validation(format!(
            "候选 #{} 的证据已失效",
            proposal.id
        )));
    }
    normalize_data(&proposal.target_kind, proposal.data.clone())?;
    validate_foreign_keys_tx(tx, proposal.project_id, &proposal.data)?;
    if let Some(target_id) = proposal.target_id {
        let current =
            target_snapshot_tx(tx, proposal.project_id, &proposal.target_kind, target_id)?;
        if Some(current) != proposal.target_snapshot {
            return Err(AppError::Validation(format!(
                "候选 #{} 的目标资料已变化，请重新整理",
                proposal.id
            )));
        }
    } else if find_identity_target_tx(
        tx,
        proposal.project_id,
        &proposal.target_kind,
        &proposal.data,
    )?
    .is_some()
    {
        return Err(AppError::Validation(format!(
            "候选 #{} 对应的资料已经存在，请重新整理",
            proposal.id
        )));
    }
    Ok(())
}

fn apply_proposal(tx: &Transaction<'_>, proposal: &AdoptionProposal, now: &str) -> AppResult<()> {
    match proposal.target_kind.as_str() {
        KNOWLEDGE_CARD => {
            let category = string_field(&proposal.data, "category")?;
            let title = string_field(&proposal.data, "title")?;
            let content = string_field(&proposal.data, "content")?;
            let chapter_id = optional_i64_field(&proposal.data, "source_chapter_id")?;
            if let Some(id) = proposal.target_id {
                tx.execute(
                    "UPDATE knowledge_cards SET category = ?1, title = ?2, content = ?3,
                        status = 'approved', source_artifact_id = ?4, source_chapter_id = ?5,
                        updated_at = ?6 WHERE id = ?7 AND project_id = ?8",
                    params![
                        category,
                        title,
                        content,
                        proposal.source_artifact_id,
                        chapter_id,
                        now,
                        id,
                        proposal.project_id
                    ],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO knowledge_cards
                        (project_id, category, title, content, status, source_artifact_id,
                         source_chapter_id, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 'approved', ?5, ?6, ?7, ?7)",
                    params![
                        proposal.project_id,
                        category,
                        title,
                        content,
                        proposal.source_artifact_id,
                        chapter_id,
                        now
                    ],
                )?;
            }
        }
        FORESHADOWING => {
            let title = string_field(&proposal.data, "title")?;
            let content = string_field(&proposal.data, "content")?;
            let planted = optional_i64_field(&proposal.data, "planted_chapter_id")?;
            let payoff = optional_i64_field(&proposal.data, "planned_payoff_chapter_id")?;
            let note = string_field(&proposal.data, "planned_payoff_note")?;
            if let Some(id) = proposal.target_id {
                tx.execute(
                    "UPDATE foreshadowings SET title = ?1, content = ?2, status = 'active',
                        planted_chapter_id = ?3, planned_payoff_chapter_id = ?4,
                        planned_payoff_note = ?5, source_artifact_id = ?6, updated_at = ?7
                     WHERE id = ?8 AND project_id = ?9",
                    params![
                        title,
                        content,
                        planted,
                        payoff,
                        note,
                        proposal.source_artifact_id,
                        now,
                        id,
                        proposal.project_id
                    ],
                )?;
            } else {
                tx.execute(
                    "INSERT INTO foreshadowings
                        (project_id, title, content, status, planted_chapter_id,
                         planned_payoff_chapter_id, planned_payoff_note, source_artifact_id,
                         created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?8, ?8)",
                    params![
                        proposal.project_id,
                        title,
                        content,
                        planted,
                        payoff,
                        note,
                        proposal.source_artifact_id,
                        now
                    ],
                )?;
            }
        }
        _ => return Err(AppError::Validation("未知采纳目标".to_string())),
    }
    Ok(())
}

fn normalize_data(target_kind: &str, data: Value) -> AppResult<Value> {
    let object = data
        .as_object()
        .ok_or_else(|| AppError::Validation("候选 data 必须是对象".to_string()))?;
    let aliases: HashMap<&str, &str> = match target_kind {
        KNOWLEDGE_CARD => HashMap::from([
            ("name", "title"),
            ("body", "content"),
            ("type", "category"),
            ("chapter_id", "source_chapter_id"),
        ]),
        FORESHADOWING => HashMap::from([
            ("name", "title"),
            ("body", "content"),
            ("chapter_id", "planted_chapter_id"),
            ("payoff_chapter_id", "planned_payoff_chapter_id"),
            ("payoff_note", "planned_payoff_note"),
        ]),
        _ => return Err(AppError::Validation(format!("未知采纳目标：{target_kind}"))),
    };
    let allowed: HashSet<&str> = match target_kind {
        KNOWLEDGE_CARD => ["category", "title", "content", "source_chapter_id"]
            .into_iter()
            .collect(),
        FORESHADOWING => [
            "title",
            "content",
            "planted_chapter_id",
            "planned_payoff_chapter_id",
            "planned_payoff_note",
        ]
        .into_iter()
        .collect(),
        _ => unreachable!(),
    };
    let mut normalized = Map::new();
    for (key, value) in object {
        let canonical = aliases.get(key.as_str()).copied().unwrap_or(key.as_str());
        if !allowed.contains(canonical) {
            return Err(AppError::Validation(format!(
                "字段 {key} 不允许写入 {target_kind}"
            )));
        }
        if normalized.contains_key(canonical) {
            return Err(AppError::Validation(format!("字段 {canonical} 重复")));
        }
        normalized.insert(canonical.to_string(), value.clone());
    }
    for field in ["title", "content"] {
        let value = normalized
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if value.is_empty() {
            return Err(AppError::Validation(format!("字段 {field} 不能为空")));
        }
        normalized.insert(field.to_string(), Value::String(value.to_string()));
    }
    match target_kind {
        KNOWLEDGE_CARD => {
            let category = normalized
                .get("category")
                .and_then(Value::as_str)
                .unwrap_or("");
            let category = normalize_category(category)
                .ok_or_else(|| AppError::Validation("资料分类无效".to_string()))?;
            normalized.insert("category".to_string(), Value::String(category.to_string()));
            normalize_optional_id(&mut normalized, "source_chapter_id")?;
        }
        FORESHADOWING => {
            normalize_optional_id(&mut normalized, "planted_chapter_id")?;
            normalize_optional_id(&mut normalized, "planned_payoff_chapter_id")?;
            let note = normalized
                .get("planned_payoff_note")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            normalized.insert("planned_payoff_note".to_string(), Value::String(note));
        }
        _ => unreachable!(),
    }
    Ok(Value::Object(normalized))
}

fn normalize_optional_id(data: &mut Map<String, Value>, field: &str) -> AppResult<()> {
    match data.get(field) {
        None | Some(Value::Null) => {
            data.insert(field.to_string(), Value::Null);
        }
        Some(Value::Number(number)) if number.as_i64().is_some_and(|id| id > 0) => {}
        _ => {
            return Err(AppError::Validation(format!(
                "字段 {field} 必须是有效章节 ID 或 null"
            )))
        }
    }
    Ok(())
}

fn normalize_category(category: &str) -> Option<&'static str> {
    match category.trim().to_lowercase().as_str() {
        "world" | "世界" | "世界观" => Some("world"),
        "cultivation" | "修行" | "修行体系" => Some("cultivation"),
        "map" | "地图" | "地点" => Some("map"),
        "faction" | "势力" | "组织" => Some("faction"),
        "taboo" | "禁忌" | "边界" => Some("taboo"),
        "item" | "物件" | "道具" => Some("item"),
        "outline" | "大纲" => Some("outline"),
        "character" | "角色" | "人物" => Some("character"),
        "other" | "其他" => Some("other"),
        _ => None,
    }
}

fn normalize_target(target: &str) -> String {
    match target.trim().to_lowercase().as_str() {
        "knowledge_card" | "knowledge" | "card" | "资料卡" | "角色卡" => {
            KNOWLEDGE_CARD.to_string()
        }
        "foreshadowing" | "foreshadow" | "伏笔" => FORESHADOWING.to_string(),
        other => other.to_string(),
    }
}

fn approved_source_artifact(
    state: &AppState,
    project_id: i64,
    artifact_id: i64,
) -> AppResult<Artifact> {
    let artifact = state.get_artifact(artifact_id)?;
    if artifact.project_id != project_id {
        return Err(AppError::Validation("来源产物不属于当前项目".to_string()));
    }
    if artifact.status != "approved" {
        return Err(AppError::Validation(
            "请先人工通过该产物，再整理资料".to_string(),
        ));
    }
    Ok(artifact)
}

fn parse_extracted_candidates(raw: &str) -> AppResult<Vec<ExtractedCandidate>> {
    let trimmed = raw.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let candidate = without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim();
    if let Ok(items) = serde_json::from_str(candidate) {
        return Ok(items);
    }
    let start = candidate
        .find('[')
        .ok_or_else(|| AppError::Validation("资料整理 Agent 未返回 JSON 数组".to_string()))?;
    let end = candidate
        .rfind(']')
        .ok_or_else(|| AppError::Validation("资料整理 Agent 返回的 JSON 不完整".to_string()))?;
    serde_json::from_str(&candidate[start..=end])
        .map_err(|error| AppError::Validation(format!("无法解析资料候选：{error}")))
}

fn normalized_ids(ids: &[i64]) -> AppResult<Vec<i64>> {
    let mut seen = HashSet::new();
    let ids: Vec<_> = ids
        .iter()
        .copied()
        .filter(|id| *id > 0 && seen.insert(*id))
        .collect();
    if ids.is_empty() {
        return Err(AppError::Validation("请至少选择一条候选".to_string()));
    }
    Ok(ids)
}

fn validate_candidate_foreign_keys(
    state: &AppState,
    project_id: i64,
    data: &Value,
) -> AppResult<()> {
    for field in [
        "source_chapter_id",
        "planted_chapter_id",
        "planned_payoff_chapter_id",
    ] {
        if let Some(id) = optional_i64_field(data, field)? {
            validate_optional_chapter(state, project_id, Some(id))?;
        }
    }
    Ok(())
}

fn validate_foreign_keys_tx(tx: &Transaction<'_>, project_id: i64, data: &Value) -> AppResult<()> {
    for field in [
        "source_chapter_id",
        "planted_chapter_id",
        "planned_payoff_chapter_id",
    ] {
        if let Some(id) = optional_i64_field(data, field)? {
            let exists = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM chapters WHERE id = ?1 AND project_id = ?2)",
                params![id, project_id],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(AppError::Validation(format!("章节 #{id} 不属于当前项目")));
            }
        }
    }
    Ok(())
}

fn validate_optional_chapter(
    state: &AppState,
    project_id: i64,
    chapter_id: Option<i64>,
) -> AppResult<()> {
    if let Some(chapter_id) = chapter_id {
        match state.ensure_chapter(project_id, Some(chapter_id)) {
            Ok(Some(_)) => {}
            // 章节不在本项目（缺失或归属其他项目）统一为项目归属错误；
            // 真实的 DB 错误（Database/Io 等）仍向上传播，不被伪装成业务错误
            Ok(None) | Err(AppError::Validation(_)) => {
                return Err(AppError::Validation(format!(
                    "章节 #{chapter_id} 不属于当前项目"
                )));
            }
            Err(other) => return Err(other),
        }
    }
    Ok(())
}

fn validate_optional_artifact(
    state: &AppState,
    project_id: i64,
    artifact_id: Option<i64>,
) -> AppResult<()> {
    if let Some(artifact_id) = artifact_id {
        if state.get_artifact(artifact_id)?.project_id != project_id {
            return Err(AppError::Validation("来源产物不属于当前项目".to_string()));
        }
    }
    Ok(())
}

fn find_identity_target(
    state: &AppState,
    project_id: i64,
    target_kind: &str,
    data: &Value,
) -> AppResult<Option<i64>> {
    state.with_conn(|conn| find_identity_target_conn(conn, project_id, target_kind, data))
}

fn find_identity_target_tx(
    tx: &Transaction<'_>,
    project_id: i64,
    target_kind: &str,
    data: &Value,
) -> AppResult<Option<i64>> {
    find_identity_target_conn(tx, project_id, target_kind, data)
}

fn find_identity_target_conn(
    conn: &rusqlite::Connection,
    project_id: i64,
    target_kind: &str,
    data: &Value,
) -> AppResult<Option<i64>> {
    let title = string_field(data, "title")?;
    match target_kind {
        KNOWLEDGE_CARD => {
            let category = string_field(data, "category")?;
            conn.query_row(
                "SELECT id FROM knowledge_cards WHERE project_id = ?1 AND lower(trim(category)) = lower(trim(?2)) AND lower(trim(title)) = lower(trim(?3)) ORDER BY id LIMIT 1",
                params![project_id, category, title],
                |row| row.get(0),
            ).optional().map_err(AppError::from)
        }
        FORESHADOWING => conn.query_row(
            "SELECT id FROM foreshadowings WHERE project_id = ?1 AND lower(trim(title)) = lower(trim(?2)) ORDER BY id LIMIT 1",
            params![project_id, title],
            |row| row.get(0),
        ).optional().map_err(AppError::from),
        _ => Ok(None),
    }
}

fn target_belongs_to_project(
    state: &AppState,
    project_id: i64,
    target_kind: &str,
    target_id: i64,
) -> AppResult<bool> {
    let table = match target_kind {
        KNOWLEDGE_CARD => "knowledge_cards",
        FORESHADOWING => "foreshadowings",
        _ => return Ok(false),
    };
    state.with_conn(|conn| {
        conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1 AND project_id = ?2)"),
            params![target_id, project_id],
            |row| row.get(0),
        )
        .map_err(AppError::from)
    })
}

fn target_snapshot(
    state: &AppState,
    project_id: i64,
    target_kind: &str,
    target_id: i64,
) -> AppResult<String> {
    state.with_conn(|conn| target_snapshot_conn(conn, project_id, target_kind, target_id))
}

fn target_snapshot_tx(
    tx: &Transaction<'_>,
    project_id: i64,
    target_kind: &str,
    target_id: i64,
) -> AppResult<String> {
    target_snapshot_conn(tx, project_id, target_kind, target_id)
}

fn target_snapshot_conn(
    conn: &rusqlite::Connection,
    project_id: i64,
    target_kind: &str,
    target_id: i64,
) -> AppResult<String> {
    let table = match target_kind {
        KNOWLEDGE_CARD => "knowledge_cards",
        FORESHADOWING => "foreshadowings",
        _ => return Err(AppError::Validation("未知采纳目标".to_string())),
    };
    conn.query_row(
        &format!("SELECT updated_at FROM {table} WHERE id = ?1 AND project_id = ?2"),
        params![target_id, project_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::Validation("采纳目标不存在".to_string()))
}

fn normalized_identity(target_kind: &str, data: &Value) -> String {
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if target_kind == KNOWLEDGE_CARD {
        let category = data
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        format!("{category}:{title}")
    } else {
        title
    }
}

fn get_proposal(state: &AppState, id: i64) -> AppResult<AdoptionProposal> {
    state.with_conn(|conn| query_proposal(conn, id))
}

fn query_proposal(conn: &rusqlite::Connection, id: i64) -> AppResult<AdoptionProposal> {
    conn.query_row(
        "SELECT id, project_id, source_artifact_id, target_kind, target_id, operation,
                data_json, evidence_quote, target_snapshot, status, validation_error,
                decision_note, created_at, updated_at
         FROM adoption_proposals WHERE id = ?1",
        params![id],
        map_proposal,
    )
    .optional()?
    .ok_or_else(|| AppError::Validation("采纳候选不存在".to_string()))
}

fn map_proposal(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdoptionProposal> {
    let data_json: String = row.get(6)?;
    let data = serde_json::from_str(&data_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(AdoptionProposal {
        id: row.get(0)?,
        project_id: row.get(1)?,
        source_artifact_id: row.get(2)?,
        target_kind: row.get(3)?,
        target_id: row.get(4)?,
        operation: row.get(5)?,
        data,
        evidence_quote: row.get(7)?,
        target_snapshot: row.get(8)?,
        status: row.get(9)?,
        validation_error: row.get(10)?,
        decision_note: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn query_knowledge_card(conn: &rusqlite::Connection, id: i64) -> AppResult<KnowledgeCard> {
    conn.query_row(
        "SELECT id, project_id, category, title, content, status, source_artifact_id, source_chapter_id, created_at, updated_at FROM knowledge_cards WHERE id = ?1",
        params![id],
        |row| Ok(KnowledgeCard { id: row.get(0)?, project_id: row.get(1)?, category: row.get(2)?, title: row.get(3)?, content: row.get(4)?, status: row.get(5)?, source_artifact_id: row.get(6)?, source_chapter_id: row.get(7)?, created_at: row.get(8)?, updated_at: row.get(9)? }),
    ).map_err(AppError::from)
}

fn query_foreshadowing(conn: &rusqlite::Connection, id: i64) -> AppResult<Foreshadowing> {
    conn.query_row(
        "SELECT id, project_id, title, content, status, planted_chapter_id, planned_payoff_chapter_id, planned_payoff_note, source_artifact_id, created_at, updated_at FROM foreshadowings WHERE id = ?1",
        params![id],
        |row| Ok(Foreshadowing { id: row.get(0)?, project_id: row.get(1)?, title: row.get(2)?, content: row.get(3)?, status: row.get(4)?, planted_chapter_id: row.get(5)?, planned_payoff_chapter_id: row.get(6)?, planned_payoff_note: row.get(7)?, source_artifact_id: row.get(8)?, created_at: row.get(9)?, updated_at: row.get(10)? }),
    ).map_err(AppError::from)
}

fn string_field<'a>(data: &'a Value, field: &str) -> AppResult<&'a str> {
    data.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Validation(format!("字段 {field} 必须是文本")))
}

fn optional_i64_field(data: &Value, field: &str) -> AppResult<Option<i64>> {
    match data.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .filter(|id| *id > 0)
            .map(Some)
            .ok_or_else(|| AppError::Validation(format!("字段 {field} 必须是有效 ID 或 null"))),
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewProject, SaveKnowledgeCard};
    use std::ops::Deref;

    struct TestState {
        state: AppState,
        _file: tempfile::NamedTempFile,
    }

    impl Deref for TestState {
        type Target = AppState;

        fn deref(&self) -> &Self::Target {
            &self.state
        }
    }

    fn test_state() -> TestState {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let state = AppState::from_path(path).unwrap();
        TestState { state, _file: file }
    }

    fn approved_artifact(state: &AppState, content: &str) -> Artifact {
        let project = state
            .create_project(NewProject {
                title: "烬骨长生".to_string(),
                genre: "男频修仙".to_string(),
                target_words: 1_000_000,
                premise: "矿奴求生".to_string(),
            })
            .unwrap();
        let artifact = state
            .insert_artifact(project.id, None, "setting", "设定", content, None)
            .unwrap();
        state
            .approve_stage(project.id, "setting", artifact.id, "通过")
            .unwrap();
        state.get_artifact(artifact.id).unwrap()
    }

    #[test]
    fn registry_normalizes_aliases_and_categories() {
        let normalized = normalize_data(
            KNOWLEDGE_CARD,
            json!({"type": "角色", "name": "宁烬", "body": "矿奴少年。"}),
        )
        .unwrap();
        assert_eq!(normalized["category"], "character");
        assert_eq!(normalized["title"], "宁烬");
        assert_eq!(normalized["source_chapter_id"], Value::Null);
    }

    #[test]
    fn registry_rejects_unknown_fields() {
        let error = normalize_data(
            KNOWLEDGE_CARD,
            json!({"category": "world", "title": "矿场", "content": "事实", "status": "approved"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("status"));
    }

    #[test]
    fn extraction_parser_accepts_fenced_json() {
        let parsed = parse_extracted_candidates(
            "```json\n[{\"target_kind\":\"knowledge_card\",\"data\":{\"category\":\"world\",\"title\":\"矿场\",\"content\":\"事实\"},\"evidence_quote\":\"事实\"}]\n```",
        ).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].operation, "create");
    }

    #[test]
    fn pending_candidate_does_not_enter_canonical_library() {
        let state = test_state();
        let artifact = approved_artifact(&state, "宁烬来自黑石矿场。");
        let proposals = replace_pending_proposals(
            &state,
            &artifact,
            vec![ExtractedCandidate {
                target_kind: KNOWLEDGE_CARD.to_string(),
                target_id: None,
                operation: "create".to_string(),
                data: json!({"category": "character", "title": "宁烬", "content": "来自黑石矿场"}),
                evidence_quote: "宁烬来自黑石矿场".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(proposals.len(), 1);
        assert!(state
            .list_knowledge_cards(artifact.project_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn apply_creates_once_and_repeat_extraction_becomes_update() {
        let state = test_state();
        let artifact = approved_artifact(&state, "宁烬来自黑石矿场。");
        let extracted = || ExtractedCandidate {
            target_kind: KNOWLEDGE_CARD.to_string(),
            target_id: None,
            operation: "create".to_string(),
            data: json!({"category": "character", "title": "宁烬", "content": "来自黑石矿场"}),
            evidence_quote: "宁烬来自黑石矿场".to_string(),
        };
        let first = replace_pending_proposals(&state, &artifact, vec![extracted()]).unwrap();
        apply_adoption_proposals(
            &state,
            DecideAdoptionProposalsRequest {
                project_id: artifact.project_id,
                proposal_ids: vec![first[0].id],
                note: "确认".to_string(),
            },
        )
        .unwrap();
        let second = replace_pending_proposals(&state, &artifact, vec![extracted()]).unwrap();
        assert_eq!(second[0].operation, "update");
        assert!(second[0].target_id.is_some());
        apply_adoption_proposals(
            &state,
            DecideAdoptionProposalsRequest {
                project_id: artifact.project_id,
                proposal_ids: vec![second[0].id],
                note: "再次确认".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            state
                .list_knowledge_cards(artifact.project_id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn stale_target_blocks_entire_batch() {
        let state = test_state();
        let artifact = approved_artifact(&state, "宁烬来自黑石矿场。矿场位于北荒。");
        let existing = state
            .save_knowledge_card(SaveKnowledgeCard {
                id: None,
                project_id: artifact.project_id,
                category: "character".to_string(),
                title: "宁烬".to_string(),
                content: "矿奴".to_string(),
                status: "approved".to_string(),
                source_artifact_id: None,
                source_chapter_id: None,
            })
            .unwrap();
        let proposals = replace_pending_proposals(&state, &artifact, vec![
            ExtractedCandidate {
                target_kind: KNOWLEDGE_CARD.to_string(),
                target_id: Some(existing.id),
                operation: "update".to_string(),
                data: json!({"category": "character", "title": "宁烬", "content": "来自黑石矿场"}),
                evidence_quote: "宁烬来自黑石矿场".to_string(),
            },
            ExtractedCandidate {
                target_kind: KNOWLEDGE_CARD.to_string(),
                target_id: None,
                operation: "create".to_string(),
                data: json!({"category": "map", "title": "北荒", "content": "矿场位于北荒"}),
                evidence_quote: "矿场位于北荒".to_string(),
            },
        ]).unwrap();
        state
            .save_knowledge_card(SaveKnowledgeCard {
                id: Some(existing.id),
                project_id: artifact.project_id,
                category: "character".to_string(),
                title: "宁烬".to_string(),
                content: "人工刚刚修改".to_string(),
                status: "approved".to_string(),
                source_artifact_id: None,
                source_chapter_id: None,
            })
            .unwrap();
        let result = apply_adoption_proposals(
            &state,
            DecideAdoptionProposalsRequest {
                project_id: artifact.project_id,
                proposal_ids: proposals.iter().map(|item| item.id).collect(),
                note: "批量".to_string(),
            },
        );
        assert!(result.is_err());
        let cards = state.list_knowledge_cards(artifact.project_id).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].content, "人工刚刚修改");
        assert_eq!(
            get_proposal(&state, proposals[0].id).unwrap().status,
            "stale"
        );
    }

    #[test]
    fn cross_project_chapter_is_rejected() {
        let state = test_state();
        let artifact = approved_artifact(&state, "北荒矿场已经封闭。");
        let other = state
            .create_project(NewProject {
                title: "其他书".to_string(),
                genre: "悬疑".to_string(),
                target_words: 100_000,
                premise: "测试".to_string(),
            })
            .unwrap();
        let chapter = state
            .create_chapter(crate::models::NewChapter {
                project_id: other.id,
                title: None,
            })
            .unwrap();
        let proposals = replace_pending_proposals(&state, &artifact, vec![ExtractedCandidate {
            target_kind: KNOWLEDGE_CARD.to_string(),
            target_id: None,
            operation: "create".to_string(),
            data: json!({"category": "map", "title": "北荒", "content": "矿场已经封闭", "source_chapter_id": chapter.id}),
            evidence_quote: "北荒矿场已经封闭".to_string(),
        }]).unwrap();
        assert!(proposals[0]
            .validation_error
            .as_deref()
            .unwrap_or("")
            .contains("不属于当前项目"));
    }
}
