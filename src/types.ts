export type Stage = "setting" | "outline" | "characters" | "draft" | "review" | "revision";

export interface Project {
  id: number;
  title: string;
  genre: string;
  target_words: number;
  premise: string;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface NewProject {
  title: string;
  genre: string;
  target_words: number;
  premise: string;
}

export interface ProjectUpdate {
  id: number;
  title: string;
  genre: string;
  target_words: number;
  premise: string;
  status: string;
}

export interface NewChapter {
  project_id: number;
  title?: string | null;
}

export interface Agent {
  id: number;
  stage: string;
  name: string;
  role: string;
  system_prompt: string;
  temperature: number;
}

export interface GenreAgentProfile {
  agent_key: string;
  name: string;
  role: string;
  system_prompt: string;
  primary_skill_key: string;
  allowed_skill_keys: string[];
}

export type StoryArchitectMode = "initialize" | "refine_canon" | "plan_current_arc" | "extend_next_arc" | "design_characters";

export interface StoryBible {
  id: number;
  project_id: number;
  reader_promise: string;
  protagonist_engine: string;
  core_conflict: string;
  endgame_direction: string;
  immutable_rules: string;
  canon_version: number;
  status: string;
  source_artifact_id?: number | null;
  created_at: string;
  updated_at: string;
}

export interface StoryArc {
  id: number;
  project_id: number;
  arc_no: number;
  title: string;
  objective: string;
  entry_state: string;
  exit_change: string;
  core_conflict: string;
  involved_characters: string;
  chapter_start?: number | null;
  chapter_end?: number | null;
  status: string;
  source_artifact_id?: number | null;
  created_at: string;
  updated_at: string;
}

export interface CanonIssue {
  domain: string;
  severity: "minor" | "moderate" | "major" | string;
  title: string;
  conflict: string;
  impact: string;
  owner_mode: StoryArchitectMode | string;
  rework_instruction: string;
  evidence_quotes: string[];
}

export interface StoryBibleReview {
  id: number;
  project_id: number;
  canon_fingerprint: string;
  verdict: string;
  summary: string;
  issues: CanonIssue[];
  status: string;
  note: string;
  created_at: string;
  confirmed_at?: string | null;
}

export interface Chapter {
  id: number;
  project_id: number;
  chapter_no: number;
  title: string;
  status: string;
  current_artifact_id?: number | null;
  created_at: string;
  updated_at: string;
}

export interface ChapterUpdate {
  id: number;
  title: string;
  status: string;
}

export interface Artifact {
  id: number;
  project_id: number;
  chapter_id?: number | null;
  stage: Stage;
  title: string;
  content: string;
  version: number;
  status: string;
  parent_artifact_id?: number | null;
  created_at: string;
}

export interface SpanReplacementInput {
  project_id: number;
  artifact_id: number;
  find_text: string;
  replace_text: string;
  note?: string | null;
}

export interface AiSpanRevisionInput {
  project_id: number;
  artifact_id: number;
  find_text: string;
  instruction: string;
}

export interface DeleteArtifactInput {
  project_id: number;
  artifact_id: number;
}

export interface ClearChapterHistoryInput {
  project_id: number;
  chapter_id: number;
  keep_artifact_ids?: number[] | null;
}

export interface HistoryCleanupResult {
  deleted_artifact_ids: number[];
  kept_artifact_ids: number[];
}

export interface Approval {
  id: number;
  project_id: number;
  chapter_id?: number | null;
  stage: Stage;
  artifact_id: number;
  note: string;
  created_at: string;
}

export interface Message {
  id: number;
  project_id: number;
  chapter_id?: number | null;
  role: string;
  content: string;
  created_at: string;
}

export interface WorkflowRun {
  id: number;
  project_id: number;
  chapter_id?: number | null;
  stage: string;
  input: string;
  output: string;
  status: string;
  error?: string | null;
  elapsed_ms: number;
  created_at: string;
}

export interface ChapterMemoryRecord {
  id: number;
  project_id: number;
  chapter_id: number;
  source_artifact_id: number;
  source_text_hash: string;
  normalization_version: string;
  content: string;
  created_at: string;
  updated_at: string;
}

export interface StoryThread {
  id: number;
  project_id: number;
  thread_key: string;
  label: string;
  kind: string;
  status: string;
  current_cost?: string | null;
  last_seen_chapter_no?: number | null;
  last_artifact_id?: number | null;
  notes: string;
  created_at: string;
  updated_at: string;
}

export interface KnowledgeCard {
  id: number;
  project_id: number;
  category: string;
  title: string;
  content: string;
  status: string;
  source_artifact_id?: number | null;
  source_chapter_id?: number | null;
  created_at: string;
  updated_at: string;
}

export interface SaveKnowledgeCardInput {
  id?: number | null;
  project_id: number;
  category: string;
  title: string;
  content: string;
  status: string;
  source_artifact_id?: number | null;
  source_chapter_id?: number | null;
}

export interface Foreshadowing {
  id: number;
  project_id: number;
  title: string;
  content: string;
  status: string;
  planted_chapter_id?: number | null;
  planned_payoff_chapter_id?: number | null;
  planned_payoff_note: string;
  source_artifact_id?: number | null;
  created_at: string;
  updated_at: string;
}

export interface StoryEntity {
  id: number;
  project_id: number;
  kind: string;
  name: string;
  status: string;
  first_seen_chapter_id?: number | null;
  source_artifact_id?: number | null;
  source_quote: string;
  created_at: string;
  updated_at: string;
}

export interface StoryEvent {
  id: number;
  project_id: number;
  title: string;
  kind: string;
  status: string;
  story_time: string;
  summary: string;
  narrative_chapter_id?: number | null;
  source_artifact_id: number;
  source_quote: string;
  created_at: string;
  updated_at: string;
}

export interface StoryEventParticipant {
  event_id: number;
  entity_id: number;
  entity_name: string;
  role: string;
}

export interface StoryFact {
  id: number;
  project_id: number;
  entity_id: number;
  event_id?: number | null;
  dimension: string;
  value: string;
  visibility: string;
  status: string;
  narrative_chapter_id?: number | null;
  source_artifact_id: number;
  source_quote: string;
  supersedes_fact_id?: number | null;
  created_at: string;
}

export interface StoryIndexSource {
  project_id: number;
  chapter_id: number;
  source_artifact_id: number;
  status: string;
  error?: string | null;
  indexed_at: string;
}

export interface StoryIndexSummary {
  project_id: number;
  chapter_id: number;
  source_artifact_id: number;
  entity_count: number;
  event_count: number;
  fact_count: number;
  status: string;
}

export interface StorySearchSource {
  project_id: number;
  source_kind: string;
  source_id: number;
  chapter_id?: number | null;
  chapter_no_sort?: number | null;
  stage?: string | null;
  source_artifact_id?: number | null;
  source_text_hash: string;
  normalization_version: string;
  status: string;
  error?: string | null;
  indexed_at: string;
}

export interface DerivedIndexJob {
  id: number;
  project_id: number;
  chapter_id?: number | null;
  source_artifact_id?: number | null;
  job_type: string;
  scope_key: string;
  status: string;
  attempt_count: number;
  next_attempt_at: string;
  last_error?: string | null;
  created_at: string;
  started_at?: string | null;
  finished_at?: string | null;
  updated_at: string;
}

export interface StorySearchStatus {
  project_id: number;
  model_version: string;
  model_status: string;
  sqlite_vec_status: string;
  document_count: number;
  embedding_count: number;
  indexed_source_count: number;
  last_indexed_at?: string | null;
  stale: boolean;
  stale_sources: number;
  sources: StorySearchSource[];
}

export type AdoptionTargetKind = "knowledge_card" | "foreshadowing";
export type AdoptionProposalStatus = "pending" | "applied" | "rejected" | "stale";

export interface AdoptionProposal {
  id: number;
  project_id: number;
  source_artifact_id: number;
  target_kind: AdoptionTargetKind | string;
  target_id?: number | null;
  operation: "create" | "update";
  data: Record<string, unknown>;
  evidence_quote: string;
  target_snapshot?: string | null;
  status: AdoptionProposalStatus;
  validation_error?: string | null;
  decision_note: string;
  created_at: string;
  updated_at: string;
}

export interface AdoptionBatchResult {
  proposals: AdoptionProposal[];
}

export interface SaveForeshadowingInput {
  id?: number | null;
  project_id: number;
  title: string;
  content: string;
  status: string;
  planted_chapter_id?: number | null;
  planned_payoff_chapter_id?: number | null;
  planned_payoff_note: string;
  source_artifact_id?: number | null;
}

export interface WritingSkill {
  id: number;
  skill_key: string;
  name: string;
  category: string;
  description: string;
  content: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface SaveWritingSkill {
  skill_key: string;
  name: string;
  category: string;
  description: string;
  content: string;
  enabled: boolean;
}

export interface AiSettings {
  base_url: string;
  model: string;
  temperature: number;
  thinking_enabled: boolean;
  has_api_key: boolean;
}

export interface ModelInfo {
  id: string;
  owned_by?: string | null;
}

export interface ProjectDetail {
  project: Project;
  genre_agent: GenreAgentProfile;
  chapters: Chapter[];
  agents: Agent[];
  artifacts: Artifact[];
  approvals: Approval[];
  messages: Message[];
  workflow_runs: WorkflowRun[];
  story_threads: StoryThread[];
  knowledge_cards: KnowledgeCard[];
  foreshadowings: Foreshadowing[];
  story_entities: StoryEntity[];
  story_events: StoryEvent[];
  story_event_participants: StoryEventParticipant[];
  story_facts: StoryFact[];
  story_index_sources: StoryIndexSource[];
  story_search_sources: StorySearchSource[];
  index_jobs: DerivedIndexJob[];
  adoption_proposals: AdoptionProposal[];
  story_bible?: StoryBible | null;
  story_arcs: StoryArc[];
  story_bible_review?: StoryBibleReview | null;
  settings: AiSettings;
}

export interface AgentStepResult {
  artifact: Artifact;
  run: {
    id: number;
    status: string;
    elapsed_ms: number;
  };
}

export interface ContextPreviewSegment {
  label: string;
  content: string;
  chars: number;
  truncated: boolean;
}

export interface ContextPreview {
  stage: string;
  genre_agent: GenreAgentProfile;
  system_prompt: string;
  segments: ContextPreviewSegment[];
  total_chars: number;
  estimated_tokens: number;
}

export interface ReviewIssue {
  issue_type: string;
  severity: string;
  location: string;
  reason: string;
  suggestion: string;
  evidence_quote?: string;
  action_evidence_quote?: string;
}

export interface QualityMetric {
  label: string;
  value: number;
  unit: string;
  target?: number | null;
}

export interface QualityWarning {
  title: string;
  detail: string;
  suggestion: string;
}

export interface QualityReport {
  artifact_id: number;
  stage: string;
  verdict: "strong" | "usable" | "needs_revision" | "weak" | string;
  score: number;
  summary: string;
  metrics: QualityMetric[];
  warnings: QualityWarning[];
}

export interface ContinuityIssue {
  issue_type: string;
  severity: string;
  chapters: string[];
  reason: string;
  suggestion: string;
}

export interface ContinuityReport {
  project_id: number;
  chapter_titles: string[];
  verdict: "strong" | "usable" | "needs_revision" | "weak" | string;
  summary: string;
  issues: ContinuityIssue[];
}

export interface LedgerContinuityIssue {
  severity: string;
  entity_label: string;
  entity_kind: string;
  state_kind: string;
  candidate_quote: string;
  source_chapter: string;
  source_quote: string;
  reason: string;
  suggestion: string;
}

export interface LedgerContinuityReport {
  project_id: number;
  artifact_id: number;
  summary: string;
  issues: LedgerContinuityIssue[];
}

export interface GateBlocker {
  kind: string;
  severity: string;
  title: string;
  detail: string;
  suggestion: string;
}

export interface ChapterGateReport {
  project_id: number;
  chapter_id: number;
  artifact_id: number;
  passed: boolean;
  verdict: "strong" | "usable" | "needs_revision" | "weak" | "blocked" | string;
  recommended_action: "approve" | "revise" | "split" | string;
  action_reason: string;
  summary: string;
  blockers: GateBlocker[];
  quality: QualityReport;
  continuity: ContinuityReport;
}

export interface ChapterSplitPlan {
  project_id: number;
  chapter_id: number;
  artifact_id: number;
  suggested_current_title: string;
  suggested_next_title: string;
  rationale: string;
  current_chapter_mission: string;
  next_chapter_mission: string;
  keep_in_current: string[];
  move_to_next: string[];
  carryover_closing_beats: string[];
  next_chapter_opening_beats: string[];
  revision_prompt_current: string;
  next_chapter_instruction: string;
}

export interface ContinuityReviewInput {
  project_id: number;
  chapter_ids?: number[] | null;
  candidate_artifact_id?: number | null;
  candidate_artifact_ids?: number[] | null;
}

export interface StoryContextSearchInput {
  project_id: number;
  chapter_id?: number | null;
  query: string;
  limit?: number | null;
  include_immediate_previous?: boolean;
}

export interface StoryContextSnippet {
  source_label: string;
  matched_term: string;
  content: string;
  score: number;
}
