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
            Stage::Setting => "设定 Agent",
            Stage::Outline => "大纲 Agent",
            Stage::Characters => "角色 Agent",
            Stage::Draft => "写作 Agent",
            Stage::Review => "试读 Agent",
            Stage::Revision => "修订 Agent",
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
    pub current_artifact_id: Option<i64>,
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
    pub chapters: Vec<Chapter>,
    pub agents: Vec<Agent>,
    pub artifacts: Vec<Artifact>,
    pub approvals: Vec<Approval>,
    pub messages: Vec<Message>,
    pub workflow_runs: Vec<WorkflowRun>,
    pub story_threads: Vec<StoryThread>,
    pub knowledge_cards: Vec<KnowledgeCard>,
    pub foreshadowings: Vec<Foreshadowing>,
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
