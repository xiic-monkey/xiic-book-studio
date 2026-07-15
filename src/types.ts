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
  stage: Stage;
  name: string;
  role: string;
  system_prompt: string;
  temperature: number;
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
  current_artifact_id?: number | null;
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
  chapters: Chapter[];
  agents: Agent[];
  artifacts: Artifact[];
  approvals: Approval[];
  messages: Message[];
  workflow_runs: WorkflowRun[];
  story_threads: StoryThread[];
  knowledge_cards: KnowledgeCard[];
  foreshadowings: Foreshadowing[];
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
