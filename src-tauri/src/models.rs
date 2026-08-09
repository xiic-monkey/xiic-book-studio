use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    pub project_id: i64,
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
    pub editable_role: String,
    pub system_prompt: String,
    pub editable_system_prompt: String,
    pub temperature: f64,
    pub provider_base_url: String,
    pub model: String,
    pub thinking_enabled: bool,
    pub thinking_level: String,
    pub uses_global_runtime_settings: bool,
    pub enabled_tool_keys: Vec<String>,
    pub allowed_skill_keys: Vec<String>,
}

impl Agent {
    pub fn ai_settings(&self) -> AiSettings {
        AiSettings {
            base_url: self.provider_base_url.clone(),
            model: self.model.clone(),
            temperature: self.temperature,
            thinking_enabled: self.thinking_enabled,
            thinking_level: self.thinking_level.clone(),
            has_api_key: false,
        }
    }

    pub fn has_tool(&self, key: &str) -> bool {
        crate::agent_tools::has_tool(&self.enabled_tool_keys, key)
    }

    pub fn has_skill(&self, key: &str) -> bool {
        self.allowed_skill_keys.iter().any(|item| item == key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Proposal,
}

impl ToolKind {
    pub fn parse(value: &str) -> Self {
        if value == "proposal" {
            Self::Proposal
        } else {
            Self::Read
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct AgentToolDefinition {
    pub key: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub kind: ToolKind,
    pub supported_stages: Vec<String>,
    pub previewable: bool,
    #[ts(type = "Record<string, unknown>")]
    pub parameters_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum ToolProtocol {
    Auto,
    Native,
    Structured,
}

impl ToolProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Native => "native",
            Self::Structured => "structured",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "native" => Self::Native,
            "structured" => Self::Structured,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct GenreAgentProfile {
    pub agent_key: String,
    pub name: String,
    pub role: String,
    pub system_prompt: String,
    pub primary_skill_key: String,
    pub allowed_skill_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Artifact {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    #[ts(type = "Stage")]
    pub stage: String,
    pub title: String,
    pub content: String,
    pub version: i64,
    pub status: String,
    pub parent_artifact_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Approval {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub artifact_id: i64,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Message {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct WorkflowRunSummary {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub status: String,
    pub error: Option<String>,
    pub elapsed_ms: i64,
    pub output_chars: usize,
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StoryEventParticipant {
    pub event_id: i64,
    pub entity_id: i64,
    pub entity_name: String,
    pub role: String,
}

/// An immutable, evidence-backed assertion extracted from approved formal text.
/// `status` and `supersedes_fact_id` make a later change traceable rather than
/// silently overwriting earlier text.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
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
    #[serde(default)]
    pub reference_selection: Option<ReferenceSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceTag {
    Style,
    Structure,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReferenceSelection {
    #[serde(default = "default_reference_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub source_ids: Option<Vec<u64>>,
    #[serde(default)]
    pub tags: Option<Vec<ReferenceTag>>,
}

impl Default for ReferenceSelection {
    fn default() -> Self {
        Self {
            enabled: true,
            source_ids: None,
            tags: None,
        }
    }
}

fn default_reference_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceMaterial {
    pub id: u64,
    pub project_id: i64,
    pub file_name: String,
    pub char_count: usize,
    pub tags: Vec<ReferenceTag>,
    pub enabled: bool,
    pub chunk_count: usize,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReferenceTextRequest {
    pub project_id: i64,
    pub file_name: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<ReferenceTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateReferenceMaterialRequest {
    pub project_id: i64,
    pub reference_id: u64,
    pub enabled: Option<bool>,
    pub tags: Option<Vec<ReferenceTag>>,
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AdoptionProposal {
    pub id: i64,
    pub project_id: i64,
    pub source_artifact_id: i64,
    pub target_kind: String,
    pub target_id: Option<i64>,
    pub operation: String,
    #[ts(type = "Record<string, unknown>")]
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
    pub project_id: i64,
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
    #[serde(default)]
    pub id: Option<i64>,
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
    pub canonical_fingerprint: String,
    pub settings: AiSettings,
}

/// V2 project workspace payload. It intentionally excludes artifact bodies and
/// workflow inputs/outputs; those are loaded through dedicated detail queries.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProjectWorkspace {
    pub project: Project,
    pub genre_agent: GenreAgentProfile,
    pub chapters: Vec<Chapter>,
    pub artifacts: Vec<ArtifactSummary>,
    pub approvals: Vec<Approval>,
    pub messages: Vec<Message>,
    pub workflow_runs: Vec<WorkflowRunSummary>,
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
    pub canonical_fingerprint: String,
    pub settings: AiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AiSettings {
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    pub thinking_enabled: bool,
    #[ts(type = "\"off\" | \"low\" | \"medium\" | \"high\"")]
    pub thinking_level: String,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: i64,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    pub thinking_enabled: bool,
    pub thinking_level: String,
    pub tool_protocol: ToolProtocol,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveAiProvider {
    pub id: Option<i64>,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f64,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default = "default_thinking_level")]
    pub thinking_level: String,
    #[serde(default = "default_tool_protocol")]
    pub tool_protocol: ToolProtocol,
}

fn default_tool_protocol() -> ToolProtocol {
    ToolProtocol::Auto
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
    pub thinking_level: Option<String>,
    pub api_key: Option<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            temperature: 0.75,
            thinking_enabled: false,
            thinking_level: "off".to_string(),
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
    #[serde(default = "default_thinking_level")]
    pub thinking_level: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveAgentSettings {
    pub agent_id: i64,
    pub provider_base_url: String,
    pub model: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default)]
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub uses_global_runtime_settings: Option<bool>,
    #[serde(default)]
    pub enabled_tool_keys: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_skill_keys: Option<Vec<String>>,
}

fn default_thinking_level() -> String {
    "off".to_string()
}

pub fn normalize_thinking_level(enabled: bool, value: &str) -> Result<String, String> {
    if !enabled {
        return Ok("off".to_string());
    }

    match value.trim().to_ascii_lowercase().as_str() {
        "" | "off" => Ok("medium".to_string()),
        "low" | "medium" | "high" => Ok(value.trim().to_ascii_lowercase()),
        _ => Err("思考强度只能是 low、medium 或 high".to_string()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAgentRequest {
    pub project_id: i64,
    pub stage: Stage,
    pub chapter_id: Option<i64>,
    pub user_instruction: Option<String>,
    pub source_artifact_id: Option<i64>,
    #[serde(default)]
    pub reference_selection: Option<ReferenceSelection>,
    #[serde(default)]
    pub prepared_context_id: Option<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentRunRequest {
    pub project_id: i64,
    pub stage: Stage,
    pub chapter_id: Option<i64>,
    pub user_instruction: Option<String>,
    pub source_artifact_id: Option<i64>,
    #[serde(default)]
    pub reference_selection: Option<ReferenceSelection>,
    #[serde(default)]
    pub prepared_context_id: Option<i64>,
}

impl From<AgentRunRequest> for RunAgentRequest {
    fn from(value: AgentRunRequest) -> Self {
        Self {
            project_id: value.project_id,
            stage: value.stage,
            chapter_id: value.chapter_id,
            user_instruction: value.user_instruction,
            source_artifact_id: value.source_artifact_id,
            reference_selection: value.reference_selection,
            prepared_context_id: value.prepared_context_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ContextSegment {
    pub kind: String,
    pub label: String,
    pub source: String,
    pub content: String,
    pub chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PreparedContext {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub fingerprint: String,
    pub system_prompt: String,
    pub prompt: String,
    pub segments: Vec<ContextSegment>,
    pub tool_invocation_ids: Vec<i64>,
    pub total_chars: usize,
    pub estimated_tokens: usize,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ToolCall {
    pub call_id: String,
    pub tool_key: String,
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub arguments: serde_json::Value,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ToolResult {
    pub call_id: String,
    pub tool_key: String,
    pub status: String,
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub data: serde_json::Value,
    #[serde(default)]
    pub citations: Vec<String>,
    pub error: Option<String>,
    pub elapsed_ms: i64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ToolInvocation {
    pub id: i64,
    pub run_id: Option<i64>,
    pub prepared_context_id: Option<i64>,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub tool_key: String,
    pub protocol: String,
    #[ts(type = "Record<string, unknown>")]
    pub arguments: serde_json::Value,
    #[ts(type = "Record<string, unknown>")]
    pub result: serde_json::Value,
    pub status: String,
    pub error: Option<String>,
    pub elapsed_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Applied,
    Rejected,
    Expired,
}

impl ProposalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "applied" => Self::Applied,
            "rejected" => Self::Rejected,
            "expired" => Self::Expired,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ActionProposal {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub source_run_id: Option<i64>,
    pub proposal_type: String,
    pub summary: String,
    #[ts(type = "Record<string, unknown>")]
    pub payload: serde_json::Value,
    pub expected_version: Option<String>,
    pub status: ProposalStatus,
    pub decision_note: String,
    pub created_at: String,
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct LegacyAgentPrompt {
    pub id: i64,
    pub legacy_agent_id: Option<i64>,
    pub stage: String,
    pub name: String,
    pub role: String,
    pub system_prompt: String,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListActionProposalsRequest {
    pub project_id: i64,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecideActionProposalRequest {
    pub project_id: i64,
    pub proposal_id: i64,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProposalApplyResult {
    pub proposal: ActionProposal,
    pub entity_kind: String,
    pub entity_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProviderCapabilities {
    pub provider_base_url: String,
    pub configured_protocol: ToolProtocol,
    pub detected_protocol: Option<ToolProtocol>,
    pub last_error: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentRunSummary {
    pub run: WorkflowRun,
    pub artifact: Option<Artifact>,
    pub prepared_context_id: Option<i64>,
    pub tool_invocations: Vec<ToolInvocation>,
    pub proposals: Vec<ActionProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RunEvent {
    pub run_id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub sequence: i64,
    pub kind: String,
    pub delta: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ActiveAgentRun {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub stage: String,
    pub output: String,
    pub status: String,
    pub error: Option<String>,
    pub elapsed_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ArtifactSummary {
    pub id: i64,
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    #[ts(type = "Stage")]
    pub stage: String,
    pub title: String,
    pub version: i64,
    pub status: String,
    pub parent_artifact_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionRequest {
    pub project_id: i64,
    pub artifact_id: i64,
    pub feedback: String,
    #[serde(default)]
    pub reference_selection: Option<ReferenceSelection>,
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

/// Server-owned reranking request. The client supplies only the search intent;
/// candidates are always retrieved again by the application before the model sees them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryContextRerankRequest {
    pub project_id: i64,
    pub chapter_id: Option<i64>,
    pub query: String,
    #[serde(default)]
    pub include_immediate_previous: bool,
    pub stage: Option<Stage>,
    pub task_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryContextRerankedSnippet {
    pub candidate_id: usize,
    pub source_label: String,
    pub matched_term: String,
    pub content: String,
    pub score: usize,
    pub category: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryContextRerankResult {
    pub candidates: Vec<StoryContextSnippet>,
    pub selected: Vec<StoryContextRerankedSnippet>,
    pub status: String,
    pub error: Option<String>,
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
