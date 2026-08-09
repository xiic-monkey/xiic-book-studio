import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { V2_COMMANDS } from "./generated/v2-contracts";
import type { DerivedIndexJob as V2DerivedIndexJob } from "./generated/v2-contracts";
import type {
  AdoptionBatchResult,
  AdoptionProposal,
  ActionProposal,
  ActiveAgentRun,
  Approval,
  AiSpanRevisionInput,
  AgentStepResult,
  AgentRunSummary,
  AgentToolDefinition,
  AiSettings,
  AiProvider,
  Agent,
  Artifact,
  ArtifactSummary,
  Chapter,
  ChapterMemoryRecord,
  ChapterGateReport,
  ChapterSplitPlan,
  ChapterUpdate,
  ClearChapterHistoryInput,
  DeleteArtifactInput,
  LedgerContinuityReport,
  ContinuityReport,
  ContextPreview,
  DerivedIndexJob,
  Foreshadowing,
  HistoryCleanupResult,
  KnowledgeCard,
  LegacyAgentPrompt,
  NewProject,
  NewChapter,
  ModelInfo,
  PreparedContext,
  Project,
  ProjectUpdate,
  ProjectDetail,
  ProjectWorkspace,
  ProposalApplyResult,
  ProviderCapabilities,
  QualityReport,
  ReferenceMaterial,
  ReferenceSelection,
  ReferenceTag,
  RunEvent,
  ReviewIssue,
  SpanReplacementInput,
  SaveWritingSkill,
  SaveAiProvider,
  SaveForeshadowingInput,
  SaveKnowledgeCardInput,
  Stage,
  StoryContextSnippet,
  StoryContextRerankResult,
  StorySearchStatus,
  StoryIndexSummary,
  StoryArc,
  StoryBible,
  StoryBibleReview,
  StoryArchitectMode,
  WritingSkill
} from "./types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

const DEV_API_BASE = import.meta.env.VITE_DEV_API_BASE ?? "http://127.0.0.1:4141";

export type RuntimeMode = "tauri" | "web-dev-api";

function isTauriRuntime() {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>) {
  if (isTauriRuntime()) {
    return invoke<T>(command, args);
  }

  const response = await fetch(`${DEV_API_BASE}/commands/${command}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(args ?? {}),
  });

  const payload = (await response.json()) as { ok: boolean; data?: T; error?: string };
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error ?? `调用失败：${command}`);
  }
  return payload.data as T;
}

export const api = {
  getRuntimeMode: (): RuntimeMode => (isTauriRuntime() ? "tauri" : "web-dev-api"),
  listProjects: () => invokeCommand<Project[]>("list_projects"),
  createProject: (input: NewProject) => invokeCommand<Project>("create_project", { input }),
  updateProject: (input: ProjectUpdate) => invokeCommand<Project>("update_project", { input }),
  deleteProject: (projectId: number) => invokeCommand<void>("delete_project", { projectId }),
  importReferenceText: (input: {
    project_id: number;
    file_name: string;
    content: string;
    tags: ReferenceTag[];
  }) => invokeCommand<ReferenceMaterial>("import_reference_text", { input }),
  listReferenceMaterials: (projectId: number) =>
    invokeCommand<ReferenceMaterial[]>("list_reference_materials", { projectId }),
  updateReferenceMaterial: (input: {
    project_id: number;
    reference_id: number;
    enabled?: boolean | null;
    tags?: ReferenceTag[] | null;
  }) => invokeCommand<ReferenceMaterial>("update_reference_material", { input }),
  removeReferenceMaterial: (projectId: number, referenceId: number) =>
    invokeCommand<void>("remove_reference_material", { projectId, referenceId }),
  createChapter: (input: NewChapter) => invokeCommand<Chapter>("create_chapter", { input }),
  deleteChapter: (projectId: number, chapterId: number) =>
    invokeCommand<void>("delete_chapter", { projectId, chapterId }),
  updateChapter: (input: ChapterUpdate) => invokeCommand<Chapter>("update_chapter", { input }),
  getProject: (projectId: number) =>
    invokeCommand<ProjectWorkspace>(V2_COMMANDS.getProjectWorkspace, { projectId }),
  getArtifact: (projectId: number, artifactId: number) =>
    invokeCommand<Artifact>(V2_COMMANDS.getArtifact, { projectId, artifactId }),
  saveAiSettings: (input: {
    base_url: string;
    model: string;
    temperature: number;
    thinking_enabled: boolean;
    thinking_level: string;
    api_key?: string | null;
  }) => invokeCommand<AiSettings>("save_ai_settings", { input }),
  listAiProviders: () => invokeCommand<AiProvider[]>("list_ai_providers"),
  saveAiProvider: (input: SaveAiProvider) =>
    invokeCommand<AiProvider>("save_ai_provider", { input }),
  deleteAiProvider: (providerId: number) =>
    invokeCommand<void>("delete_ai_provider", { providerId }),
  listAgents: () => invokeCommand<Agent[]>("list_agents"),
  listAgentTools: () => invokeCommand<AgentToolDefinition[]>("list_agent_tools"),
  listToolDefinitions: () => invokeCommand<AgentToolDefinition[]>(V2_COMMANDS.listToolDefinitions),
  getProviderCapabilities: (providerBaseUrl: string) =>
    invokeCommand<ProviderCapabilities>(V2_COMMANDS.getProviderCapabilities, { providerBaseUrl }),
  saveAgentSettings: (input: {
    agent_id: number;
    provider_base_url: string;
    model: string;
    name?: string | null;
    role?: string | null;
    system_prompt?: string | null;
    temperature?: number | null;
    thinking_enabled: boolean;
    thinking_level?: string | null;
    uses_global_runtime_settings?: boolean | null;
    enabled_tool_keys?: string[] | null;
    allowed_skill_keys?: string[] | null;
  }) => invokeCommand<Agent>("save_agent_settings", { input }),
  resetAgentPrompt: (agentId: number) =>
    invokeCommand<Agent>(V2_COMMANDS.resetAgentPrompt, { agentId }),
  listWritingSkills: () => invokeCommand<WritingSkill[]>("list_writing_skills"),
  saveWritingSkill: (input: SaveWritingSkill) =>
    invokeCommand<WritingSkill>("save_writing_skill", { input }),
  saveKnowledgeCard: (input: SaveKnowledgeCardInput) =>
    invokeCommand<KnowledgeCard>("save_knowledge_card", { input }),
  saveForeshadowing: (input: SaveForeshadowingInput) =>
    invokeCommand<Foreshadowing>("save_foreshadowing", { input }),
  prepareArtifactAdoptions: (input: { project_id: number; artifact_id: number }) =>
    invokeCommand<AdoptionProposal[]>("prepare_artifact_adoptions", { input }),
  listAdoptionProposals: (input: { project_id: number; artifact_id?: number | null }) =>
    invokeCommand<AdoptionProposal[]>("list_adoption_proposals", { input }),
  updateAdoptionProposal: (input: { project_id: number; proposal_id: number; data: Record<string, unknown> }) =>
    invokeCommand<AdoptionProposal>("update_adoption_proposal", { input }),
  applyAdoptionProposals: (input: { project_id: number; proposal_ids: number[]; note: string }) =>
    invokeCommand<AdoptionBatchResult>("apply_adoption_proposals", { input }),
  rejectAdoptionProposals: (input: { project_id: number; proposal_ids: number[]; note: string }) =>
    invokeCommand<AdoptionBatchResult>("reject_adoption_proposals", { input }),
  getSettings: () => invokeCommand<AiSettings>("get_settings"),
  testAiConnection: (input?: {
    base_url?: string | null;
    model?: string | null;
    temperature?: number | null;
    thinking_enabled?: boolean | null;
    thinking_level?: string | null;
    api_key?: string | null;
  }) => invokeCommand<string>("test_ai_connection", { input }),
  listModels: (input?: { base_url?: string | null; api_key?: string | null }) =>
    invokeCommand<ModelInfo[]>("list_models", { input }),
  runAgentStep: (input: {
    project_id: number;
    stage: Stage;
    chapter_id?: number | null;
    user_instruction?: string | null;
    source_artifact_id?: number | null;
    reference_selection?: ReferenceSelection | null;
    prepared_context_id?: number | null;
  }) => invokeCommand<AgentStepResult>("run_agent_step", { input }),
  previewAgentRun: (input: {
    project_id: number;
    stage: Stage;
    chapter_id?: number | null;
    user_instruction?: string | null;
    source_artifact_id?: number | null;
    reference_selection?: ReferenceSelection | null;
    prepared_context_id?: number | null;
  }) => invokeCommand<PreparedContext>(V2_COMMANDS.previewAgentRun, { input }),
  startAgentRun: (input: {
    project_id: number;
    stage: Stage;
    chapter_id?: number | null;
    user_instruction?: string | null;
    source_artifact_id?: number | null;
    reference_selection?: ReferenceSelection | null;
    prepared_context_id?: number | null;
  }) => invokeCommand<AgentRunSummary>(V2_COMMANDS.startAgentRun, { input }),
  startStoryArchitectRun: (input: {
    project_id: number;
    mode: StoryArchitectMode;
    arc_id?: number | null;
    user_instruction?: string | null;
    source_artifact_id?: number | null;
    reference_selection?: ReferenceSelection | null;
  }) => invokeCommand<AgentRunSummary>(V2_COMMANDS.startStoryArchitectRun, { input }),
  startRevisionRun: (input: {
    project_id: number;
    artifact_id: number;
    feedback: string;
    reference_selection?: ReferenceSelection | null;
  }) => invokeCommand<AgentRunSummary>(V2_COMMANDS.startRevisionRun, { input }),
  cancelAgentRun: (runId: number) =>
    invokeCommand<AgentRunSummary>(V2_COMMANDS.cancelAgentRun, { runId }),
  getAgentRun: (runId: number) =>
    invokeCommand<AgentRunSummary>(V2_COMMANDS.getAgentRun, { runId }),
  listRunEvents: (runId: number, afterSequence = 0) =>
    invokeCommand<RunEvent[]>(V2_COMMANDS.listRunEvents, { runId, afterSequence }),
  subscribeRunEvents: async (
    projectId: number,
    onEvent: (event: RunEvent) => void,
  ): Promise<() => void> => {
    if (isTauriRuntime()) {
      return listen<RunEvent>("agent-run-event", ({ payload }) => {
        if (payload.project_id === projectId) onEvent(payload);
      });
    }
    const source = new EventSource(
      `${DEV_API_BASE}/events/agent-runs?project_id=${encodeURIComponent(projectId)}`
    );
    const listener = (message: MessageEvent<string>) => {
      try {
        onEvent(JSON.parse(message.data) as RunEvent);
      } catch {
        // Ignore malformed development events; command responses still carry errors.
      }
    };
    source.addEventListener("run_event", listener as EventListener);
    return () => {
      source.removeEventListener("run_event", listener as EventListener);
      source.close();
    };
  },
  getActiveAgentRun: (projectId: number) =>
    invokeCommand<ActiveAgentRun | null>(V2_COMMANDS.getActiveAgentRun, { projectId }),
  listArtifactSummaries: (filters: {
    project_id: number;
    stage?: Stage | null;
    chapter_id?: number | null;
  }) => invokeCommand<ArtifactSummary[]>(V2_COMMANDS.listArtifactSummaries, { filters }),
  listIndexJobs: (projectId: number) =>
    invokeCommand<V2DerivedIndexJob[]>(V2_COMMANDS.listIndexJobs, { projectId }),
  listLegacyAgentPrompts: () =>
    invokeCommand<LegacyAgentPrompt[]>(V2_COMMANDS.listLegacyAgentPrompts),
  listActionProposals: (input: { project_id: number; status?: string | null }) =>
    invokeCommand<ActionProposal[]>(V2_COMMANDS.listActionProposals, { input }),
  applyActionProposal: (input: { project_id: number; proposal_id: number; note?: string }) =>
    invokeCommand<ProposalApplyResult>(V2_COMMANDS.applyActionProposal, { input }),
  rejectActionProposal: (input: { project_id: number; proposal_id: number; note?: string }) =>
    invokeCommand<ActionProposal>(V2_COMMANDS.rejectActionProposal, { input }),
  rebuildChapterMemory: (input: { project_id: number; chapter_id: number }) =>
    invokeCommand<ChapterMemoryRecord>("rebuild_chapter_memory", { input }),
  rebuildStoryIndex: (input: { project_id: number; chapter_id?: number | null }) =>
    invokeCommand<StoryIndexSummary[]>("rebuild_story_index", { input }),
  retryIndexJobs: (input: { project_id: number; chapter_id?: number | null }) =>
    invokeCommand<V2DerivedIndexJob[]>("retry_index_jobs", { input }),
  rebuildStorySearchIndex: (input: { project_id: number }) =>
    invokeCommand<StorySearchStatus>("rebuild_story_search_index", { input }),
  getStorySearchStatus: (projectId: number) =>
    invokeCommand<StorySearchStatus>("get_story_search_status", { projectId }),
  runStoryArchitect: (input: {
    project_id: number;
    mode: StoryArchitectMode;
    arc_id?: number | null;
    user_instruction?: string | null;
    source_artifact_id?: number | null;
    reference_selection?: ReferenceSelection | null;
  }) => invokeCommand<AgentStepResult>("run_story_architect", { input }),
  createTargetedRework: (input: {
    project_id: number;
    mode: StoryArchitectMode;
    arc_id?: number | null;
    user_instruction?: string | null;
    source_artifact_id?: number | null;
    reference_selection?: ReferenceSelection | null;
  }) => invokeCommand<AgentStepResult>("create_targeted_rework", { input }),
  confirmStoryBible: (input: { project_id: number; note: string }) =>
    invokeCommand<StoryBible>("confirm_story_bible", { input }),
  reviewStoryBible: (input: { project_id: number }) =>
    invokeCommand<StoryBibleReview>("review_story_bible", { input }),
  confirmStoryBibleReview: (input: { project_id: number; review_id: number; note: string }) =>
    invokeCommand<StoryBibleReview>("confirm_story_bible_review", { input }),
  listStoryArcs: (projectId: number) => invokeCommand<StoryArc[]>("list_story_arcs", { projectId }),
  previewAgentContext: (input: {
    project_id: number;
    stage: Stage;
    chapter_id?: number | null;
    user_instruction?: string | null;
    source_artifact_id?: number | null;
    reference_selection?: ReferenceSelection | null;
  }) => invokeCommand<ContextPreview>("preview_agent_context", { input }),
  approveStage: (projectId: number, stage: Stage, artifactId: number, note?: string) =>
    invokeCommand<Approval>("approve_stage", {
      projectId,
      stage,
      artifactId,
      note: note || ""
    }),
  requestRevision: (input: {
    project_id: number;
    artifact_id: number;
    feedback: string;
    reference_selection?: ReferenceSelection | null;
  }) =>
    invokeCommand<AgentStepResult>("request_revision", { input }),
  replaceArtifactSpan: (input: SpanReplacementInput) =>
    invokeCommand<AgentStepResult>("replace_artifact_span", { input }),
  reviseArtifactSpanWithAi: (input: AiSpanRevisionInput) =>
    invokeCommand<AgentStepResult>("revise_artifact_span_with_ai", { input }),
  deleteArtifact: (input: DeleteArtifactInput) =>
    invokeCommand<void>("delete_artifact", { input }),
  clearChapterHistory: (input: ClearChapterHistoryInput) =>
    invokeCommand<HistoryCleanupResult>("clear_chapter_history", { input }),
  listArtifacts: (filters: { project_id: number; stage?: Stage | null; chapter_id?: number | null }) =>
    invokeCommand<Artifact[]>("list_artifacts", { filters }),
  exportProject: (projectId: number) =>
    invokeCommand<string>("export_project", { projectId, format: "markdown" }),
  analyzeArtifactQuality: (projectId: number, artifactId: number) =>
    invokeCommand<QualityReport>("analyze_artifact_quality", { projectId, artifactId }),
  analyzeChapterGate: (input: { project_id: number; chapter_id: number; artifact_id: number }) =>
    invokeCommand<ChapterGateReport>("analyze_chapter_gate", { input }),
  generateChapterSplitPlan: (input: { project_id: number; chapter_id: number; artifact_id: number }) =>
    invokeCommand<ChapterSplitPlan>("generate_chapter_split_plan", { input }),
  reviewProjectContinuity: (input: {
    project_id: number;
    chapter_ids?: number[] | null;
    candidate_artifact_id?: number | null;
    candidate_artifact_ids?: number[] | null;
  }) =>
    invokeCommand<ContinuityReport>("review_project_continuity", { input }),
  checkArtifactLedgerContinuity: (input: { project_id: number; artifact_id: number }) =>
    invokeCommand<LedgerContinuityReport>("check_artifact_ledger_continuity", { input }),
  searchStoryContext: (input: {
    project_id: number;
    chapter_id?: number | null;
    query: string;
    limit?: number | null;
    include_immediate_previous?: boolean;
  }) =>
    invokeCommand<StoryContextSnippet[]>("search_story_context", { input }),
  rerankStoryContext: (input: {
    project_id: number;
    chapter_id?: number | null;
    query: string;
    include_immediate_previous?: boolean;
    stage?: Stage | null;
    task_context?: string | null;
  }) => invokeCommand<StoryContextRerankResult>("rerank_story_context", { input })
};
