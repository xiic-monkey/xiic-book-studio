import { invoke } from "@tauri-apps/api/core";
import type {
  AiSpanRevisionInput,
  AgentStepResult,
  AiSettings,
  Artifact,
  Chapter,
  ChapterGateReport,
  ChapterSplitPlan,
  ChapterUpdate,
  ClearChapterHistoryInput,
  DeleteArtifactInput,
  ContinuityReport,
  Foreshadowing,
  HistoryCleanupResult,
  KnowledgeCard,
  NewProject,
  NewChapter,
  ModelInfo,
  Project,
  ProjectUpdate,
  ProjectDetail,
  QualityReport,
  ReviewIssue,
  SpanReplacementInput,
  SaveWritingSkill,
  SaveForeshadowingInput,
  SaveKnowledgeCardInput,
  Stage,
  StoryContextSnippet,
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
  createChapter: (input: NewChapter) => invokeCommand<Chapter>("create_chapter", { input }),
  deleteChapter: (projectId: number, chapterId: number) =>
    invokeCommand<void>("delete_chapter", { projectId, chapterId }),
  updateChapter: (input: ChapterUpdate) => invokeCommand<Chapter>("update_chapter", { input }),
  getProject: (projectId: number) => invokeCommand<ProjectDetail>("get_project", { projectId }),
  saveAiSettings: (input: {
    base_url: string;
    model: string;
    temperature: number;
    thinking_enabled: boolean;
    api_key?: string | null;
  }) => invokeCommand<AiSettings>("save_ai_settings", { input }),
  listWritingSkills: () => invokeCommand<WritingSkill[]>("list_writing_skills"),
  saveWritingSkill: (input: SaveWritingSkill) =>
    invokeCommand<WritingSkill>("save_writing_skill", { input }),
  saveKnowledgeCard: (input: SaveKnowledgeCardInput) =>
    invokeCommand<KnowledgeCard>("save_knowledge_card", { input }),
  saveForeshadowing: (input: SaveForeshadowingInput) =>
    invokeCommand<Foreshadowing>("save_foreshadowing", { input }),
  getSettings: () => invokeCommand<AiSettings>("get_settings"),
  testAiConnection: (input?: {
    base_url?: string | null;
    model?: string | null;
    temperature?: number | null;
    thinking_enabled?: boolean | null;
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
  }) => invokeCommand<AgentStepResult>("run_agent_step", { input }),
  approveStage: (projectId: number, stage: Stage, artifactId: number, note?: string) =>
    invokeCommand("approve_stage", {
      projectId,
      stage,
      artifactId,
      note: note || ""
    }),
  requestRevision: (input: { project_id: number; artifact_id: number; feedback: string }) =>
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
  analyzeArtifactQuality: (artifactId: number) =>
    invokeCommand<QualityReport>("analyze_artifact_quality", { artifactId }),
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
  searchStoryContext: (input: {
    project_id: number;
    chapter_id?: number | null;
    query: string;
    limit?: number | null;
    include_immediate_previous?: boolean;
  }) =>
    invokeCommand<StoryContextSnippet[]>("search_story_context", { input })
};
