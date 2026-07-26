use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Setting,
    Outline,
    Characters,
    Draft,
    Review,
    Revision,
}

impl Stage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Stage::Setting => "setting",
            Stage::Outline => "outline",
            Stage::Characters => "characters",
            Stage::Draft => "draft",
            Stage::Review => "review",
            Stage::Revision => "revision",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Stage::Setting => "设定资料",
            Stage::Outline => "阶段大纲",
            Stage::Characters => "角色资料",
            Stage::Draft => "写作 Agent",
            Stage::Review => "试读 Agent",
            Stage::Revision => "修订 Agent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoryArchitectMode {
    Initialize,
    RefineCanon,
    PlanCurrentArc,
    ExtendNextArc,
    DesignCharacters,
}

impl StoryArchitectMode {
    pub fn artifact_stage(&self) -> Stage {
        match self {
            Self::Initialize | Self::RefineCanon => Stage::Setting,
            Self::PlanCurrentArc | Self::ExtendNextArc => Stage::Outline,
            Self::DesignCharacters => Stage::Characters,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Initialize => "初始化创作基准",
            Self::RefineCanon => "补充设定",
            Self::PlanCurrentArc => "细化当前阶段",
            Self::ExtendNextArc => "扩展下一阶段",
            Self::DesignCharacters => "补充角色",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub title: String,
    pub genre: String,
    pub target_words: i64,
    pub premise: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProject {
    pub title: String,
    pub genre: String,
    pub target_words: i64,
    pub premise: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChapter {
    pub project_id: i64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectUpdate {
    pub id: i64,
    pub title: String,
    pub genre: String,
    pub target_words: i64,
    pub premise: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterUpdate {
    pub id: i64,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: i64,
    pub stage: String,
    pub name: String,
    pub role: String,
    pub system_prompt: String,
    pub temperature: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreAgentProfile {
    pub agent_key: String,
    pub name: String,
    pub role: String,
    pub system_prompt: String,
    pub primary_skill_key: String,
    pub allowed_skill_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub id: i64,
    pub project_id: i64,
    pub chapter_no: i64,
    pub title: String,
    pub status: String,
    pub current_artifact_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub title: String,
    pub content: String,
    pub version: i64,
    pub status: String,
    pub parent_artifact_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub artifact_id: i64,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub input: String,
    pub output: String,
    pub status: String,
    pub error: Option<String>,
    pub elapsed_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterMemoryRecord {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: i64,
    pub source_artifact_id: i64,
    pub source_text_hash: String,
    pub normalization_version: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryThread {
    pub id: i64,
    pub project_id: i64,
    pub thread_key: String,
    pub label: String,
    pub kind: String,
    pub status: String,
    pub current_cost: Option<String>,
    pub last_seen_chapter_no: Option<i64>,
    pub last_artifact_id: Option<i64>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCard {
    pub id: i64,
    pub project_id: i64,
    pub category: String,
    pub title: String,
    pub content: String,
    pub status: String,
    pub source_artifact_id: Option<i64>,
    pub source_chapter_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveKnowledgeCard {
    pub id: Option<i64>,
    pub project_id: i64,
    pub category: String,
    pub title: String,
    pub content: String,
    pub status: String,
    pub source_artifact_id: Option<i64>,
    pub source_chapter_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Foreshadowing {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub content: String,
    pub status: String,
    pub planted_chapter_id: Option<i64>,
    pub planned_payoff_chapter_id: Option<i64>,
    pub planned_payoff_note: String,
    pub source_artifact_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// A named story-world entity. This is an identity registry, not a mutable
/// character sheet: changing facts are stored separately with source evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryEntity {
    pub id: i64,
    pub project_id: i64,
    pub kind: String,
    pub name: String,
    pub status: String,
    pub first_seen_chapter_id: Option<i64>,
    pub source_artifact_id: Option<i64>,
    pub source_quote: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryEvent {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub story_time: String,
    pub summary: String,
    pub narrative_chapter_id: Option<i64>,
    pub source_artifact_id: i64,
    pub source_quote: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryEventParticipant {
    pub event_id: i64,
    pub entity_id: i64,
    pub entity_name: String,
    pub role: String,
}

/// An immutable, evidence-backed assertion extracted from approved formal text.
/// `status` and `supersedes_fact_id` make a later change traceable rather than
/// silently overwriting earlier text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryFact {
    pub id: i64,
    pub project_id: i64,
    pub entity_id: i64,
    pub event_id: Option<i64>,
    pub dimension: String,
    pub value: String,
    pub visibility: String,
    pub status: String,
    pub narrative_chapter_id: Option<i64>,
    pub source_artifact_id: i64,
    pub source_quote: String,
    pub supersedes_fact_id: Option<i64>,
    pub created_at: String,
}

/// The derived-index result for one approved formal chapter. This is exposed
/// separately from creative approval so the UI can distinguish a valid chapter
/// from an index that merely needs a retry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryIndexSource {
    pub project_id: i64,
    pub chapter_id: i64,
    pub source_artifact_id: i64,
    pub status: String,
    pub error: Option<String>,
    pub indexed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildStoryIndexRequest {
    pub project_id: i64,
    pub chapter_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryIndexSummary {
    pub project_id: i64,
    pub chapter_id: i64,
    pub source_artifact_id: i64,
    pub entity_count: usize,
    pub event_count: usize,
    pub fact_count: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorySearchSource {
    pub project_id: i64,
    pub source_kind: String,
    pub source_id: i64,
    pub chapter_id: Option<i64>,
    pub chapter_no_sort: Option<i64>,
    pub stage: Option<String>,
    pub source_artifact_id: Option<i64>,
    pub source_text_hash: String,
    pub normalization_version: String,
    pub status: String,
    pub error: Option<String>,
    pub indexed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedIndexJob {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub source_artifact_id: Option<i64>,
    pub job_type: String,
    pub scope_key: String,
    pub status: String,
    pub attempt_count: i64,
    pub next_attempt_at: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryIndexJobsRequest {
    pub project_id: i64,
    pub chapter_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorySearchStatus {
    pub project_id: i64,
    pub model_version: String,
    pub model_status: String,
    pub sqlite_vec_status: String,
    pub document_count: usize,
    pub embedding_count: usize,
    pub indexed_source_count: usize,
    pub last_indexed_at: Option<String>,
    pub stale: bool,
    pub stale_sources: usize,
    pub sources: Vec<StorySearchSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildStorySearchIndexRequest {
    pub project_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryBible {
    pub id: i64,
    pub project_id: i64,
    pub reader_promise: String,
    pub protagonist_engine: String,
    pub core_conflict: String,
    pub endgame_direction: String,
    pub immutable_rules: String,
    pub canon_version: i64,
    pub status: String,
    pub source_artifact_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryArc {
    pub id: i64,
    pub project_id: i64,
    pub arc_no: i64,
    pub title: String,
    pub objective: String,
    pub entry_state: String,
    pub exit_change: String,
    pub core_conflict: String,
    pub involved_characters: String,
    pub chapter_start: Option<i64>,
    pub chapter_end: Option<i64>,
    pub status: String,
    pub source_artifact_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonIssue {
    pub domain: String,
    pub severity: String,
    pub title: String,
    pub conflict: String,
    pub impact: String,
    pub owner_mode: String,
    pub rework_instruction: String,
    #[serde(default)]
    pub evidence_quotes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryBibleReview {
    pub id: i64,
    pub project_id: i64,
    pub canon_fingerprint: String,
    pub verdict: String,
    pub summary: String,
    pub issues: Vec<CanonIssue>,
    pub status: String,
    pub note: String,
    pub created_at: String,
    pub confirmed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStoryArchitectRequest {
    pub project_id: i64,
    pub mode: StoryArchitectMode,
    pub arc_id: Option<i64>,
    pub user_instruction: Option<String>,
    pub source_artifact_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmStoryBibleRequest {
    pub project_id: i64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryBibleReviewRequest {
    pub project_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmStoryBibleReviewRequest {
    pub project_id: i64,
    pub review_id: i64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptionProposal {
    pub id: i64,
    pub project_id: i64,
    pub source_artifact_id: i64,
    pub target_kind: String,
    pub target_id: Option<i64>,
    pub operation: String,
    pub data: serde_json::Value,
    pub evidence_quote: String,
    pub target_snapshot: Option<String>,
    pub status: String,
    pub validation_error: Option<String>,
    pub decision_note: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareArtifactAdoptionsRequest {
    pub project_id: i64,
    pub artifact_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAdoptionProposalsRequest {
    pub project_id: i64,
    pub artifact_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAdoptionProposalRequest {
    pub proposal_id: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecideAdoptionProposalsRequest {
    pub project_id: i64,
    pub proposal_ids: Vec<i64>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptionBatchResult {
    pub proposals: Vec<AdoptionProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveForeshadowing {
    pub id: Option<i64>,
    pub project_id: i64,
    pub title: String,
    pub content: String,
    pub status: String,
    pub planted_chapter_id: Option<i64>,
    pub planned_payoff_chapter_id: Option<i64>,
    pub planned_payoff_note: String,
    pub source_artifact_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingSkill {
    pub id: i64,
    pub skill_key: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub content: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveWritingSkill {
    pub skill_key: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub content: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDetail {
    pub project: Project,
    pub genre_agent: GenreAgentProfile,
    pub chapters: Vec<Chapter>,
    pub agents: Vec<Agent>,
    pub artifacts: Vec<Artifact>,
    pub approvals: Vec<Approval>,
    pub messages: Vec<Message>,
    pub workflow_runs: Vec<WorkflowRun>,
    pub story_threads: Vec<StoryThread>,
    pub knowledge_cards: Vec<KnowledgeCard>,
    pub foreshadowings: Vec<Foreshadowing>,
    pub story_entities: Vec<StoryEntity>,
    pub story_events: Vec<StoryEvent>,
    pub story_event_participants: Vec<StoryEventParticipant>,
    pub story_facts: Vec<StoryFact>,
    pub story_index_sources: Vec<StoryIndexSource>,
    pub story_search_sources: Vec<StorySearchSource>,
    pub index_jobs: Vec<DerivedIndexJob>,
    pub adoption_proposals: Vec<AdoptionProposal>,
    pub story_bible: Option<StoryBible>,
    pub story_arcs: Vec<StoryArc>,
    pub story_bible_review: Option<StoryBibleReview>,
    pub settings: AiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    pub thinking_enabled: bool,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListModelsInput {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestAiConnectionInput {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub thinking_enabled: Option<bool>,
    pub api_key: Option<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            temperature: 0.75,
            thinking_enabled: false,
            has_api_key: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveAiSettings {
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    #[serde(default)]
    pub thinking_enabled: bool,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAgentRequest {
    pub project_id: i64,
    pub stage: Stage,
    pub chapter_id: Option<i64>,
    pub user_instruction: Option<String>,
    pub source_artifact_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildChapterMemoryRequest {
    pub project_id: i64,
    pub chapter_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPreviewSegment {
    pub label: String,
    pub content: String,
    pub chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPreview {
    pub stage: String,
    pub genre_agent: GenreAgentProfile,
    pub system_prompt: String,
    pub segments: Vec<ContextPreviewSegment>,
    pub total_chars: usize,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionRequest {
    pub project_id: i64,
    pub artifact_id: i64,
    pub feedback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanReplacementRequest {
    pub project_id: i64,
    pub artifact_id: i64,
    pub find_text: String,
    pub replace_text: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSpanRevisionRequest {
    pub project_id: i64,
    pub artifact_id: i64,
    pub find_text: String,
    pub instruction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteArtifactRequest {
    pub project_id: i64,
    pub artifact_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearChapterHistoryRequest {
    pub project_id: i64,
    pub chapter_id: i64,
    pub keep_artifact_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryCleanupResult {
    pub deleted_artifact_ids: Vec<i64>,
    pub kept_artifact_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityReviewRequest {
    pub project_id: i64,
    pub chapter_ids: Option<Vec<i64>>,
    pub candidate_artifact_id: Option<i64>,
    pub candidate_artifact_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterGateRequest {
    pub project_id: i64,
    pub chapter_id: i64,
    pub artifact_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterSplitPlanRequest {
    pub project_id: i64,
    pub chapter_id: i64,
    pub artifact_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryContextSearchInput {
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub query: String,
    pub limit: Option<usize>,
    #[serde(default)]
    pub include_immediate_previous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactFilters {
    pub project_id: i64,
    pub stage: Option<String>,
    pub chapter_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStepResult {
    pub artifact: Artifact,
    pub run: WorkflowRun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewIssue {
    pub issue_type: String,
    pub severity: String,
    pub location: String,
    pub reason: String,
    pub suggestion: String,
    #[serde(default)]
    pub evidence_quote: String,
    #[serde(default)]
    pub action_evidence_quote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetric {
    pub label: String,
    pub value: f64,
    pub unit: String,
    pub target: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityWarning {
    pub title: String,
    pub detail: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    pub artifact_id: i64,
    pub stage: String,
    pub verdict: String,
    pub score: u8,
    pub summary: String,
    pub metrics: Vec<QualityMetric>,
    pub warnings: Vec<QualityWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityIssue {
    pub issue_type: String,
    pub severity: String,
    pub chapters: Vec<String>,
    pub reason: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityReport {
    pub project_id: i64,
    pub chapter_titles: Vec<String>,
    pub verdict: String,
    pub summary: String,
    pub issues: Vec<ContinuityIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityLedgerEntry {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: i64,
    pub source_artifact_id: i64,
    pub source_text_hash: String,
    pub normalization_version: String,
    pub entity_kind: String,
    pub entity_key: String,
    pub entity_label: String,
    pub state_kind: String,
    pub state_value: String,
    pub evidence_quote: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerContinuityCheckRequest {
    pub project_id: i64,
    pub artifact_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerContinuityIssue {
    pub severity: String,
    pub entity_label: String,
    pub entity_kind: String,
    pub state_kind: String,
    pub candidate_quote: String,
    pub source_chapter: String,
    pub source_quote: String,
    pub reason: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerContinuityReport {
    pub project_id: i64,
    pub artifact_id: i64,
    pub summary: String,
    pub issues: Vec<LedgerContinuityIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateBlocker {
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterGateReport {
    pub project_id: i64,
    pub chapter_id: i64,
    pub artifact_id: i64,
    pub passed: bool,
    pub verdict: String,
    pub recommended_action: String,
    pub action_reason: String,
    pub summary: String,
    pub blockers: Vec<GateBlocker>,
    pub quality: QualityReport,
    pub continuity: ContinuityReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterSplitPlan {
    pub project_id: i64,
    pub chapter_id: i64,
    pub artifact_id: i64,
    pub suggested_current_title: String,
    pub suggested_next_title: String,
    pub rationale: String,
    pub current_chapter_mission: String,
    pub next_chapter_mission: String,
    pub keep_in_current: Vec<String>,
    pub move_to_next: Vec<String>,
    pub carryover_closing_beats: Vec<String>,
    pub next_chapter_opening_beats: Vec<String>,
    pub revision_prompt_current: String,
    pub next_chapter_instruction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryContextSnippet {
    pub source_label: String,
    pub matched_term: String,
    pub content: String,
    pub score: usize,
}
