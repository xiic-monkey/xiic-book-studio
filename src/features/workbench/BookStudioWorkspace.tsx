import {
  AlertCircle,
  BarChart3,
  Box,
  BookOpen,
  CalendarDays,
  CalendarPlus,
  Check,
  Copy,
  ChevronLeft,
  ChevronRight,
  Edit3,
  Download,
  Eye,
  FileText,
  Loader2,
  Rows3,
  MessageSquare,
  PenLine,
  Send,
  SlidersHorizontal,
  Play,
  Plus,
  RefreshCcw,
  Save,
  Search,
  Settings,
  Sparkles,
  Trash2,
  Users,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, KeyboardEvent, PointerEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../../api";
import { ArtifactDiffPanel } from "../../components/ArtifactDiffPanel";
import { ContinuityLibraryPanel } from "../../components/ContinuityLibraryPanel";
import type { EntityTimelineEntry, StoryIndexStatus } from "../../components/ContinuityLibraryPanel";
import { KnowledgeSectionCard, parseKnowledgeSections } from "../../components/KnowledgeSectionCard";
import { Select } from "../../components/Select";
import type {
  AdoptionProposal,
  ActionProposal,
  ActiveAgentRun,
  AiProvider,
  AiSettings,
  Agent,
  AgentToolDefinition,
  Artifact,
  CanonIssue,
  ChapterGateReport,
  ChapterSplitPlan,
  Chapter,
  Foreshadowing,
  KnowledgeCard,
  LedgerContinuityReport,
  NewProject,
  Project,
  ProjectUpdate,
  ContinuityReport,
  ProjectWorkspace,
  QualityReport,
  ReferenceMaterial,
  ReferenceSelection,
  ReferenceTag,
  ReviewIssue,
  RunEvent,
  SaveWritingSkill,
  SaveAiProvider,
  SaveForeshadowingInput,
  SaveKnowledgeCardInput,
  Stage,
  StoryContextSnippet,
  StoryContextRerankResult,
  StoryArchitectMode,
  StoryEntity,
  StoryEvent,
  StoryEventParticipant,
  StoryFact,
  StoryIndexSource,
  StorySearchStatus,
  WritingSkill,
  PreparedContext,
  AgentRunSummary,
} from "../../types";
import { asStage } from "../../types";
import { NewProjectModal } from "../../components/NewProjectModal";
import { ProjectEditorModal } from "../../components/ProjectEditorModal";
import { SettingsView } from "../../components/SettingsView";
import { DropdownMenu } from "../../components/DropdownMenu";
import { AdoptionDrawer } from "../../components/AdoptionDrawer";
import { AgentRunInspector } from "../agent-runs/AgentRunInspector";
import { useActionProposals } from "../proposals/useActionProposals";
import { useArtifact } from "./useArtifact";
import { projectWorkspaceQueryKey, useProjectWorkspace } from "./useProjectWorkspace";

const foundationStages: Array<{ id: Stage; label: string; scope: "book" }> = [
  { id: "setting", label: "世界观", scope: "book" },
  { id: "outline", label: "大纲", scope: "book" },
  { id: "characters", label: "角色", scope: "book" },
];

const productionStages: Array<{ id: Stage; label: string; scope: "chapter" }> = [
  { id: "draft", label: "写作", scope: "chapter" },
  { id: "review", label: "试读", scope: "chapter" },
  { id: "revision", label: "修订", scope: "chapter" },
];

const stages = [...foundationStages, ...productionStages];

const bodyStages: Stage[] = ["revision", "draft"];

const architectModeByStage: Record<"setting" | "outline" | "characters", StoryArchitectMode> = {
  setting: "refine_canon",
  outline: "plan_current_arc",
  characters: "design_characters",
};

const architectModeLabel: Record<StoryArchitectMode, string> = {
  initialize: "初始化创作基准",
  refine_canon: "补充设定",
  plan_current_arc: "细化当前阶段",
  extend_next_arc: "扩展下一阶段",
  design_characters: "补充角色",
};

function artifactStageForArchitectMode(mode: StoryArchitectMode): LibrarySection {
  if (mode === "initialize" || mode === "refine_canon") return "setting";
  if (mode === "plan_current_arc" || mode === "extend_next_arc") return "outline";
  return "characters";
}

function resolveArchitectMode(value: string): StoryArchitectMode {
  return ["initialize", "refine_canon", "plan_current_arc", "extend_next_arc", "design_characters"].includes(value)
    ? value as StoryArchitectMode
    : "refine_canon";
}

const defaultProject: NewProject = {
  title: "未命名小说",
  genre: "都市异能",
  target_words: 300000,
  premise: "一个被低估的人在危机中获得改变命运的机会。",
};

const defaultSettings: AiSettings = {
  base_url: "https://api.deepseek.com",
  model: "deepseek-v4-pro",
  temperature: 0.75,
  thinking_enabled: false,
  thinking_level: "off",
  has_api_key: false,
};

type ViewMode = "main" | "settings";
type MainSurface = "official" | "workbench" | "library";
type ContentSurface = "official" | "workbench";
type LibrarySection = "setting" | "outline" | "characters";
type LibraryFocus = LibrarySection | "items" | "events" | "foreshadowing";
type SettingsCategory = "ai" | "agents" | "skills" | "editor" | "data" | "appearance";
type AgentRunMode = "smart" | "fresh";
type AssistantChatMessage = { id: string; role: "user" | "assistant"; content: string };
type AssistantThinkingRound = { id: string; content: string; active: boolean };
type AssistantToolTimelineItem = {
  id: string;
  toolKey: string;
  status: "running" | "success" | "failed" | "rejected";
  elapsedMs?: number | null;
  summary?: string;
};
const SIDEBAR_WIDTH_STORAGE_KEY = "book-studio.sidebar-width";
const SIDEBAR_COLLAPSED_STORAGE_KEY = "book-studio.sidebar-collapsed";
const SIDEBAR_DEFAULT_WIDTH = 280;
const SIDEBAR_COLLAPSED_WIDTH = 52;
const SIDEBAR_MIN_WIDTH = 220;
const SIDEBAR_MAX_WIDTH = 420;
const MAX_REFERENCE_FILE_BYTES = 20 * 1024 * 1024;

function decodeReferenceText(buffer: ArrayBuffer) {
  const bytes = new Uint8Array(buffer);
  const utf8 = new TextDecoder("utf-8").decode(bytes).replace(/^\uFEFF/, "");
  if (!utf8.includes("\uFFFD")) return utf8;
  try {
    return new TextDecoder("gb18030").decode(bytes).replace(/^\uFEFF/, "");
  } catch {
    return utf8;
  }
}

function referenceTagLabel(tag: ReferenceTag) {
  return tag === "style" ? "文风" : "结构/内容";
}

function clampSidebarWidth(width: number) {
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, width));
}

const assistantToolLabels: Record<string, string> = {
  story_context_search: "检索故事资料",
  prepare_agent_context: "准备创作上下文",
  check_continuity: "检查连续性",
  create_artifact: "生成候选版本",
  get_current_artifact: "读取当前版本",
  get_story_bible: "读取创作基准",
  get_chapter_context: "读取当前章节",
  search_story: "检索故事内容",
};

function assistantToolLabel(toolKey: string) {
  return assistantToolLabels[toolKey] ?? toolKey.replace(/[_-]+/g, " ");
}

function assistantToolTimeline(events: RunEvent[]): AssistantToolTimelineItem[] {
  const items: AssistantToolTimelineItem[] = [];
  for (const event of events) {
    if (!event.tool_key) continue;
    if (event.kind === "tool_started") {
      items.push({
        id: `tool-${event.sequence}`,
        toolKey: event.tool_key,
        status: "running",
      });
      continue;
    }
    if (event.kind !== "tool_completed") continue;
    const target = [...items].reverse().find((item) => item.toolKey === event.tool_key && item.status === "running");
    if (target) {
      target.status = event.status === "success" ? "success" : event.status === "rejected" ? "rejected" : "failed";
      target.elapsedMs = event.elapsed_ms;
      target.summary = event.delta || event.error || undefined;
    } else {
      items.push({
        id: `tool-${event.sequence}`,
        toolKey: event.tool_key,
        status: event.status === "success" ? "success" : event.status === "rejected" ? "rejected" : "failed",
        elapsedMs: event.elapsed_ms,
        summary: event.delta || event.error || undefined,
      });
    }
  }
  return items;
}

function buildThinkingRounds(events: RunEvent[]): AssistantThinkingRound[] {
  return events.reduce(applyThinkingEvent, [] as AssistantThinkingRound[]);
}

function applyThinkingEvent(
  rounds: AssistantThinkingRound[],
  event: RunEvent,
): AssistantThinkingRound[] {
  if (event.kind === "thinking_start") {
    return [...rounds.map((round) => ({ ...round, active: false })), {
      id: `thinking-${event.sequence}`,
      content: "",
      active: true,
    }];
  }
  if (event.kind === "thinking_end") {
    return rounds.map((round) => ({ ...round, active: false }));
  }
  if (event.kind !== "thinking_delta") return rounds;
  const current = rounds.length > 0 ? rounds[rounds.length - 1] : {
    id: `thinking-${event.sequence}`,
    content: "",
    active: true,
  };
  return [
    ...rounds.slice(0, -1),
    { ...current, content: `${current.content}${event.delta}` },
  ];
}

function downloadMarkdownFile(markdown: string, projectTitle: string) {
  const safeTitle = projectTitle.trim().replace(/[<>:"/\\|?*\u0000-\u001F]/g, "_").slice(0, 80) || "book";
  const blob = new Blob([markdown], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `${safeTitle}.md`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

function resolveChapterBody(detail: ProjectWorkspace | null, chapter: Chapter | null) {
  if (!detail || !chapter) return null;
  const currentBody = detail.artifacts.find((artifact) => artifact.id === chapter.current_artifact_id);
  if (currentBody) return currentBody;

  const chapterBodies = detail.artifacts
    .filter((artifact) => artifact.chapter_id === chapter.id)
    .filter((artifact) => bodyStages.includes(artifact.stage))
    .sort((a, b) => {
      const approvalDelta = Number(b.status === "approved") - Number(a.status === "approved");
      if (approvalDelta !== 0) return approvalDelta;
      const stageDelta = bodyStages.indexOf(a.stage) - bodyStages.indexOf(b.stage);
      if (stageDelta !== 0) return stageDelta;
      return b.version - a.version;
    });

  return chapterBodies[0] ?? null;
}

export function BookStudioWorkspace() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState<number | null>(null);
  const queryClient = useQueryClient();
  const projectWorkspaceQuery = useProjectWorkspace(selectedProjectId);
  const detail = projectWorkspaceQuery.data ?? null;
  const [selectedChapterId, setSelectedChapterId] = useState<number | null>(null);
  const [selectedStage, setSelectedStage] = useState<Stage>("setting");
  const [selectedArtifactId, setSelectedArtifactId] = useState<number | null>(null);
  const [explicitArchitectSourceId, setExplicitArchitectSourceId] = useState<number | null>(null);
  const [newProject, setNewProject] = useState<NewProject>(defaultProject);
  const [projectDraft, setProjectDraft] = useState<ProjectUpdate | null>(null);
  const [settings, setSettings] = useState<AiSettings>(defaultSettings);
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [agentCatalog, setAgentCatalog] = useState<Agent[]>([]);
  const [agentTools, setAgentTools] = useState<AgentToolDefinition[]>([]);
  const [storySearchStatus, setStorySearchStatus] = useState<StorySearchStatus | null>(null);
  const [writingSkills, setWritingSkills] = useState<WritingSkill[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [instruction, setInstruction] = useState("");
  const [assistantMessages, setAssistantMessages] = useState<AssistantChatMessage[]>([]);
  const [liveToolEvents, setLiveToolEvents] = useState<RunEvent[]>([]);
  const [thinkingRounds, setThinkingRounds] = useState<AssistantThinkingRound[]>([]);
  const [revisionFeedback, setRevisionFeedback] = useState("");
  const [patchFindText, setPatchFindText] = useState("");
  const [patchReplaceText, setPatchReplaceText] = useState("");
  const [aiPatchInstruction, setAiPatchInstruction] = useState("");
  const [approvalNote, setApprovalNote] = useState("");
  const [reviewIssues, setReviewIssues] = useState<ReviewIssue[]>([]);
  const [qualityReport, setQualityReport] = useState<QualityReport | null>(null);
  const [continuityReport, setContinuityReport] = useState<ContinuityReport | null>(null);
  const [ledgerContinuityReport, setLedgerContinuityReport] = useState<LedgerContinuityReport | null>(null);
  const [chapterGateReport, setChapterGateReport] = useState<ChapterGateReport | null>(null);
  const [chapterSplitPlan, setChapterSplitPlan] = useState<ChapterSplitPlan | null>(null);
  const [streamingRun, setStreamingRun] = useState<ActiveAgentRun | null>(null);
  const [lastAgentRun, setLastAgentRun] = useState<AgentRunSummary | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exportText, setExportText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [contextQuery, setContextQuery] = useState("");
  const [contextSnippets, setContextSnippets] = useState<StoryContextSnippet[]>([]);
  const [contextRerank, setContextRerank] = useState<StoryContextRerankResult | null>(null);
  const [contextPreview, setContextPreview] = useState<PreparedContext | null>(null);
  const [showAdoptionDrawer, setShowAdoptionDrawer] = useState(false);
  const [referenceMaterials, setReferenceMaterials] = useState<ReferenceMaterial[]>([]);
  const [referenceSelections, setReferenceSelections] = useState<Record<string, ReferenceSelection>>({});
  const {
    proposals: actionProposals,
    error: actionProposalError,
    merge: mergeActionProposals,
    invalidate: invalidateActionProposals,
  } = useActionProposals(selectedProjectId);

  const [viewMode, setViewMode] = useState<ViewMode>("main");
  const [mainSurface, setMainSurface] = useState<MainSurface>("official");
  const [libraryOriginSurface, setLibraryOriginSurface] = useState<ContentSurface>("official");
  const [librarySection, setLibrarySection] = useState<LibrarySection>("setting");
  const [libraryFocus, setLibraryFocus] = useState<LibraryFocus>("setting");
  const [libraryMode, setLibraryMode] = useState<ContentSurface>("workbench");
  const [selectedLibraryEntityId, setSelectedLibraryEntityId] = useState<number | null>(null);
  const [showKnowledgeComposer, setShowKnowledgeComposer] = useState(false);
  const [knowledgeTitle, setKnowledgeTitle] = useState("");
  const [knowledgeContent, setKnowledgeContent] = useState("");
  const [knowledgeCategory, setKnowledgeCategory] = useState("world");
  const [editingKnowledgeCardId, setEditingKnowledgeCardId] = useState<number | null>(null);
  const [showForeshadowingComposer, setShowForeshadowingComposer] = useState(false);
  const [foreshadowingTitle, setForeshadowingTitle] = useState("");
  const [foreshadowingContent, setForeshadowingContent] = useState("");
  const [foreshadowingPayoffNote, setForeshadowingPayoffNote] = useState("");
  const [foreshadowingPayoffChapterId, setForeshadowingPayoffChapterId] = useState<number | null>(null);
  const [editingForeshadowingId, setEditingForeshadowingId] = useState<number | null>(null);
  const [showNewProjectModal, setShowNewProjectModal] = useState(false);
  const [showProjectEditor, setShowProjectEditor] = useState(false);
  const [projectPendingDeletion, setProjectPendingDeletion] = useState<Project | null>(null);
  const [settingsCategory, setSettingsCategory] = useState<SettingsCategory>("ai");
  const [chapterDraft, setChapterDraft] = useState("");
  const [compareArtifactId, setCompareArtifactId] = useState<number | null>(null);
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    if (typeof window === "undefined") return SIDEBAR_DEFAULT_WIDTH;
    const raw = window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY);
    const parsed = Number(raw);
    return Number.isFinite(parsed)
      ? clampSidebarWidth(parsed)
      : SIDEBAR_DEFAULT_WIDTH;
  });
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    if (typeof window === "undefined") return false;
    return window.localStorage.getItem(SIDEBAR_COLLAPSED_STORAGE_KEY) === "true";
  });
  const [sidebarResizing, setSidebarResizing] = useState(false);
  const activeProjectRequestRef = useRef<number | null>(null);
  const activeAgentRunIdRef = useRef<number | null>(null);
  const referenceFileInputRef = useRef<HTMLInputElement | null>(null);

  function currentContentSurface(): ContentSurface {
    return mainSurface === "library" ? libraryMode : mainSurface;
  }

  function switchContentSurface(surface: ContentSurface) {
    if (mainSurface === "library") {
      setLibraryOriginSurface(surface);
      setLibraryMode(surface);
      setMainSurface("library");
      return;
    }
    setMainSurface(surface);
    setLibraryMode(surface);
  }

  function enterWorkbench() {
    // 资料页的“创作工作台”应进入对应的资料工具，而不是跳到章节编辑器。
    // 章节正文页才进入 draft/review/revision 流水线。
    if (mainSurface === "library") {
      setLibraryOriginSurface("workbench");
      setLibraryMode("workbench");
      if (libraryFocus === "setting" || libraryFocus === "outline" || libraryFocus === "characters") {
        setSelectedChapterId(null);
        setSelectedStage(librarySection);
        setSelectedArtifactId(null);
        openKnowledgeEditor();
      }
      return;
    }
    switchContentSurface("workbench");
  }

  function exitLibrary() {
    switchContentSurface(libraryOriginSurface);
  }

  function openLibrary(focus?: LibraryFocus, mode?: ContentSurface) {
    const nextMode = mode ?? currentContentSurface();
    setLibraryOriginSurface(nextMode);
    setLibraryMode(nextMode);
    const section = focus && foundationStages.some((stage) => stage.id === focus)
      ? focus as LibrarySection
      : null;
    if (section) {
      setLibraryFocus(section);
      setLibrarySection(section);
      setSelectedStage(section);
      setSelectedArtifactId(null);
      setSelectedChapterId(null);
    } else {
      setLibraryFocus(focus ?? "foreshadowing");
    }
    setSelectedLibraryEntityId(null);
    resetKnowledgeComposer();
    setMainSurface("library");
  }

  useEffect(() => {
    void refreshProjects();
    void refreshProviders();
    void refreshAgents();
    void refreshAgentTools();
    void refreshWritingSkills();
  }, []);

  useEffect(() => {
    if (!selectedProjectId) return;
    activeProjectRequestRef.current = selectedProjectId;
    void api.getActiveAgentRun(selectedProjectId)
      .then((run) => {
        if (activeProjectRequestRef.current !== selectedProjectId) return;
        setStreamingRun(run);
        activeAgentRunIdRef.current = run?.id ?? null;
        if (!run) return;
        void api.listRunEvents(run.id)
          .then((events) => {
            if (activeProjectRequestRef.current !== selectedProjectId) return;
            const historical = events.filter((event) => event.kind === "tool_started" || event.kind === "tool_completed");
            const thinkingEvents = events.filter((event) => ["thinking_start", "thinking_delta", "thinking_end"].includes(event.kind));
            setLiveToolEvents((current) => {
              const merged = new Map<number, RunEvent>();
              [...historical, ...current]
                .filter((event) => event.run_id === run.id)
                .forEach((event) => merged.set(event.sequence, event));
              return [...merged.values()].sort((a, b) => a.sequence - b.sequence);
            });
            setThinkingRounds(buildThinkingRounds(thinkingEvents));
          })
          .catch(() => {
            // The live event stream remains authoritative when historical loading is unavailable.
          });
      })
      .catch((err) => {
        if (activeProjectRequestRef.current === selectedProjectId) setError(String(err));
      });
    void refreshReferenceMaterials(selectedProjectId);
  }, [selectedProjectId]);

  useEffect(() => {
    if (projectWorkspaceQuery.error) setError(String(projectWorkspaceQuery.error));
  }, [projectWorkspaceQuery.error]);

  useEffect(() => {
    if (detail) setSettings(detail.settings);
  }, [detail]);

  useEffect(() => {
    if (actionProposalError) setError(String(actionProposalError));
  }, [actionProposalError]);

  useEffect(() => {
    if (!selectedProjectId) return;
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void api.subscribeRunEvents(selectedProjectId, (event) => {
      if (activeProjectRequestRef.current !== event.project_id) return;
      if (event.kind === "tool_started" || event.kind === "tool_completed") {
        if (activeAgentRunIdRef.current == null) activeAgentRunIdRef.current = event.run_id;
        if (activeAgentRunIdRef.current !== event.run_id) return;
        setLiveToolEvents((current) => [...current, event]);
        return;
      }
      if (["thinking_start", "thinking_delta", "thinking_end"].includes(event.kind)) {
        if (activeAgentRunIdRef.current == null) activeAgentRunIdRef.current = event.run_id;
        if (activeAgentRunIdRef.current !== event.run_id) return;
        setThinkingRounds((current) => applyThinkingEvent(current, event));
        return;
      }
      if (event.kind === "started" || event.kind === "output_delta" || event.kind === "output_reset" || event.kind === "cancellation_requested") {
        activeAgentRunIdRef.current = event.run_id;
      }
      if (["completed", "failed", "cancelled"].includes(event.kind)) {
        setStreamingRun((current) => current?.id === event.run_id ? null : current);
        void hydrateFinishedAgentRun(event);
        return;
      }
      setStreamingRun((current) => {
        if (event.kind === "output_reset") {
          return current?.id === event.run_id && current
            ? { ...current, output: "", status: event.status, error: event.error }
            : current;
        }
        if (event.kind === "started" || event.kind === "output_delta" || event.kind === "cancellation_requested") {
          const sameRun = current?.id === event.run_id;
          return {
            id: event.run_id,
            project_id: event.project_id,
            chapter_id: event.chapter_id,
            stage: event.stage || current?.stage || "draft",
            output: `${sameRun ? current.output : ""}${event.delta}`,
            status: event.status,
            error: event.error,
            elapsed_ms: sameRun ? current.elapsed_ms : 0,
            created_at: sameRun ? current.created_at : event.created_at,
          };
        }
        return current;
      });
    }).then((stop) => {
      if (disposed) stop();
      else unsubscribe = stop;
    }).catch(() => {
      // The command result remains authoritative if the live event transport is unavailable.
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [selectedProjectId]);

  useEffect(() => {
    setContextPreview(null);
  }, [selectedProjectId, selectedChapterId, selectedStage, selectedArtifactId]);

  useEffect(() => {
    if (!notice || busy || error) return;
    const timer = window.setTimeout(() => setNotice(null), 3000);
    return () => window.clearTimeout(timer);
  }, [notice, busy, error]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(sidebarWidth));
  }, [sidebarWidth]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    window.localStorage.setItem(SIDEBAR_COLLAPSED_STORAGE_KEY, String(sidebarCollapsed));
  }, [sidebarCollapsed]);

  useEffect(() => {
    if (!detail) return;
    // 资料工具页不应自动选中第一章；章节选择只属于正文工作台。
    if (mainSurface === "library") return;
    const selectedExists = detail.chapters.some((chapter) => chapter.id === selectedChapterId);
    if (selectedExists) return;
    const firstChapter = detail.chapters[0];
    if (!firstChapter) return;

    const hasApprovedFoundation = detail.artifacts.some(
      (artifact) =>
        artifact.stage === "setting" &&
        (artifact.status === "approved" || detail.approvals.some((approval) => approval.artifact_id === artifact.id))
    );
    selectChapter(firstChapter, hasApprovedFoundation ? "draft" : "setting");
  }, [detail, mainSurface, selectedChapterId]);

  const selectedChapter = useMemo(
    () => detail?.chapters.find((chapter) => chapter.id === selectedChapterId) ?? null,
    [detail, selectedChapterId]
  );

  const referenceScopeKey = `${selectedProjectId ?? 0}:${selectedChapterId ?? 0}`;
  const activeReferenceSelection = useMemo<ReferenceSelection>(
    () => referenceSelections[referenceScopeKey] ?? {
      enabled: true,
      source_ids: null,
      tags: ["style", "structure"],
    },
    [referenceScopeKey, referenceSelections]
  );

  useEffect(() => {
    setContextPreview(null);
  }, [instruction, activeReferenceSelection]);

  const enabledReferenceMaterials = useMemo(
    () => referenceMaterials.filter((material) => material.enabled),
    [referenceMaterials]
  );

  const selectedReferenceIds = useMemo(
    () => new Set(
      (activeReferenceSelection.source_ids ?? enabledReferenceMaterials.map((material) => material.id))
        .filter((id) => enabledReferenceMaterials.some((material) => material.id === id))
    ),
    [activeReferenceSelection.source_ids, enabledReferenceMaterials]
  );

  const visibleArtifacts = useMemo(() => {
    if (!detail) return [];
    const stageMeta = stages.find((stage) => stage.id === selectedStage);
    // 章节体（草稿/修订）同属"正文演进"，合并展示以便跨阶段对比 diff
    const stagesToShow =
      stageMeta?.scope === "chapter" && bodyStages.includes(selectedStage)
        ? bodyStages
        : [selectedStage];
    return detail.artifacts
      .filter((artifact) => stagesToShow.includes(artifact.stage))
      .filter((artifact) =>
        stageMeta?.scope === "chapter" ? artifact.chapter_id === selectedChapterId : artifact.chapter_id == null
      )
      .sort((a, b) => b.version - a.version);
  }, [detail, selectedStage, selectedChapterId]);

  const selectedArtifactSummary = useMemo(() => {
    return visibleArtifacts.find((artifact) => artifact.id === selectedArtifactId) ?? visibleArtifacts[0] ?? null;
  }, [visibleArtifacts, selectedArtifactId]);

  const selectedArtifactQuery = useArtifact(selectedProjectId, selectedArtifactSummary?.id);
  const selectedArtifact = selectedArtifactQuery.data ?? null;

  // 试读产物指向被审的源产物（草稿），用于在试读页并排展示原文
  const reviewSourceSummary = useMemo(() => {
    if (!selectedArtifact || selectedArtifact.stage !== "review" || selectedArtifact.parent_artifact_id == null) return null;
    return detail?.artifacts.find((artifact) => artifact.id === selectedArtifact.parent_artifact_id) ?? null;
  }, [detail, selectedArtifact]);
  const reviewSourceQuery = useArtifact(selectedProjectId, reviewSourceSummary?.id);
  const reviewSourceArtifact = reviewSourceQuery.data ?? null;

  const libraryArtifactSummary = useMemo(() => {
    if (!detail) return null;
    const sectionArtifacts = detail.artifacts.filter(
      (artifact) => artifact.stage === librarySection && artifact.chapter_id == null,
    );
    if (libraryMode === "official") {
      const approved = sectionArtifacts.filter((artifact) => artifact.status === "approved");
      return approved.sort((a, b) => b.version - a.version)[0] ?? null;
    }
    return sectionArtifacts.sort((a, b) => b.version - a.version)[0] ?? null;
  }, [detail, librarySection, libraryMode]);

  const libraryArtifactQuery = useArtifact(selectedProjectId, libraryArtifactSummary?.id);
  const libraryArtifact = libraryArtifactQuery.data ?? null;

  const libraryArtifactApproved = useMemo(() => {
    if (!detail || !libraryArtifactSummary) return false;
    return (
      libraryArtifactSummary.status === "approved" ||
      detail.approvals.some((approval) => approval.artifact_id === libraryArtifactSummary.id)
    );
  }, [detail, libraryArtifactSummary]);

  const libraryArtifactLabel = useMemo(() => {
    if (!libraryArtifactSummary) {
      return libraryMode === "official"
        ? "暂无正式资料"
        : "暂无资料";
    }
    return libraryArtifactApproved
      ? `基准 v${libraryArtifactSummary.version} · 已确认`
      : `候选 v${libraryArtifactSummary.version} · 待确认`;
  }, [libraryArtifactSummary, libraryArtifactApproved, libraryMode]);

  const currentChapterBodySummary = useMemo(() => {
    if (!detail || !selectedChapter?.current_artifact_id) return null;
    return detail.artifacts.find((artifact) => artifact.id === selectedChapter.current_artifact_id) ?? null;
  }, [detail, selectedChapter]);

  const currentChapterBodyQuery = useArtifact(selectedProjectId, currentChapterBodySummary?.id);
  const currentChapterBody = currentChapterBodyQuery.data ?? null;

  const librarySourceSections = useMemo(
    () => (libraryArtifact ? parseKnowledgeSections(libraryArtifact.content) : []),
    [libraryArtifact]
  );

  const libraryCards = useMemo(() => {
    if (!detail) return [];
    const categoryMatch = (card: KnowledgeCard) => {
      if (librarySection === "characters") return card.category === "character";
      if (librarySection === "outline") return card.category === "outline" || card.category === "chapter_plan";
      return ["world", "cultivation", "map", "faction", "taboo", "item", "rule"].includes(card.category);
    };
    const modeMatch = (card: KnowledgeCard) =>
      libraryMode === "official" ? card.status === "approved" : card.status !== "archived";
    return (detail.knowledge_cards ?? []).filter((card) => categoryMatch(card) && modeMatch(card));
  }, [detail, librarySection, libraryMode]);

  const visibleForeshadowings = useMemo(
    () => detail?.foreshadowings?.filter((item) =>
      libraryMode === "official"
        ? ["active", "ready_for_payoff", "resolved"].includes(item.status)
        : item.status !== "archived"
    ) ?? [],
    [detail, libraryMode]
  );

  const timelineEntityKind = libraryFocus === "characters"
    ? "character"
    : libraryFocus === "items"
      ? null
      : undefined;

  const visibleTimelineEntities = useMemo(() => {
    const entities = detail?.story_entities ?? [];
    if (timelineEntityKind === "character") return entities.filter((entity) => entity.kind === "character");
    if (libraryFocus === "items") return entities.filter((entity) => entity.kind === "item" || entity.kind === "resource");
    return [];
  }, [detail, libraryFocus, timelineEntityKind]);

  useEffect(() => {
    if (libraryFocus !== "characters" && libraryFocus !== "items") return;
    if (visibleTimelineEntities.some((entity) => entity.id === selectedLibraryEntityId)) return;
    setSelectedLibraryEntityId(visibleTimelineEntities[0]?.id ?? null);
  }, [libraryFocus, selectedLibraryEntityId, visibleTimelineEntities]);

  const selectedLibraryEntity = useMemo(
    () => visibleTimelineEntities.find((entity) => entity.id === selectedLibraryEntityId) ?? null,
    [visibleTimelineEntities, selectedLibraryEntityId]
  );

  const chapterNumbers = useMemo(
    () => new Map((detail?.chapters ?? []).map((chapter) => [chapter.id, chapter.chapter_no])),
    [detail]
  );

  const participantsByEvent = useMemo(() => {
    const map = new Map<number, StoryEventParticipant[]>();
    for (const participant of detail?.story_event_participants ?? []) {
      const current = map.get(participant.event_id) ?? [];
      current.push(participant);
      map.set(participant.event_id, current);
    }
    return map;
  }, [detail]);

  const selectedEntityFacts = useMemo(() => {
    if (!selectedLibraryEntity) return [];
    return (detail?.story_facts ?? [])
      .filter((fact) => fact.entity_id === selectedLibraryEntity.id)
      .sort((left, right) => (chapterNumbers.get(left.narrative_chapter_id ?? 0) ?? 0) - (chapterNumbers.get(right.narrative_chapter_id ?? 0) ?? 0) || left.id - right.id);
  }, [chapterNumbers, detail, selectedLibraryEntity]);

  const selectedEntityEvents = useMemo(() => {
    if (!selectedLibraryEntity) return [];
    const factEventIds = new Set(selectedEntityFacts.map((fact) => fact.event_id).filter((id): id is number => id != null));
    return (detail?.story_events ?? [])
      .filter((event) => factEventIds.has(event.id) || (participantsByEvent.get(event.id) ?? []).some((participant) => participant.entity_id === selectedLibraryEntity.id))
      .sort((left, right) => (chapterNumbers.get(left.narrative_chapter_id ?? 0) ?? 0) - (chapterNumbers.get(right.narrative_chapter_id ?? 0) ?? 0) || left.id - right.id);
  }, [chapterNumbers, detail, participantsByEvent, selectedEntityFacts, selectedLibraryEntity]);

  const selectedEntityCurrentFacts = useMemo(() => {
    const latest = new Map<string, StoryFact>();
    for (const fact of selectedEntityFacts) latest.set(fact.dimension, fact);
    return [...latest.values()].sort((left, right) => left.dimension.localeCompare(right.dimension));
  }, [selectedEntityFacts]);

  const selectedEntityTimeline = useMemo(() => {
    const events = selectedEntityEvents.map((event) => ({
      type: "event" as const,
      id: event.id,
      chapterId: event.narrative_chapter_id ?? null,
      event,
    }));
    const facts = selectedEntityFacts.map((fact) => ({
      type: "fact" as const,
      id: fact.id,
      chapterId: fact.narrative_chapter_id ?? null,
      fact,
    }));
    return [...events, ...facts].sort((left, right) => {
      const chapterDelta = (chapterNumbers.get(left.chapterId ?? 0) ?? 0) - (chapterNumbers.get(right.chapterId ?? 0) ?? 0);
      if (chapterDelta !== 0) return chapterDelta;
      if (left.type !== right.type) return left.type === "event" ? -1 : 1;
      return left.id - right.id;
    });
  }, [chapterNumbers, selectedEntityEvents, selectedEntityFacts]);

  const storyIndexStatus = useMemo(() => {
    const approvedChapters = (detail?.chapters ?? []).filter((chapter) => chapter.current_artifact_id != null);
    const sourceByChapter = new Map<number, StoryIndexSource>();
    for (const source of detail?.story_index_sources ?? []) {
      sourceByChapter.set(source.chapter_id, source);
    }
    const currentSources = approvedChapters
      .map((chapter) => ({ chapter, source: sourceByChapter.get(chapter.id) }))
      .filter(({ chapter, source }) => source?.source_artifact_id === chapter.current_artifact_id);
    const succeeded = currentSources.filter(({ source }) => source?.status === "success").length;
    const failed = currentSources.filter(({ source }) => source?.status === "failed");
    const running = (detail?.index_jobs ?? []).filter(
      (job) => job.job_type === "story_chapter" && job.status === "running",
    ).length;
    return {
      approved: approvedChapters.length,
      succeeded,
      pending: approvedChapters.length - succeeded - failed.length,
      running,
      failed,
    };
  }, [detail]);

  const hasActiveIndexJobs = useMemo(
    () => (detail?.index_jobs ?? []).some((job) => job.status === "pending" || job.status === "running"),
    [detail],
  );

  useEffect(() => {
    if (!selectedProjectId || !hasActiveIndexJobs) return;
    let active = true;
    const interval = window.setInterval(() => {
      if (!active) return;
      void api.listIndexJobs(selectedProjectId)
        .then((jobs) => {
          if (!active || activeProjectRequestRef.current !== selectedProjectId) return;
          queryClient.setQueryData<ProjectWorkspace>(
            projectWorkspaceQueryKey(selectedProjectId),
            (current) => current ? { ...current, index_jobs: jobs } : current,
          );
        })
        .catch(() => {
          // The next lightweight poll can recover from a transient status failure.
        });
    }, 2000);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [hasActiveIndexJobs, queryClient, selectedProjectId]);

  const activeStoryArc = useMemo(
    () => detail?.story_arcs?.find((arc) => arc.status === "active") ?? null,
    [detail]
  );

  useEffect(() => {
    if (!detail || !selectedChapter) return;

    const selectedStillExists =
      selectedArtifactId == null || detail.artifacts.some((artifact) => artifact.id === selectedArtifactId);
    if (selectedArtifactSummary && selectedStillExists) return;
    if (!bodyStages.includes(selectedStage) && selectedArtifactId == null) return;

    const body = resolveChapterBody(detail, selectedChapter);
    if (!body) return;
    if (body.stage !== selectedStage) setSelectedStage(body.stage);
    if (body.id !== selectedArtifactId) setSelectedArtifactId(body.id);
  }, [detail, selectedChapter, selectedArtifactId, selectedArtifactSummary, selectedStage]);

  const compareArtifactSummary = useMemo(() => {
    if (!compareArtifactId) return null;
    return visibleArtifacts.find((artifact) => artifact.id === compareArtifactId) ?? null;
  }, [visibleArtifacts, compareArtifactId]);

  const compareArtifactQuery = useArtifact(selectedProjectId, compareArtifactSummary?.id);
  const compareArtifact = compareArtifactQuery.data ?? null;

  useEffect(() => {
    const loadError = selectedArtifactQuery.error
      ?? libraryArtifactQuery.error
      ?? currentChapterBodyQuery.error
      ?? compareArtifactQuery.error;
    if (loadError) setError(`产物正文加载失败：${String(loadError)}`);
  }, [
    compareArtifactQuery.error,
    currentChapterBodyQuery.error,
    libraryArtifactQuery.error,
    selectedArtifactQuery.error,
  ]);

  const selectedReviewIssues = useMemo(() => {
    if (!selectedArtifact || selectedArtifact.stage !== "review") return [];
    try {
      const parsed = JSON.parse(selectedArtifact.content);
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }, [selectedArtifact]);

  const selectedArtifactApproved = useMemo(() => {
    if (!detail || !selectedArtifact) return false;
    return (
      selectedArtifact.status === "approved" ||
      detail.approvals.some((approval) => approval.artifact_id === selectedArtifact.id)
    );
  }, [detail, selectedArtifact]);

  const selectedArtifactProposals = useMemo(
    () => detail?.adoption_proposals?.filter((proposal) => proposal.source_artifact_id === selectedArtifact?.id) ?? [],
    [detail, selectedArtifact]
  );

  const selectedArtifactSupportsAdoption = Boolean(
    selectedArtifactApproved
      && selectedArtifact
      && ["setting", "outline", "characters", "draft", "revision"].includes(selectedArtifact.stage)
  );

  const selectedArtifactIsCurrentBody =
    Boolean(selectedChapter?.current_artifact_id && selectedArtifact) &&
    selectedChapter?.current_artifact_id === selectedArtifact?.id;

  const selectedArtifactDeleteBlockReason = useMemo(() => {
    if (!selectedArtifact) return "未选择版本";
    if (selectedArtifact.chapter_id != null) {
      return selectedArtifactIsCurrentBody ? "当前正式正文不能删除" : null;
    }
    return null;
  }, [selectedArtifact, selectedArtifactIsCurrentBody]);

  const gateArtifact = useMemo(() => {
    if (!detail || !chapterGateReport) return null;
    return detail.artifacts.find((artifact) => artifact.id === chapterGateReport.artifact_id) ?? null;
  }, [detail, chapterGateReport]);

  const qualityArtifact = useMemo(() => {
    if (!detail || !qualityReport) return null;
    return detail.artifacts.find((artifact) => artifact.id === qualityReport.artifact_id) ?? null;
  }, [detail, qualityReport]);

  const selectedArtifactSupportsLocalPatch = Boolean(
    selectedArtifact &&
      ["setting", "outline", "characters", "draft", "revision"].includes(selectedArtifact.stage)
  );
  const selectedBookArtifactCanIterate = Boolean(
    selectedArtifact &&
      explicitArchitectSourceId === selectedArtifact.id &&
      (selectedStage === "setting" || selectedStage === "outline" || selectedStage === "characters") &&
      selectedArtifact.stage === selectedStage &&
      selectedArtifact.chapter_id == null
  );

  const chapterBodyCandidates = useMemo(() => {
    if (!detail || !selectedChapter) return [];
    return detail.artifacts
      .filter((artifact) => artifact.chapter_id === selectedChapter.id)
      .filter((artifact) => bodyStages.includes(artifact.stage))
      .sort((a, b) => {
        const currentDelta =
          Number(b.id === selectedChapter.current_artifact_id) -
          Number(a.id === selectedChapter.current_artifact_id);
        if (currentDelta !== 0) return currentDelta;
        const approvalDelta = Number(b.status === "approved") - Number(a.status === "approved");
        if (approvalDelta !== 0) return approvalDelta;
        return b.id - a.id;
      });
  }, [detail, selectedChapter]);

  const filteredProjects = useMemo(() => {
    if (!searchQuery.trim()) return projects;
    const q = searchQuery.toLowerCase();
    return projects.filter(
      (p) => p.title.toLowerCase().includes(q) || p.genre.toLowerCase().includes(q)
    );
  }, [projects, searchQuery]);

  const visibleMessages = useMemo(() => {
    if (!detail) return [];
    return detail.messages
      .filter((message) => message.chapter_id == null || message.chapter_id === selectedChapterId)
      .slice(0, 12);
  }, [detail, selectedChapterId]);

  const visibleRuns = useMemo(() => {
    if (!detail) return [];
    return detail.workflow_runs
      .filter((run) => {
        if (run.stage === "context_search_plan") {
          return run.chapter_id === selectedChapterId;
        }
        const stageMeta = stages.find((stage) => stage.id === run.stage);
        if (!stageMeta) return false;
        if (stageMeta.scope === "book") return run.chapter_id == null;
        return run.chapter_id === selectedChapterId;
      })
      .slice(0, 10);
  }, [detail, selectedChapterId]);

  const canRunReview =
    selectedArtifact?.stage === "draft" ||
    selectedArtifact?.stage === "revision" ||
    selectedStage === "review";

  const canRequestRevision =
    selectedArtifact?.stage === "draft" ||
    selectedArtifact?.stage === "revision" ||
    selectedArtifact?.stage === "review";

  const sidebarShellStyle = useMemo(
    () => ({
      width: `${sidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : sidebarWidth}px`,
      minWidth: `${sidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : sidebarWidth}px`,
    }),
    [sidebarCollapsed, sidebarWidth]
  );

  async function refreshProjects() {
    await runTask("加载项目", async () => {
      const list = await api.listProjects();
      setProjects(list);
      // A late initial response must not overwrite a project selected by the user.
      if (!activeProjectRequestRef.current) openProject(list[0]?.id ?? null);
    });
  }

  function openProject(projectId: number | null) {
    activeProjectRequestRef.current = projectId;
    setNotice(null);
    setError(null);
    setSelectedProjectId(projectId);
    setSelectedChapterId(null);
    setSelectedStage("setting");
    setSelectedArtifactId(null);
    setCompareArtifactId(null);
    setExplicitArchitectSourceId(null);
    setStreamingRun(null);
    setLastAgentRun(null);
    activeAgentRunIdRef.current = null;
    setStorySearchStatus(null);
    setQualityReport(null);
    setContinuityReport(null);
    setLedgerContinuityReport(null);
    setChapterGateReport(null);
    setChapterSplitPlan(null);
    setReviewIssues([]);
    setExportText("");
    setInstruction("");
    setAssistantMessages([]);
    setLiveToolEvents([]);
    setThinkingRounds([]);
    setRevisionFeedback("");
    setPatchFindText("");
    setPatchReplaceText("");
    setAiPatchInstruction("");
    setApprovalNote("");
    setChapterDraft("");
    setContextQuery("");
    setContextSnippets([]);
    setContextRerank(null);
    setContextPreview(null);
    setShowAdoptionDrawer(false);
    setReferenceMaterials([]);
    setReferenceSelections({});
    setProjectDraft(null);
    setSelectedLibraryEntityId(null);
    setApiKey("");
    resetKnowledgeComposer();
    resetForeshadowingComposer();
    if (!projectId) {
      setSettings(defaultSettings);
      void api.getSettings()
        .then((saved) => {
          if (activeProjectRequestRef.current === null) setSettings(saved);
        })
        .catch((err) => {
          if (activeProjectRequestRef.current === null) setError(String(err));
        });
    }
  }

  function toggleSidebarCollapsed() {
    setSidebarCollapsed((current) => !current);
  }

  function finishSidebarResize() {
    setSidebarResizing(false);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }

  function beginSidebarResize(event: PointerEvent<HTMLDivElement>) {
    if (sidebarCollapsed) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    setSidebarWidth(clampSidebarWidth(event.clientX));
    setSidebarResizing(true);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }

  function resizeSidebar(event: PointerEvent<HTMLDivElement>) {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
    setSidebarWidth(clampSidebarWidth(event.clientX));
  }

  function endSidebarResize(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    finishSidebarResize();
  }

  function resizeSidebarWithKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    const delta = event.key === "ArrowLeft" ? -16 : event.key === "ArrowRight" ? 16 : 0;
    if (!delta) return;
    event.preventDefault();
    setSidebarWidth((width) => clampSidebarWidth(width + delta));
  }

  async function refreshDetail(projectId = selectedProjectId) {
    if (!projectId) return;
    const queryKey = projectWorkspaceQueryKey(projectId);
    await queryClient.invalidateQueries({ queryKey, refetchType: "none" });
    const [, activeRun] = await Promise.all([
      queryClient.fetchQuery({
        queryKey,
        queryFn: () => api.getProject(projectId),
        staleTime: 0,
      }),
      api.getActiveAgentRun(projectId),
    ]);
    if (activeProjectRequestRef.current === projectId) setStreamingRun(activeRun);
  }

  function appendAssistantMessage(id: string, content: string) {
    setAssistantMessages((current) => {
      if (current.some((message) => message.id === id)) return current;
      return [...current, { id, role: "assistant", content }];
    });
  }

  async function hydrateFinishedAgentRun(event: RunEvent) {
    if (activeProjectRequestRef.current !== event.project_id) return;
    try {
      const summary = await api.getAgentRun(event.run_id);
      if (activeProjectRequestRef.current !== event.project_id) return;
      setLastAgentRun(summary);
      mergeActionProposals(summary.proposals);

      if (event.kind === "failed") {
        const message = event.error ?? summary.run.error ?? "Agent 运行失败，请检查配置后重试。";
        setError(message);
        appendAssistantMessage(`run-${event.run_id}-failed`, `这次${stageLabel(summary.run.stage)}任务没有完成：${message}`);
      } else if (event.kind === "cancelled") {
        setNotice("Agent 运行已取消");
        appendAssistantMessage(`run-${event.run_id}-cancelled`, `已停止${stageLabel(summary.run.stage)}任务，当前内容没有自动应用。`);
      }

      if (summary.artifact) {
        const stage = asStage(summary.artifact.stage);
        if (stage) setSelectedStage(stage);
        setSelectedArtifactId(summary.artifact.id);
        await refreshDetailBestEffort(event.project_id, "Agent 运行");
        if (summary.artifact.stage === "review" && summary.artifact.parent_artifact_id) {
          try {
            setLedgerContinuityReport(
              await api.checkArtifactLedgerContinuity({
                project_id: event.project_id,
                artifact_id: summary.artifact.parent_artifact_id,
              })
            );
          } catch {
            // Trial reading already completed. The ledger is an additional, non-blocking check.
            setLedgerContinuityReport(null);
          }
        }
        if (event.kind === "completed") {
          setNotice(`${stageLabel(summary.artifact.stage)}已生成 v${summary.artifact.version}`);
          const proposalHint = summary.proposals.length > 0
            ? `另有 ${summary.proposals.length} 条待确认提案。`
            : "结果已放入主编辑区，等待你确认或继续修订。";
          appendAssistantMessage(
            `run-${event.run_id}-completed`,
            `已生成${stageLabel(summary.artifact.stage)} v${summary.artifact.version}。${proposalHint}`,
          );
        }
      } else if (event.kind === "completed" && ["setting", "outline", "characters"].includes(summary.run.stage)) {
        const message = "故事架构生成完成，但没有返回候选资料。请检查 Agent 配置或重试。";
        setError(message);
        appendAssistantMessage(`run-${event.run_id}-completed`, message);
      } else if (event.kind === "completed") {
        appendAssistantMessage(
          `run-${event.run_id}-completed`,
          `已完成${stageLabel(summary.run.stage)}任务。结果已放入主编辑区，等待你确认或继续修订。`,
        );
      }
    } catch (error) {
      if (activeProjectRequestRef.current === event.project_id) {
        setError(`运行已结束，但详情读取失败：${String(error)}`);
        appendAssistantMessage(`run-${event.run_id}-error`, `运行结果已返回，但详情读取失败：${String(error)}`);
      }
    }
  }

  function showStartedAgentRun(result: AgentRunSummary) {
    activeAgentRunIdRef.current = result.run.id;
    setLiveToolEvents([]);
    setThinkingRounds([]);
    setLastAgentRun(result);
    setStreamingRun({
      id: result.run.id,
      project_id: result.run.project_id,
      chapter_id: result.run.chapter_id,
      stage: result.run.stage,
      output: result.run.output,
      status: result.run.status,
      error: result.run.error,
      elapsed_ms: result.run.elapsed_ms,
      created_at: result.run.created_at,
    });
  }

  async function refreshDetailBestEffort(projectId: number, operation: string) {
    try {
      await refreshDetail(projectId);
    } catch (err) {
      setError(`${operation}已完成，但详情刷新失败：${String(err)}`);
    }
  }

  async function refreshStorySearchStatusBestEffort(projectId: number, operation: string) {
    try {
      await refreshStorySearchStatus(projectId);
    } catch (err) {
      setError(`${operation}已完成，但检索状态刷新失败：${String(err)}`);
    }
  }

  async function refreshStorySearchStatus(projectId = selectedProjectId) {
    if (!projectId) {
      setStorySearchStatus(null);
      return null;
    }
    const status = await api.getStorySearchStatus(projectId);
    if (activeProjectRequestRef.current === projectId) {
      setStorySearchStatus(status);
    }
    return status;
  }

  async function refreshReferenceMaterials(projectId = selectedProjectId) {
    if (!projectId) {
      setReferenceMaterials([]);
      return;
    }
    try {
      const materials = await api.listReferenceMaterials(projectId);
      if (activeProjectRequestRef.current === projectId) {
        setReferenceMaterials(materials);
      }
    } catch (err) {
      if (activeProjectRequestRef.current === projectId) setError(String(err));
    }
  }

  function updateActiveReferenceSelection(patch: Partial<ReferenceSelection>) {
    setReferenceSelections((current) => ({
      ...current,
      [referenceScopeKey]: {
        ...activeReferenceSelection,
        ...patch,
      },
    }));
  }

  function toggleReferenceSource(referenceId: number) {
    const enabledIds = enabledReferenceMaterials.map((material) => material.id);
    const next = new Set(selectedReferenceIds);
    if (next.has(referenceId)) next.delete(referenceId);
    else next.add(referenceId);
    const nextIds = enabledIds.filter((id) => next.has(id));
    updateActiveReferenceSelection({
      source_ids: nextIds.length === enabledIds.length ? null : nextIds,
    });
  }

  function toggleReferenceTag(tag: ReferenceTag) {
    const currentTags = activeReferenceSelection.tags ?? ["style", "structure"];
    const nextTags = currentTags.includes(tag)
      ? currentTags.filter((item) => item !== tag)
      : [...currentTags, tag];
    if (nextTags.length === 0) return;
    updateActiveReferenceSelection({ tags: nextTags });
  }

  async function importReferenceFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file || !detail) return;
    if (!file.name.toLowerCase().endsWith(".txt")) {
      setError("只支持导入 .txt 文本文件");
      return;
    }
    if (file.size > MAX_REFERENCE_FILE_BYTES) {
      setError("单个 TXT 文件不能超过 20 MiB");
      return;
    }
    try {
      const content = decodeReferenceText(await file.arrayBuffer());
      await runTask("导入仿写参考", async () => {
        const material = await api.importReferenceText({
          project_id: detail.project.id,
          file_name: file.name,
          content,
          tags: ["style", "structure"],
        });
        setReferenceMaterials((current) => [...current, material]);
        setNotice(`已导入临时参考《${material.file_name}》`);
      });
    } catch (err) {
      setError(String(err));
    }
  }

  async function updateReferenceMaterial(material: ReferenceMaterial, patch: {
    enabled?: boolean;
    tags?: ReferenceTag[];
  }) {
    if (!detail) return;
    await runTask("更新仿写参考", async () => {
      const updated = await api.updateReferenceMaterial({
        project_id: detail.project.id,
        reference_id: material.id,
        ...patch,
      });
      setReferenceMaterials((current) => current.map((item) => item.id === updated.id ? updated : item));
    });
  }

  async function removeReferenceMaterial(material: ReferenceMaterial) {
    if (!detail) return;
    if (!window.confirm(`移除临时参考《${material.file_name}》？原文件不会被删除。`)) return;
    await runTask("移除仿写参考", async () => {
      await api.removeReferenceMaterial(detail.project.id, material.id);
      setReferenceMaterials((current) => current.filter((item) => item.id !== material.id));
      setReferenceSelections((current) => {
        const next = { ...current };
        for (const [key, selection] of Object.entries(next)) {
          if (!selection.source_ids) continue;
          next[key] = {
            ...selection,
            source_ids: selection.source_ids.filter((id) => id !== material.id),
          };
        }
        return next;
      });
      setNotice(`已移除临时参考《${material.file_name}》`);
    });
  }

  async function refreshWritingSkills() {
    try {
      const list = await api.listWritingSkills();
      setWritingSkills(list);
    } catch (err) {
      setError(String(err));
    }
  }

  async function refreshProviders() {
    try {
      const list = await api.listAiProviders();
      setProviders(list);
      return list;
    } catch (err) {
      setError(String(err));
      return [];
    }
  }

  async function refreshAgents() {
    try {
      const list = await api.listAgents();
      setAgentCatalog(list);
      return list;
    } catch (err) {
      setError(String(err));
      return [];
    }
  }

  async function refreshAgentTools() {
    try {
      const list = await api.listAgentTools();
      setAgentTools(list);
      return list;
    } catch (err) {
      setError(String(err));
      return [];
    }
  }

  async function runTask<T>(label: string, task: () => Promise<T>): Promise<T | null> {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      return await task();
    } catch (err) {
      setError(String(err));
      return null;
    } finally {
      setBusy(null);
    }
  }

  async function createProject() {
    const project = await runTask("新建项目", () => api.createProject(newProject));
    if (!project) return;
    setProjects((current) => [project, ...current]);
    openProject(project.id);
    setNotice("项目已创建");
    setShowNewProjectModal(false);
    setNewProject(defaultProject);
  }

  async function updateProject() {
    if (!projectDraft || !detail) return;
    const updated = await runTask("保存项目", async () => {
      const updated = await api.updateProject(projectDraft);
      setProjects((current) =>
        [...current.map((project) => (project.id === updated.id ? updated : project))].sort(
          (left, right) => right.updated_at.localeCompare(left.updated_at) || right.id - left.id,
        )
      );
      setProjectDraft({
        id: updated.id,
        title: updated.title,
        genre: updated.genre,
        target_words: updated.target_words,
        premise: updated.premise,
        status: updated.status,
      });
      await refreshDetailBestEffort(updated.id, "项目信息保存");
      return updated;
    });
    if (!updated) return;
    setShowProjectEditor(false);
    setNotice("项目信息已更新");
  }

  async function deleteProject(project: Project) {
    await runTask("删除书籍", async () => {
      const nextProjects = projects.filter((item) => item.id !== project.id);
      const fallbackProjectId = nextProjects[0]?.id ?? null;

      await api.deleteProject(project.id);
      setProjects(nextProjects);
      setProjectPendingDeletion(null);

      if (selectedProjectId === project.id) {
        openProject(fallbackProjectId);
        if (fallbackProjectId == null) {
          setProjectDraft(null);
        }
      }

      setNotice(`已删除《${project.title}》`);
    });
  }

  async function handleSaveSettings(savedSettings: AiSettings, key: string): Promise<boolean> {
    const saved = await runTask("保存设置", () => api.saveAiSettings({
      ...savedSettings,
      api_key: key.trim() || null,
    }));
    if (!saved) return false;
    setSettings(saved);
    setApiKey("");
    setNotice("AI 设置已保存");
    return true;
  }

  async function handleSaveProvider(input: SaveAiProvider): Promise<AiProvider | null> {
    const saved = await runTask("保存供应商", () => api.saveAiProvider(input));
    if (!saved) return null;
    setProviders((current) => {
      const exists = current.some((provider) => provider.id === saved.id);
      return exists
        ? current.map((provider) => (provider.id === saved.id ? saved : provider))
        : [...current, saved];
    });
    setNotice("供应商配置已保存");
    return saved;
  }

  async function handleDeleteProvider(providerId: number): Promise<boolean> {
    const deleted = await runTask("删除供应商", async () => {
      await api.deleteAiProvider(providerId);
      setProviders((current) => current.filter((provider) => provider.id !== providerId));
      const refreshedSettings = await api.getSettings();
      setSettings(refreshedSettings);
      return true;
    });
    if (deleted) setNotice("供应商配置已删除");
    return Boolean(deleted);
  }

  async function handleSaveAgentSettings(input: {
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
  }): Promise<Agent | null> {
    return runTask("保存 Agent 配置", async () => {
      const saved = await api.saveAgentSettings(input);
      setAgentCatalog((current) =>
        current.map((agent) => (agent.id === saved.id ? saved : agent))
      );
      if (detail) {
        await refreshDetailBestEffort(detail.project.id, "Agent 配置保存");
      }
      setNotice("Agent 配置已保存");
      return saved;
    });
  }

  async function handleResetAgentPrompt(agentId: number): Promise<Agent | null> {
    return runTask("恢复 Agent Prompt", async () => {
      const saved = await api.resetAgentPrompt(agentId);
      setAgentCatalog((current) =>
        current.map((agent) => (agent.id === saved.id ? saved : agent))
      );
      if (detail) {
        await refreshDetailBestEffort(detail.project.id, "Agent Prompt 恢复");
      }
      setNotice("已恢复 V2 默认 Prompt");
      return saved;
    });
  }

  async function handleTestConnection(_currentSettings: AiSettings, _key: string) {
    await runTask("测试连接", async () => {
      const result = await api.testAiConnection({
        base_url: _currentSettings.base_url,
        model: _currentSettings.model,
        temperature: _currentSettings.temperature,
        thinking_enabled: _currentSettings.thinking_enabled,
        thinking_level: _currentSettings.thinking_level,
        api_key: _key.trim() || null,
      });
      setNotice(`连接成功：${result.slice(0, 80)}`);
    });
  }

  async function handleRefreshModels(input?: { base_url?: string | null; api_key?: string | null }) {
    return api.listModels(input);
  }

  async function copyCurrentChapterBody() {
    if (!currentChapterBody) return;
    try {
      await navigator.clipboard.writeText(currentChapterBody.content);
      setNotice("已复制本章正文");
    } catch {
      const textarea = document.createElement("textarea");
      textarea.value = currentChapterBody.content;
      textarea.setAttribute("readonly", "");
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      const copied = document.execCommand("copy");
      document.body.removeChild(textarea);
      if (copied) setNotice("已复制本章正文");
      else setError("复制失败，请检查系统剪贴板权限");
    }
  }

  function selectChapter(chapter: Chapter, stage?: Stage) {
    if (mainSurface === "library") {
      switchContentSurface(libraryOriginSurface);
    }
    const currentBody = resolveChapterBody(detail, chapter);
    setSelectedChapterId(chapter.id);
    setSelectedStage(stage ?? currentBody?.stage ?? "draft");
    setSelectedArtifactId(stage ? null : currentBody?.id ?? null);
  }

  function openTimelineChapter(chapterId?: number | null) {
    const chapter = detail?.chapters.find((item) => item.id === chapterId);
    if (!chapter) return;
    const body = resolveChapterBody(detail, chapter);
    setSelectedChapterId(chapter.id);
    setSelectedStage(body?.stage ?? "draft");
    setSelectedArtifactId(body?.id ?? null);
    switchContentSurface("official");
  }

  async function rebuildLibraryIndex() {
    if (!detail) return;
    await runTask("更新资料索引", async () => {
      const jobs = await api.retryIndexJobs({ project_id: detail.project.id });
      await refreshDetailBestEffort(detail.project.id, "资料索引更新");
      const queued = jobs.filter((job) => job.status === "pending").length;
      setNotice(queued > 0 ? `索引任务已排队：${queued}` : "索引已是最新");
    });
  }

  async function rebuildStorySearchIndex() {
    if (!detail) return;
    await runTask("重建本地检索", async () => {
      const status = await api.rebuildStorySearchIndex({ project_id: detail.project.id });
      setStorySearchStatus(status);
      await refreshDetailBestEffort(detail.project.id, "本地检索重建");
      setNotice(
        status.embedding_count > 0
          ? `本地混合检索已重建：${status.document_count} 个片段`
          : `全文检索已重建：${status.document_count} 个片段`
      );
    });
  }

  async function handleSaveWritingSkill(input: SaveWritingSkill) {
    await runTask("保存技能", async () => {
      const saved = await api.saveWritingSkill(input);
      setWritingSkills((current) => {
        const exists = current.some((skill) => skill.skill_key === saved.skill_key);
        if (exists) {
          return current.map((skill) => (skill.skill_key === saved.skill_key ? saved : skill));
        }
        return [...current, saved];
      });
      setNotice("技能库已保存，下一次 Agent 运行会使用新版规则");
    });
  }

  function resetKnowledgeComposer() {
    setKnowledgeTitle("");
    setKnowledgeContent("");
    setKnowledgeCategory(librarySection === "characters" ? "character" : librarySection === "outline" ? "outline" : "world");
    setEditingKnowledgeCardId(null);
    setShowKnowledgeComposer(false);
  }

  function editKnowledgeCard(card: KnowledgeCard) {
    setKnowledgeTitle(card.title);
    setKnowledgeContent(card.content);
    setKnowledgeCategory(card.category);
    setEditingKnowledgeCardId(card.id);
    setShowKnowledgeComposer(true);
  }

  function editKnowledgeSection(section: { title: string; content: string[] }) {
    setKnowledgeTitle(section.title);
    setKnowledgeContent(section.content.join("\n"));
    setKnowledgeCategory(librarySection === "characters" ? "character" : librarySection === "outline" ? "outline" : "world");
    setEditingKnowledgeCardId(null);
    setShowKnowledgeComposer(true);
  }

  function openKnowledgeEditor() {
    const firstCard = libraryCards[0];
    if (firstCard) {
      editKnowledgeCard(firstCard);
      return;
    }
    const firstSection = librarySourceSections[0];
    if (firstSection) {
      editKnowledgeSection(firstSection);
      return;
    }
    resetKnowledgeComposer();
    setShowKnowledgeComposer(true);
  }

  function resetForeshadowingComposer() {
    setForeshadowingTitle("");
    setForeshadowingContent("");
    setForeshadowingPayoffNote("");
    setForeshadowingPayoffChapterId(null);
    setEditingForeshadowingId(null);
    setShowForeshadowingComposer(false);
  }

  function editForeshadowing(item: Foreshadowing) {
    setForeshadowingTitle(item.title);
    setForeshadowingContent(item.content);
    setForeshadowingPayoffNote(item.planned_payoff_note);
    setForeshadowingPayoffChapterId(item.planned_payoff_chapter_id ?? null);
    setEditingForeshadowingId(item.id);
    setShowForeshadowingComposer(true);
  }

  async function saveKnowledgeCard(status: "pending_human_approval" | "approved") {
    if (!detail || !knowledgeTitle.trim() || !knowledgeContent.trim()) return;
    await runTask("保存资料卡", async () => {
      const input: SaveKnowledgeCardInput = {
        id: editingKnowledgeCardId,
        project_id: detail.project.id,
        category: librarySection === "characters" ? "character" : librarySection === "outline" ? "outline" : knowledgeCategory,
        title: knowledgeTitle.trim(),
        content: knowledgeContent.trim(),
        status,
        source_artifact_id: null,
        source_chapter_id: null,
      };
      await api.saveKnowledgeCard(input);
      resetKnowledgeComposer();
      await refreshDetailBestEffort(detail.project.id, "资料卡保存");
      setNotice(status === "approved" ? "资料卡已确认并加入写作依据" : "资料卡已保存，等待人工确认");
    });
  }

  async function saveForeshadowing(status: "pending_human_approval" | "active") {
    if (!detail || !foreshadowingTitle.trim() || !foreshadowingContent.trim()) return;
    await runTask("保存伏笔", async () => {
      const input: SaveForeshadowingInput = {
        id: editingForeshadowingId,
        project_id: detail.project.id,
        title: foreshadowingTitle.trim(),
        content: foreshadowingContent.trim(),
        status,
        planted_chapter_id: selectedChapterId,
        planned_payoff_chapter_id: foreshadowingPayoffChapterId,
        planned_payoff_note: foreshadowingPayoffNote.trim(),
        source_artifact_id: selectedArtifact?.id ?? null,
      };
      await api.saveForeshadowing(input);
      resetForeshadowingComposer();
      await refreshDetailBestEffort(detail.project.id, "伏笔保存");
      setNotice(status === "active" ? "伏笔已确认并加入追踪" : "伏笔已保存，等待人工确认");
    });
  }

  async function updateKnowledgeCardStatus(card: KnowledgeCard, status: "approved" | "archived") {
    if (!detail) return;
    await runTask(status === "approved" ? "确认资料卡" : "归档资料卡", async () => {
      await api.saveKnowledgeCard({ ...card, status });
      await refreshDetailBestEffort(detail.project.id, "资料卡更新");
      setNotice(status === "approved" ? "资料卡已确认并加入写作依据" : "资料卡已归档，不再作为写作依据");
    });
  }

  async function deleteKnowledgeCard(card: KnowledgeCard) {
    if (!detail) return;
    const confirmed = window.confirm(
      `确定删除资料卡“${card.title}”吗？\n该资料卡会从项目资料中彻底移除，且不可恢复。`
    );
    if (!confirmed) return;
    await runTask("删除资料卡", async () => {
      await api.deleteKnowledgeCard({ project_id: detail.project.id, card_id: card.id });
      if (editingKnowledgeCardId === card.id) resetKnowledgeComposer();
      await refreshDetailBestEffort(detail.project.id, "资料卡删除");
      setNotice(`已删除资料卡 ${card.title}`);
    });
  }

  async function updateForeshadowingStatus(
    item: Foreshadowing,
    status: "active" | "ready_for_payoff" | "resolved" | "archived"
  ) {
    if (!detail) return;
    await runTask("更新伏笔", async () => {
      await api.saveForeshadowing({ ...item, status });
      await refreshDetailBestEffort(detail.project.id, "伏笔更新");
      setNotice(
        status === "active"
          ? "伏笔已确认追踪"
          : status === "ready_for_payoff"
            ? "伏笔已标记为可回收"
            : status === "resolved"
              ? "伏笔已标记为完成回收"
              : "伏笔已归档"
      );
    });
  }

  async function createChapter() {
    if (!detail) return;
    await runTask("新建章节", async () => {
      const chapter = await api.createChapter({
        project_id: detail.project.id,
        title: chapterDraft.trim() || null,
      });
      setChapterDraft("");
      await refreshDetailBestEffort(detail.project.id, "章节创建");
      selectChapter(chapter);
      setNotice(`已创建 ${chapter.title}`);
    });
  }

  async function deleteCurrentChapter() {
    if (!detail || !selectedChapter) return;
    const chapterTitle = selectedChapter.title;
    const confirmed = window.confirm(
      `确定删除“${chapterTitle}”吗？\n该章节的正文、试读、修订稿和运行记录都会一起删除。`
    );
    if (!confirmed) return;

    await runTask("删除章节", async () => {
      await api.deleteChapter(detail.project.id, selectedChapter.id);
      setSelectedChapterId(null);
      setSelectedArtifactId(null);
      setCompareArtifactId(null);
      setChapterDraft("");
      await refreshDetailBestEffort(detail.project.id, "章节删除");
      setNotice(`已删除 ${chapterTitle}`);
    });
  }

  async function renameCurrentChapter() {
    if (!detail || !selectedChapter) return;
    const title = chapterDraft.trim();
    if (!title) return;
    await runTask("重命名章节", async () => {
      const updated = await api.updateChapter({
        project_id: detail.project.id,
        id: selectedChapter.id,
        title,
        status: selectedChapter.status,
      });
      setSelectedChapterId(updated.id);
      setSelectedArtifactId(null);
      setChapterDraft("");
      await refreshDetailBestEffort(detail.project.id, "章节重命名");
      setNotice(`章节已重命名为 ${updated.title}`);
    });
  }

  async function continueNextChapter() {
    if (!detail) return;
    await runTask("继续下一章", async () => {
      const nextTitle = `第 ${detail.chapters.length + 1} 章`;
      const chapter = await api.createChapter({
        project_id: detail.project.id,
        title: nextTitle,
      });
      await refreshDetailBestEffort(detail.project.id, "下一章创建");
      selectChapter(chapter);
      setNotice(`已进入 ${chapter.title}`);
    });
  }

  async function handleSaveCategory(category: string) {
    await runTask(`保存设置`, async () => {
      await new Promise((r) => setTimeout(r, 300));
      setNotice("设置已保存");
    });
  }

  function redirectToStoryBibleIfDraftBlocked(stage: Stage) {
    if (stage !== "draft" || !detail) return false;

    let message: string | null = null;
    if (!detail.story_bible || detail.story_bible.status !== "confirmed") {
      message = "请先确认创作基准。";
    } else if (!activeStoryArc) {
      message = "请先确认故事阶段。";
    } else if (!detail.story_bible_review) {
      message = "请先完成一致性审校。";
    } else if (detail.story_bible_review.canon_fingerprint !== detail.canonical_fingerprint) {
      message = "创作基准已变化，请重新审校。";
    } else if (detail.story_bible_review.issues.some((issue) => issue.severity === "major")) {
      message = "一致性审校有未解决问题。";
    } else if (detail.story_bible_review.status !== "confirmed") {
      message = "请确认最新审校结论。";
    }

    if (!message) return false;
    setLibraryOriginSurface("workbench");
    setLibraryMode("workbench");
    setLibrarySection("setting");
    setLibraryFocus("setting");
    setMainSurface("library");
    setNotice(null);
    setError(message);
    return true;
  }

  function selectAssistantStage(value: string) {
    const stage = asStage(value);
    if (!stage) return;
    setSelectedStage(stage);
    setSelectedArtifactId(null);
    setNotice(`${stageLabel(stage)} Agent 已切换`);
  }

  function isCasualGreeting(prompt: string) {
    const normalized = prompt
      .trim()
      .toLowerCase()
      .replace(/[，。！？!?、,.~～\s]+/g, "");
    return ["你好", "您好", "嗨", "哈喽", "hello", "hi", "hey"].includes(normalized);
  }

  function submitAssistantPrompt() {
    const prompt = instruction.trim();
    if (!prompt) return;
    if (!detail) {
      setNotice("请先打开一本书");
      return;
    }
    setAssistantMessages((current) => [
      ...current,
      { id: `user-${Date.now()}`, role: "user", content: prompt },
    ]);
    if (isCasualGreeting(prompt)) {
      appendAssistantMessage(
        `greeting-${Date.now()}`,
        "你好！我会在你明确提出创作、修改或检查需求时再启动 Agent。",
      );
      setInstruction("");
      return;
    }
    void runAgent(selectedStage);
  }

  function useAssistantPrompt(prompt: string) {
    setInstruction(prompt);
  }

  async function runAgent(
    stage: Stage = selectedStage,
    mode: AgentRunMode = "smart",
  ) {
    if (!detail) return;
    if (streamingRun) {
      setNotice("Agent 运行中");
      return;
    }
    if (stage === "setting" || stage === "outline" || stage === "characters") {
      return runStoryArchitect(architectModeByStage[stage], mode);
    }
    if (detail.project.id !== selectedProjectId) {
      setError("项目切换中，请稍候。");
      return;
    }
    if (redirectToStoryBibleIfDraftBlocked(stage)) return;
    switchContentSurface("workbench");
    const meta = stages.find((item) => item.id === stage);
    await runTask("运行 Agent", async () => {
      const sourceArtifactId = stage === "review" ? agentSourceArtifactId(stage) : null;
      const result: AgentRunSummary = await api.startAgentRun({
        project_id: detail.project.id,
        stage,
        chapter_id: meta?.scope === "chapter" ? selectedChapterId : null,
        user_instruction: instruction.trim() || null,
        source_artifact_id: sourceArtifactId,
        reference_selection: activeReferenceSelection,
        prepared_context_id: contextPreview?.id ?? null,
      });
      showStartedAgentRun(result);
      setContextPreview(null);
      setInstruction("");
      setNotice(`${meta?.label ?? "Agent"}已开始运行`);
    });
  }

  async function runStoryArchitect(
    architectMode: StoryArchitectMode,
    runMode: AgentRunMode = "smart",
  ) {
    if (!detail) return;
    if (streamingRun) {
      setNotice("Agent 运行中");
      return;
    }
    const stage = artifactStageForArchitectMode(architectMode);
    setLibraryMode("workbench");
    setLibraryOriginSurface("workbench");
    setLibrarySection(stage);
    setLibraryFocus(stage);
    setSelectedStage(stage);
    setSelectedArtifactId(null);
    setMainSurface("library");
    await runTask("运行故事架构 Agent", async () => {
      const explicitSource = detail.artifacts.find((artifact) => artifact.id === explicitArchitectSourceId);
      const fallbackSource = libraryArtifactSummary?.stage === stage
        ? libraryArtifactSummary
        : detail.artifacts
            .filter((artifact) => artifact.stage === stage && artifact.chapter_id == null)
            .sort((a, b) => {
              const approvalDelta = Number(b.status === "approved") - Number(a.status === "approved");
              if (approvalDelta !== 0) return approvalDelta;
              return b.version - a.version;
            })[0] ?? null;
      const source = explicitSource?.stage === stage && explicitSource.chapter_id == null
        ? explicitSource
        : fallbackSource;
      const sourceArtifactId = runMode === "smart" ? source?.id ?? null : null;
      const result = await api.startStoryArchitectRun({
        project_id: detail.project.id,
        mode: architectMode,
        arc_id: activeStoryArc?.id ?? null,
        user_instruction: instruction.trim() || null,
        source_artifact_id: sourceArtifactId,
        reference_selection: activeReferenceSelection,
      });
      showStartedAgentRun(result);
      setExplicitArchitectSourceId(null);
      setInstruction("");
      setNotice(`${architectModeLabel[architectMode]}已开始运行`);
    });
  }

  async function createTargetedRework(issue: CanonIssue) {
    if (!detail) return;
    const architectMode = resolveArchitectMode(issue.owner_mode);
    const stage = artifactStageForArchitectMode(architectMode);
    setLibraryMode("workbench");
    setLibraryOriginSurface("workbench");
    setMainSurface("library");
    await runTask("定向返工故事资料", async () => {
      const result = await api.startStoryArchitectRun({
        project_id: detail.project.id,
        mode: architectMode,
        arc_id: activeStoryArc?.id ?? null,
        user_instruction: issue.rework_instruction,
        source_artifact_id: null,
        reference_selection: activeReferenceSelection,
      });
      if (!result.artifact) {
        throw new Error("定向返工已结束，但没有返回候选产物");
      }
      setLastAgentRun(result);
      mergeActionProposals(result.proposals);
      setSelectedStage(stage);
      setSelectedArtifactId(result.artifact.id);
      await refreshDetailBestEffort(detail.project.id, "定向返工");
      setNotice(`已生成针对“${issue.title}”的候选资料版本`);
    });
  }

  async function confirmStoryBible() {
    if (!detail) return;
    await runTask("确认创作基准", async () => {
      await api.confirmStoryBible({ project_id: detail.project.id, note: "" });
      await refreshDetailBestEffort(detail.project.id, "创作基准确认");
      setNotice("创作基准与当前故事阶段已确认");
    });
  }

  async function reviewStoryBible() {
    if (!detail) return;
    await runTask("审校创作基准", async () => {
      await api.reviewStoryBible({ project_id: detail.project.id });
      await refreshDetailBestEffort(detail.project.id, "创作基准审校");
      setNotice("创作基准一致性审校已生成，等待人工确认");
    });
  }

  async function confirmStoryBibleReview() {
    if (!detail?.story_bible_review) return;
    const reviewId = detail.story_bible_review.id;
    await runTask("确认一致性审校", async () => {
      await api.confirmStoryBibleReview({
        project_id: detail.project.id,
        review_id: reviewId,
        note: "",
      });
      await refreshDetailBestEffort(detail.project.id, "一致性审校确认");
      setNotice("一致性审校已确认");
    });
  }

  function agentSourceArtifactId(stage: Stage) {
    const meta = stages.find((item) => item.id === stage);
    if (stage === "review") {
      if (!selectedArtifact || selectedArtifact.chapter_id !== selectedChapterId) return null;
      if (selectedArtifact.stage === "draft" || selectedArtifact.stage === "revision") {
        return selectedArtifact.id;
      }
      if (selectedArtifact.stage === "review" && selectedArtifact.parent_artifact_id != null) {
        const parent = detail?.artifacts.find(
          (artifact) => artifact.id === selectedArtifact.parent_artifact_id
        );
        if (
          parent &&
          parent.chapter_id === selectedChapterId &&
          (parent.stage === "draft" || parent.stage === "revision")
        ) {
          return parent.id;
        }
      }
      return null;
    }
    return selectedArtifact &&
      selectedArtifact.stage === stage &&
      selectedArtifact.chapter_id === (meta?.scope === "chapter" ? selectedChapterId : null) &&
      (stage === "setting" || stage === "outline" || stage === "characters")
      ? selectedArtifact.id
      : null;
  }

  async function previewAgentContext() {
    if (!detail) return;
    if (redirectToStoryBibleIfDraftBlocked(selectedStage)) return;
    const meta = stages.find((item) => item.id === selectedStage);
    await runTask("整理生成上下文", async () => {
      const preview = await api.previewAgentRun({
        project_id: detail.project.id,
        stage: selectedStage,
        chapter_id: meta?.scope === "chapter" ? selectedChapterId : null,
        user_instruction: instruction.trim() || null,
        source_artifact_id: agentSourceArtifactId(selectedStage),
        reference_selection: activeReferenceSelection,
      });
      setContextPreview(preview);
    });
  }

  async function applyAgentProposal(proposal: ActionProposal) {
    if (!detail || proposal.project_id !== detail.project.id) return;
    if (!window.confirm(`确认应用这条 Agent 提案？\n\n${proposal.summary}`)) return;
    await runTask("应用 Agent 提案", async () => {
      await api.applyActionProposal({
        project_id: detail.project.id,
        proposal_id: proposal.id,
        note: "由用户在 Agent 运行明细中确认",
      });
      await Promise.all([
        refreshDetailBestEffort(detail.project.id, "Agent 提案应用"),
        invalidateActionProposals(),
      ]);
      setLastAgentRun((current) => current
        ? {
          ...current,
          proposals: current.proposals.map((item) =>
            item.id === proposal.id ? { ...item, status: "applied" } : item
          ),
        }
        : current);
      setNotice("Agent 提案已人工确认并应用");
    });
  }

  async function rejectAgentProposal(proposal: ActionProposal) {
    if (!detail || proposal.project_id !== detail.project.id) return;
    if (!window.confirm(`确认拒绝这条 Agent 提案？\n\n${proposal.summary}`)) return;
    await runTask("拒绝 Agent 提案", async () => {
      await api.rejectActionProposal({
        project_id: detail.project.id,
        proposal_id: proposal.id,
        note: "由用户在 Agent 运行明细中拒绝",
      });
      await invalidateActionProposals();
      setLastAgentRun((current) => current
        ? {
          ...current,
          proposals: current.proposals.map((item) =>
            item.id === proposal.id ? { ...item, status: "rejected" } : item
          ),
        }
        : current);
      setNotice("Agent 提案已拒绝");
    });
  }

  async function approveArtifact() {
    if (!detail || !selectedArtifact) return;
    if (selectedArtifactApproved) {
      setNotice("当前产物已经通过");
      return;
    }
    await runTask("审核通过", async () => {
      const approvedArtifact = selectedArtifact;
      await api.approveStage(detail.project.id, approvedArtifact.stage, approvedArtifact.id, approvalNote);
      setApprovalNote("");
      await refreshDetailBestEffort(detail.project.id, "产物通过");
      await refreshStorySearchStatusBestEffort(detail.project.id, "产物通过");
      if (approvedArtifact.stage === "draft" || approvedArtifact.stage === "revision") {
        setSelectedStage(approvedArtifact.stage);
        setSelectedArtifactId(approvedArtifact.id);
        setNotice("已通过并应用为当前正文");
      } else {
        setNotice("已记录人工确认");
      }
    });
  }

  async function approveBodyArtifact(artifact: Artifact) {
    if (!detail || !selectedChapter) return;
    const artifactApproved =
      artifact.status === "approved" ||
      detail.approvals.some((approval) => approval.artifact_id === artifact.id);
    if (artifactApproved && selectedChapter.current_artifact_id === artifact.id) return;
    await runTask("通过并应用正文", async () => {
      await api.approveStage(detail.project.id, artifact.stage, artifact.id, approvalNote);
      setApprovalNote("");
      await refreshDetailBestEffort(detail.project.id, "正文应用");
      await refreshStorySearchStatusBestEffort(detail.project.id, "正文应用");
      setSelectedChapterId(selectedChapter.id);
      setSelectedStage(artifact.stage);
      setSelectedArtifactId(artifact.id);
      setNotice(`已将 ${artifact.stage === "revision" ? "修订稿" : "草稿"} v${artifact.version} 应用为当前正文`);
    });
  }

  async function prepareArtifactAdoptions() {
    if (!detail || !selectedArtifact || !selectedArtifactSupportsAdoption) return;
    setShowAdoptionDrawer(true);
    await runTask("整理资料变更", async () => {
      const proposals = await api.prepareArtifactAdoptions({
        project_id: detail.project.id,
        artifact_id: selectedArtifact.id,
      });
      await refreshDetailBestEffort(detail.project.id, "资料整理");
      setNotice(proposals.length > 0 ? `已整理出 ${proposals.length} 条待确认资料` : "未发现可靠的资料变更");
    });
  }

  async function saveAdoptionProposal(proposalId: number, data: Record<string, unknown>) {
    if (!detail) return;
    await runTask("保存资料候选", async () => {
      await api.updateAdoptionProposal({ project_id: detail.project.id, proposal_id: proposalId, data });
      await refreshDetailBestEffort(detail.project.id, "候选保存");
      setNotice("候选已保存并重新校验");
    });
  }

  async function applyAdoptionProposals(proposalIds: number[], note: string) {
    if (!detail || proposalIds.length === 0) return;
    await runTask("采纳资料变更", async () => {
      await api.applyAdoptionProposals({
        project_id: detail.project.id,
        proposal_ids: proposalIds,
        note,
      });
      await refreshDetailBestEffort(detail.project.id, "资料变更采纳");
      setNotice(`已采纳 ${proposalIds.length} 条资料变更`);
    });
  }

  async function rejectAdoptionProposals(proposalIds: number[], note: string) {
    if (!detail || proposalIds.length === 0) return;
    await runTask("拒绝资料变更", async () => {
      await api.rejectAdoptionProposals({
        project_id: detail.project.id,
        proposal_ids: proposalIds,
        note,
      });
      await refreshDetailBestEffort(detail.project.id, "资料变更拒绝");
      setNotice(`已拒绝 ${proposalIds.length} 条资料变更`);
    });
  }

  async function requestRevision() {
    if (!detail || !selectedArtifact) return;
    if (streamingRun) {
      setNotice("Agent 运行中");
      return;
    }
    await runTask("请求修订", async () => {
      const result = await api.startRevisionRun({
        project_id: detail.project.id,
        artifact_id: selectedArtifact.id,
        feedback: revisionFeedback,
        reference_selection: activeReferenceSelection,
      });
      showStartedAgentRun(result);
      setRevisionFeedback("");
      setReviewIssues([]);
      setNotice("修订 Agent 已开始运行");
    });
  }

  async function cancelStreamingAgentRun() {
    if (!streamingRun) return;
    await runTask("停止 Agent", async () => {
      const summary = await api.cancelAgentRun(streamingRun.id);
      setLastAgentRun(summary);
      setStreamingRun((current) => current?.id === streamingRun.id
        ? { ...current, status: summary.run.status, error: summary.run.error }
        : current);
      setNotice("已发送停止请求，等待 Agent 收尾");
    });
  }

  async function replaceSelectedArtifactSpan() {
    if (!detail || !selectedArtifact) return;
    if (!selectedArtifactSupportsLocalPatch) {
      setNotice("当前产物暂不支持局部替换");
      return;
    }
    await runTask("局部替换修订", async () => {
      const result = await api.replaceArtifactSpan({
        project_id: detail.project.id,
        artifact_id: selectedArtifact.id,
        find_text: patchFindText,
        replace_text: patchReplaceText,
        note: revisionFeedback.trim() || null,
      });
      const patchStage = asStage(result.artifact.stage);
      if (patchStage) setSelectedStage(patchStage);
      setSelectedArtifactId(result.artifact.id);
      setPatchFindText("");
      setPatchReplaceText("");
      await refreshDetailBestEffort(detail.project.id, "局部替换");
      setNotice(`局部替换已生成 ${result.artifact.title} v${result.artifact.version}`);
    });
  }

  async function reviseSelectedArtifactSpanWithAi() {
    if (!detail || !selectedArtifact) return;
    if (!selectedArtifactSupportsLocalPatch) {
      setNotice("当前产物暂不支持 AI 局部改写");
      return;
    }
    await runTask("AI 局部修订", async () => {
      const result = await api.reviseArtifactSpanWithAi({
        project_id: detail.project.id,
        artifact_id: selectedArtifact.id,
        find_text: patchFindText,
        instruction: aiPatchInstruction,
      });
      const aiPatchStage = asStage(result.artifact.stage);
      if (aiPatchStage) setSelectedStage(aiPatchStage);
      setSelectedArtifactId(result.artifact.id);
      setPatchFindText("");
      setPatchReplaceText("");
      setAiPatchInstruction("");
      await refreshDetailBestEffort(detail.project.id, "AI 局部改写");
      setNotice(`AI 局部改写已生成 ${result.artifact.title} v${result.artifact.version}`);
    });
  }

  async function deleteSelectedArtifact() {
    if (!detail || !selectedArtifact) return;
    if (selectedArtifactDeleteBlockReason) {
      setError(selectedArtifactDeleteBlockReason);
      return;
    }
    const confirmed = window.confirm(
      `确定删除 ${selectedArtifact.title} · v${selectedArtifact.version} 吗？`
    );
    if (!confirmed) return;
    await runTask("删除版本", async () => {
      await api.deleteArtifact({
        project_id: detail.project.id,
        artifact_id: selectedArtifact.id,
      });
      const deletedVersion = selectedArtifact.version;
      await refreshDetailBestEffort(detail.project.id, "版本删除");
      setNotice(`已删除版本 v${deletedVersion}`);
    });
  }

  async function deleteLibrarySourceArtifact() {
    if (!detail || librarySection !== "setting") return;
    const worldArtifactVersions = detail.artifacts
      .filter((artifact) =>
        artifact.stage === "setting" &&
        artifact.chapter_id == null &&
        artifact.project_id === detail.project.id
      )
      .sort((a, b) => b.version - a.version);
    const worldKnowledgeCards = (detail.knowledge_cards ?? []).filter((card) =>
      ["world", "cultivation", "map", "faction", "taboo", "item", "rule"].includes(card.category)
    );
    const confirmed = window.confirm(
      `确定清空当前世界观的全部内容吗？\n将删除 ${worldArtifactVersions.length} 个草稿/正式版本和 ${worldKnowledgeCards.length} 张设定卡，且不可恢复。`
    );
    if (!confirmed) return;
    await runTask("清空世界观", async () => {
      for (const artifact of worldArtifactVersions) {
        await api.deleteArtifact({
          project_id: detail.project.id,
          artifact_id: artifact.id,
        });
      }
      for (const card of worldKnowledgeCards) {
        await api.deleteKnowledgeCard({
          project_id: detail.project.id,
          card_id: card.id,
        });
      }
      await refreshDetailBestEffort(detail.project.id, "候选原稿删除");
      setNotice("世界观的草稿、正式版本和设定卡已清空");
    });
  }

  async function deleteLibrarySourceSection(section: { title: string }) {
    if (!detail || !libraryArtifactSummary || !libraryArtifact) return;
    const heading = section.title.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const match = libraryArtifact.content.match(new RegExp(`^##+\\s+${heading}\\s*$[\\s\\S]*?(?=^##+\\s+|\\s*$)`, "m"));
    if (!match?.[0]) {
      setError("无法定位这张候选卡片的原文");
      return;
    }
    const confirmed = window.confirm(`确定删除候选卡片“${section.title}”吗？`);
    if (!confirmed) return;
    await runTask("删除候选卡片", async () => {
      await api.replaceArtifactSpan({
        project_id: detail.project.id,
        artifact_id: libraryArtifactSummary.id,
        find_text: match[0],
        replace_text: "",
        note: `删除候选卡片：${section.title}`,
      });
      await refreshDetailBestEffort(detail.project.id, "候选卡片删除");
      setNotice(`已删除候选卡片“${section.title}”`);
    });
  }

  async function clearSelectedChapterHistory() {
    if (!detail || !selectedChapter) return;
    const confirmed = window.confirm(
      `确定清理《${selectedChapter.title}》的历史版本吗？\n会保留正式正文和当前选中的版本，其余章节草稿/试读/修订版本会被删除。`
    );
    if (!confirmed) return;
    await runTask("清理历史", async () => {
      const result = await api.clearChapterHistory({
        project_id: detail.project.id,
        chapter_id: selectedChapter.id,
        keep_artifact_ids: selectedArtifact ? [selectedArtifact.id] : [],
      });
      await refreshDetailBestEffort(detail.project.id, "历史清理");
      setNotice(`已清理 ${result.deleted_artifact_ids.length} 个历史版本`);
    });
  }

  async function analyzeQuality() {
    if (!detail || !selectedArtifact) return;
    await runTask("质量检查", async () => {
      const report = await api.analyzeArtifactQuality(detail.project.id, selectedArtifact.id);
      setQualityReport(report);
      setNotice(`质量检查完成：${qualityVerdictLabel(report.verdict)} · ${report.score}`);
    });
  }

  async function analyzeChapterGate() {
    if (!detail || !selectedChapter || !selectedArtifact) return;
    await analyzeChapterGateForArtifact(selectedArtifact);
  }

  async function analyzeChapterGateForArtifact(artifact: Artifact) {
    if (!detail || !selectedChapter) return;
    if (artifact.stage !== "draft" && artifact.stage !== "revision") {
      setNotice("通过前检查只检查草稿或修订稿");
      return;
    }
    if (artifact.chapter_id !== selectedChapter.id) {
      setNotice("通过前检查只能检查当前章节的草稿或修订稿");
      return;
    }
    setSelectedStage(artifact.stage);
    setSelectedArtifactId(artifact.id);
    await runTask("通过前检查", async () => {
      const report = await api.analyzeChapterGate({
        project_id: detail.project.id,
        chapter_id: selectedChapter.id,
        artifact_id: artifact.id,
      });
      setChapterGateReport(report);
      setChapterSplitPlan(null);
      setQualityReport(report.quality);
      setContinuityReport(report.continuity);
      setNotice(
        report.passed
          ? `v${artifact.version} 通过前检查通过，等待人工确认`
          : `v${artifact.version} 通过前检查未通过：${report.blockers.length} 个阻断项`
      );
    });
  }

  function viewBodyArtifact(artifact: Artifact) {
    switchContentSurface("workbench");
    setSelectedStage(artifact.stage);
    setSelectedArtifactId(artifact.id);
    setNotice(`已切到 ${artifact.stage === "revision" ? "修订稿" : "草稿"} v${artifact.version}`);
  }

  async function generateSplitPlan() {
    if (!detail || !selectedChapter || !selectedArtifact) return;
    await runTask("章节重规划", async () => {
      const plan = await api.generateChapterSplitPlan({
        project_id: detail.project.id,
        chapter_id: selectedChapter.id,
        artifact_id: selectedArtifact.id,
      });
      setChapterSplitPlan(plan);
      setNotice("章节重规划方案已生成");
    });
  }

  function useSplitPlanForRevision() {
    if (!chapterSplitPlan) return;
    setRevisionFeedback(chapterSplitPlan.revision_prompt_current);
    setSelectedStage("revision");
    setNotice("已把重规划方案写入修订要求");
  }

  async function applySplitCurrentTitle() {
    if (!detail || !selectedChapter || !chapterSplitPlan) return;
    const title = chapterSplitPlan.suggested_current_title.trim();
    if (!title || title === selectedChapter.title) {
      setNotice("当前章标题无需调整");
      return;
    }
    await runTask("应用标题", async () => {
      const updated = await api.updateChapter({
        project_id: detail.project.id,
        id: selectedChapter.id,
        title,
        status: selectedChapter.status,
      });
      await refreshDetailBestEffort(detail.project.id, "标题应用");
      setSelectedChapterId(updated.id);
      setChapterDraft("");
      setNotice(`已应用标题：${updated.title}`);
    });
  }

  async function createOrOpenNextChapterFromSplit() {
    if (!detail || !selectedChapter || !chapterSplitPlan) return;
    await runTask("下一章任务", async () => {
      const existing =
        detail.chapters.find((chapter) => chapter.chapter_no === selectedChapter.chapter_no + 1) ?? null;
      const chapter =
        existing ??
        (await api.createChapter({
          project_id: detail.project.id,
          title: chapterSplitPlan.suggested_next_title.trim() || null,
        }));
      await refreshDetailBestEffort(detail.project.id, "下一章任务");
      selectChapter(chapter, "draft");
      setInstruction(chapterSplitPlan.next_chapter_instruction);
      setNotice(
        existing
          ? `已切到 ${chapter.title}，并填入下一章指令`
          : `已创建 ${chapter.title}，并填入下一章指令`
      );
    });
  }

  async function reviewContinuity() {
    if (!detail) return;
    await runTask("连续性审校", async () => {
      const chapterIds = selectedChapter
        ? detail.chapters
            .filter((chapter) => chapter.chapter_no <= selectedChapter.chapter_no)
            .map((chapter) => chapter.id)
        : detail.chapters.map((chapter) => chapter.id);
      const candidateArtifactId =
        selectedArtifact &&
        selectedArtifact.chapter_id === selectedChapterId &&
        (selectedArtifact.stage === "draft" || selectedArtifact.stage === "revision")
          ? selectedArtifact.id
          : null;
      const report = await api.reviewProjectContinuity({
        project_id: detail.project.id,
        chapter_ids: chapterIds,
        candidate_artifact_id: candidateArtifactId,
      });
      setContinuityReport(report);
      setNotice(
        candidateArtifactId
          ? `候选稿连续性审校完成：${qualityVerdictLabel(report.verdict)}`
          : `连续性审校完成：${qualityVerdictLabel(report.verdict)}`
      );
    });
  }

  async function checkLedgerContinuity() {
    if (!detail || !selectedArtifact || (selectedArtifact.stage !== "draft" && selectedArtifact.stage !== "revision")) {
      setNotice("请选择一份章节草稿或修订稿进行连续性核对");
      return;
    }
    await runTask("连续性核对", async () => {
      const report = await api.checkArtifactLedgerContinuity({
        project_id: detail.project.id,
        artifact_id: selectedArtifact.id,
      });
      setLedgerContinuityReport(report);
      setNotice(report.issues.length > 0 ? `状态账本发现 ${report.issues.length} 条直接冲突` : "状态账本未发现直接冲突");
    });
  }

  async function searchContext() {
    if (!detail || !contextQuery.trim()) return;
    await runTask("历史检索", async () => {
      const snippets = await api.searchStoryContext({
        project_id: detail.project.id,
        chapter_id: selectedChapterId,
        query: contextQuery.trim(),
        limit: 8,
        include_immediate_previous: true,
      });
      setContextSnippets(snippets);
      setContextRerank(null);
      setNotice(snippets.length > 0 ? `找到 ${snippets.length} 条历史上下文` : "没有找到相关历史上下文");
    });
  }

  async function rerankContext() {
    if (!detail || !contextQuery.trim() || contextSnippets.length === 0) return;
    await runTask("AI 筛选历史上下文", async () => {
      const result = await api.rerankStoryContext({
        project_id: detail.project.id,
        chapter_id: selectedChapterId,
        query: contextQuery.trim(),
        include_immediate_previous: true,
        stage: selectedStage,
        task_context: instruction.trim() || null,
      });
      setContextSnippets(result.candidates);
      setContextRerank(result);
      if (result.status === "fallback") {
        setNotice("AI 筛选不可用，当前显示原始候选");
      } else {
        setNotice(result.selected.length > 0 ? `AI 保留 ${result.selected.length} 条相关证据` : "AI 未保留相关证据");
      }
    });
  }

  async function exportMarkdown() {
    if (!detail) return;
    await runTask("导出", async () => {
      const markdown = await api.exportProject(detail.project.id);
      setExportText(markdown);
      try {
        await navigator.clipboard?.writeText(markdown);
      } catch {
        // Clipboard permissions are optional; the generated text remains available below.
      }
      setNotice("Markdown 已生成，并尝试复制到剪贴板");
    });
  }

  function downloadExportedMarkdown() {
    if (!detail || !exportText) return;
    downloadMarkdownFile(exportText, detail.project.title);
    setNotice("Markdown 文件已下载");
  }

  function stageStatus(stage: Stage) {
    if (!detail) return "待生成";
    const meta = stages.find((item) => item.id === stage);
    const artifacts = detail.artifacts.filter((artifact) => {
      if (artifact.stage !== stage) return false;
      return meta?.scope === "chapter" ? artifact.chapter_id === selectedChapterId : artifact.chapter_id == null;
    });
    if (artifacts.some((artifact) => detail.approvals.some((approval) => approval.artifact_id === artifact.id))) {
      return "已通过";
    }
    if (artifacts.length > 0) return "待确认";
    return "待生成";
  }

  function stageLabel(stage: string) {
    if (stage === "context_search_plan") return "上下文检索";
    return stages.find((item) => item.id === stage)?.label ?? stage;
  }

  function roleLabel(role: string) {
    switch (role) {
      case "human_instruction":
        return "人工指令";
      case "revision_feedback":
        return "修订要求";
      case "approval_note":
        return "人工确认";
      case "agent_result":
        return "Agent 结果";
      case "reference_overlap_warning":
        return "参考相似度提醒";
      default:
        return role;
    }
  }

  function qualityVerdictLabel(verdict: string) {
    switch (verdict) {
      case "strong":
        return "强";
      case "usable":
        return "可用";
      case "needs_revision":
        return "需修订";
      case "weak":
        return "弱";
      default:
        return verdict;
    }
  }

  function recommendationLabel(action: string) {
    switch (action) {
      case "approve":
        return "建议通过";
      case "revise":
        return "建议修订";
      case "split":
        return "建议重规划本章";
      default:
        return action;
    }
  }

  function formatMetricValue(value: number, unit: string) {
    if (unit === "ratio") return `${Math.round(value * 100)}%`;
    if (unit === "bool") return value >= 1 ? "有" : "无";
    if (unit === "score") return `${Math.round(value)}`;
    return `${Math.round(value)}`;
  }

  useEffect(() => {
    if (selectedArtifact?.stage === "review") {
      setReviewIssues(selectedReviewIssues);
    }
  }, [selectedArtifact, selectedReviewIssues]);

  useEffect(() => {
    if (!detail) {
      setProjectDraft(null);
      return;
    }
    setProjectDraft({
      id: detail.project.id,
      title: detail.project.title,
      genre: detail.project.genre,
      target_words: detail.project.target_words,
      premise: detail.project.premise,
      status: detail.project.status,
    });
  }, [detail]);

  useEffect(() => {
    if (!selectedArtifact) {
      setCompareArtifactId(null);
      setQualityReport(null);
      setChapterSplitPlan(null);
      return;
    }
    const fallback = visibleArtifacts.find((artifact) => artifact.id !== selectedArtifact.id) ?? null;
    setCompareArtifactId(fallback?.id ?? null);
    setQualityReport(null);
    setChapterGateReport(null);
    setChapterSplitPlan(null);
  }, [selectedArtifact, visibleArtifacts]);

  useEffect(() => {
    setContinuityReport(null);
    setChapterGateReport(null);
    setChapterSplitPlan(null);
  }, [selectedProjectId]);

  useEffect(() => {
    setChapterGateReport(null);
    setChapterSplitPlan(null);
  }, [selectedChapterId]);

  if (viewMode === "settings") {
    return (
      <SettingsView
        settings={settings}
        providers={providers}
        projectId={detail?.project.id ?? null}
        storySearchStatus={storySearchStatus}
        apiKey={apiKey}
        settingsCategory={settingsCategory}
        onSettingsCategoryChange={setSettingsCategory}
        onBack={() => setViewMode("main")}
        onSaveSettings={handleSaveSettings}
        onSaveProvider={handleSaveProvider}
        onDeleteProvider={handleDeleteProvider}
        onGetProviderCapabilities={api.getProviderCapabilities}
        onSaveAgentSettings={handleSaveAgentSettings}
        onResetAgentPrompt={handleResetAgentPrompt}
        onTestConnection={handleTestConnection}
        onRefreshModels={handleRefreshModels}
        onRefreshStorySearchStatus={refreshStorySearchStatus}
        onRebuildStorySearch={rebuildStorySearchIndex}
        agents={agentCatalog}
        agentTools={agentTools}
        genreAgent={detail?.genre_agent}
        writingSkills={writingSkills}
        onSaveWritingSkill={handleSaveWritingSkill}
        onSaveCategory={handleSaveCategory}
        busy={busy}
        notice={notice}
        error={error}
      />
    );
  }

  return (
    <main className="app-shell">
      {/* ========== Left Sidebar ========== */}
      <div
        className={sidebarCollapsed ? "sidebar-shell collapsed" : "sidebar-shell"}
        style={sidebarShellStyle}
      >
        <aside className={sidebarCollapsed ? "sidebar collapsed" : "sidebar"}>
          {sidebarCollapsed ? (
            <div className="sidebar-collapsed-rail">
              <button
                className="sidebar-toggle-btn"
                onClick={toggleSidebarCollapsed}
                title="展开书籍侧栏"
                aria-label="展开书籍侧栏"
              >
                <ChevronRight size={16} />
              </button>
            </div>
          ) : (<>
              <div className="sidebar-scroll">
                <div className="sidebar-header">
                  <div className="brand">
                    <BookOpen size={20} />
                    <div>
                      <strong>Book Studio</strong>
                      <span>AI 小说工作台</span>
                    </div>
                  </div>
                  <button
                    className="sidebar-toggle-btn"
                    onClick={toggleSidebarCollapsed}
                    title="收起书籍侧栏"
                    aria-label="收起书籍侧栏"
                  >
                    <ChevronLeft size={16} />
                  </button>
                </div>

                <div className="sidebar-actions">
                  <button className="sidebar-action" onClick={() => setShowNewProjectModal(true)} disabled={Boolean(busy)}>
                    <PenLine size={16} />
                    新建书籍
                  </button>
                </div>

                <div className="sidebar-search">
                  <Search size={14} className="sidebar-search-icon" />
                  <input
                    type="text"
                    placeholder="搜索书籍..."
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                  />
                </div>

                <div className="section-label">书籍</div>
                <div className="project-list">
                  {filteredProjects.map((project) => (
                    <div
                      className={project.id === selectedProjectId ? "project-item active" : "project-item"}
                      key={project.id}
                    >
                      <button
                        className="project-item-main"
                        onClick={() => openProject(project.id)}
                        disabled={Boolean(busy)}
                      >
                        <FileText size={14} className="project-item-icon" />
                        <div className="project-item-text">
                          <strong>{project.title}</strong>
                          <span>{project.genre || "未设置"} · 预计 {project.target_words.toLocaleString()} 字</span>
                        </div>
                      </button>
                      <button
                        className="project-delete-btn"
                        title={`删除《${project.title}》`}
                        aria-label={`删除《${project.title}》`}
                        onClick={(event) => {
                          event.stopPropagation();
                          setProjectPendingDeletion(project);
                        }}
                        disabled={busy === "删除书籍"}
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  ))}
                  {filteredProjects.length === 0 && (
                    <p className="empty-hint">{searchQuery ? "无匹配书籍" : "暂无书籍"}</p>
                  )}
                </div>
              </div>

              <footer className="sidebar-footer">
                <button className="settings-btn" onClick={() => setViewMode("settings")}>
                  <Settings size={16} />
                  设置
                </button>
              </footer>
            </>
          )}
        </aside>
        <div
          className={
            sidebarCollapsed
              ? "sidebar-resize-handle hidden"
              : sidebarResizing
                ? "sidebar-resize-handle dragging"
                : "sidebar-resize-handle"
          }
          role="separator"
          aria-label="调整书籍侧栏宽度"
          aria-orientation="vertical"
          aria-valuemin={SIDEBAR_MIN_WIDTH}
          aria-valuemax={SIDEBAR_MAX_WIDTH}
          aria-valuenow={sidebarCollapsed ? SIDEBAR_COLLAPSED_WIDTH : sidebarWidth}
          tabIndex={sidebarCollapsed ? -1 : 0}
          onPointerDown={beginSidebarResize}
          onPointerMove={resizeSidebar}
          onPointerUp={endSidebarResize}
          onPointerCancel={endSidebarResize}
          onKeyDown={resizeSidebarWithKeyboard}
        />
      </div>

      {/* ========== Workspace ========== */}
      <section className="workspace">
        <header className="topbar">
          <div className="topbar-project">
            <div className="project-title-row">
              <h1>{detail?.project.title ?? "未选择项目"}</h1>
              {detail && (
                <div className="project-tags" aria-label="书籍信息">
                  <span>{detail.project.genre || "未设置题材"}</span>
                  <span>正文 {(detail.formal_char_count ?? 0).toLocaleString()} 字</span>
                  <span>{detail.chapters.length} 章</span>
                </div>
              )}
            </div>
          </div>
          {detail && (
            <div className="surface-switch topbar-surface-switch" role="tablist" aria-label="内容区域">
              <button
                className={currentContentSurface() === "official" ? "active" : ""}
                onClick={() => switchContentSurface("official")}
              >
                <BookOpen size={14} /> 正式内容
              </button>
              <button
                className={currentContentSurface() === "workbench" ? "active" : ""}
                onClick={enterWorkbench}
              >
                <Sparkles size={14} /> 创作工作台
              </button>
            </div>
          )}
          <div className="topbar-actions">
            <button onClick={() => void runTask("刷新项目", () => refreshDetail())} disabled={!detail || Boolean(busy)}>
              <RefreshCcw size={14} /> 刷新
            </button>
            <button onClick={() => setShowProjectEditor(true)} disabled={!detail || Boolean(busy)}>
              <Edit3 size={14} /> 编辑书籍
            </button>
            <button onClick={exportMarkdown} disabled={!detail || Boolean(busy)}>
              <Download size={14} /> 导出
            </button>
          </div>
        </header>

        {(notice || error || busy) && (
          <div className={error ? "status error" : "status"}>
            {busy ? <Loader2 className="spin" size={14} /> : error ? <AlertCircle size={14} /> : <Check size={14} />}
            <span>{busy ?? error ?? notice}</span>
          </div>
        )}

        <div className="content-grid">
          {/* Left: Chapters & Stages */}
          <section className="lane">
            <div className="lane-header">
              <div className="library-shortcuts">
                <div className="section-head">
                  <h2>书籍资料</h2>
                  {currentContentSurface() === "official" && <span className="read-only-note">只读</span>}
                </div>
                {currentContentSurface() === "workbench" ? (
                  <>
                    {foundationStages.map((stage) => (
                      <button
                        key={stage.id}
                        className={mainSurface === "library" && libraryFocus === stage.id ? "active" : ""}
                        onClick={() => openLibrary(stage.id as LibrarySection, "workbench")}
                      >
                        <span>{stage.label}</span>
                      </button>
                    ))}
                  </>
                ) : (
                  <>
                    <button className={mainSurface === "library" && libraryFocus === "setting" ? "active" : ""} onClick={() => openLibrary("setting", "official")}>
                      <span>世界观</span>
                    </button>
                    <button className={mainSurface === "library" && libraryFocus === "outline" ? "active" : ""} onClick={() => openLibrary("outline", "official")}>
                      <span>大纲</span>
                    </button>
                    <button className={mainSurface === "library" && libraryFocus === "characters" ? "active" : ""} onClick={() => openLibrary("characters", "official")}>
                      <span>角色</span>
                    </button>
                    <button className={mainSurface === "library" && libraryFocus === "items" ? "active" : ""} onClick={() => openLibrary("items", "official")}>
                      <span>物品与事件</span>
                      <small>{(detail?.story_entities?.filter((entity) => entity.kind === "item" || entity.kind === "resource").length ?? 0) + (detail?.story_events?.length ?? 0)}</small>
                    </button>
                    <button className={mainSurface === "library" && libraryFocus === "foreshadowing" ? "active" : ""} onClick={() => openLibrary("foreshadowing", "official")}>
                      <span>伏笔账本</span>
                    </button>
                  </>
                )}
              </div>

              <div className="section-head">
                <h2>章节</h2>
                <DropdownMenu
                  label="管理"
                  className="lane-tools"
                  triggerClassName="lane-tools-trigger"
                  menuClassName="lane-tools-menu"
                  menuWidth={168}
                >
                  {currentContentSurface() === "workbench" && (
                    <>
                      <button onClick={createChapter} disabled={!detail || Boolean(busy)}>
                        <Plus size={14} /> 新增章节
                      </button>
                      <button onClick={continueNextChapter} disabled={!detail || Boolean(busy)}>
                        <Play size={14} /> 下一章
                      </button>
                      <button onClick={renameCurrentChapter} disabled={!selectedChapter || Boolean(busy)}>
                        <Save size={14} /> 重命名
                      </button>
                      <button onClick={clearSelectedChapterHistory} disabled={!selectedChapter || Boolean(busy)}>
                        <Trash2 size={14} /> 清历史
                      </button>
                      <input
                        value={chapterDraft}
                        onChange={(event) => setChapterDraft(event.target.value)}
                        placeholder={selectedChapter ? `当前：${selectedChapter.title}` : "新章节标题"}
                      />
                    </>
                  )}
                  <button
                    className="danger"
                    onClick={deleteCurrentChapter}
                    disabled={!selectedChapter || Boolean(busy)}
                  >
                    <Trash2 size={14} /> 删除章节
                  </button>
                </DropdownMenu>
              </div>
            </div>
            <div className="lane-scroll">
              <div className="chapter-list">
                {detail?.chapters.map((chapter) => (
                  <button
                    key={chapter.id}
                    className={mainSurface !== "library" && chapter.id === selectedChapterId ? "chapter active" : "chapter"}
                    onClick={() => selectChapter(chapter)}
                  >
                    <span>{chapter.title}</span>
                    {currentContentSurface() === "workbench" && <small>{chapter.current_artifact_id ? "正式正文" : "待创作"}</small>}
                  </button>
                ))}
                {detail && detail.chapters.length === 0 && (
                  <p className="empty-hint">暂无章节</p>
                )}
              </div>

            </div>
          </section>

          {mainSurface === "official" ? (
            <section className="editor official-editor">
              <div className="editor-toolbar">
                <div>
                  <div className="editor-title-line">
                    <h2>正式正文</h2>
                    <span className="workspace-mode-badge official">已审核 · 只读</span>
                  </div>
                  <p>{selectedChapter ? selectedChapter.title : "选择章节"}</p>
                </div>
                <div className="button-row">
                  {currentChapterBody && (
                    <button className="icon-btn" onClick={copyCurrentChapterBody} title="复制本章正文" aria-label="复制本章正文">
                      <Copy size={16} />
                    </button>
                  )}
                  <button onClick={enterWorkbench} disabled={!detail}>
                    <Sparkles size={14} /> 去工作台
                  </button>
                </div>
              </div>
              <article className="official-manuscript">
                {selectedChapter ? (
                  currentChapterBody ? (
                    <>
                      <pre>{currentChapterBody.content}</pre>
                    </>
                  ) : currentChapterBodySummary && currentChapterBodyQuery.isFetching ? (
                    <div className="official-empty">
                      <Loader2 size={18} className="spin" />
                      <span>正在加载正式正文…</span>
                    </div>
                  ) : (
                    <div className="official-empty">
                      <strong>暂无正式正文</strong>
                      <button onClick={enterWorkbench}>
                        <Sparkles size={14} /> 去工作台
                      </button>
                    </div>
                  )
                ) : (
                  <div className="official-empty">
                    <strong>未选择章节</strong>
                  </div>
                )}
              </article>
            </section>
          ) : mainSurface === "library" && ["characters", "items", "events"].includes(libraryFocus) ? (
            <ContinuityLibraryPanel
              focus={libraryFocus as "characters" | "items" | "events"}
              readOnly={libraryMode === "official"}
              detail={detail}
              busy={Boolean(busy)}
              status={storyIndexStatus}
              participantsByEvent={participantsByEvent}
              entities={visibleTimelineEntities}
              selectedEntity={selectedLibraryEntity}
              currentFacts={selectedEntityCurrentFacts}
              timeline={selectedEntityTimeline}
              onRebuild={rebuildLibraryIndex}
              onSelectEntity={setSelectedLibraryEntityId}
              onOpenEntity={(entityId, kind) => {
                openLibrary(kind === "character" ? "characters" : "items");
                setSelectedLibraryEntityId(entityId);
              }}
              onOpenChapter={openTimelineChapter}
            />
          ) : mainSurface === "library" ? (
            <section className="library-workspace">
              <header className="library-header">
                <div className="library-header-title">
                  <div>
                    <h2>{libraryFocus === "foreshadowing" ? "伏笔账本" : librarySection === "setting" ? "世界观" : librarySection === "outline" ? "大纲" : "角色"}</h2>
                  </div>
                </div>
                {libraryMode === "workbench" && (
                  <button onClick={() => (showKnowledgeComposer ? resetKnowledgeComposer() : setShowKnowledgeComposer(true))} disabled={!detail || Boolean(busy)}>
                    <Plus size={14} /> 补充资料
                  </button>
                )}
              </header>
              {libraryMode === "workbench" && libraryFocus === "setting" && <section className="story-bible-overview">
                <div className="story-bible-summary">
                  <div className="story-bible-actions">
                    {libraryMode === "workbench" ? (
                      <>
                        <button
                          className="icon-btn btn-primary tooltip-button"
                          onClick={() => runStoryArchitect(libraryArtifactSummary ? "refine_canon" : "initialize")}
                          disabled={!detail || Boolean(busy)}
                          data-tooltip={libraryArtifactSummary ? "补充世界观" : "整理世界观"}
                          aria-label={libraryArtifactSummary ? "补充世界观" : "整理世界观"}
                        >
                          <Sparkles size={14} />
                        </button>
                        <button
                          className="icon-btn tooltip-button"
                          onClick={() => runStoryArchitect("plan_current_arc")}
                          disabled={!detail || Boolean(busy)}
                          data-tooltip="细化当前阶段"
                          aria-label="细化当前阶段"
                        >
                          <Rows3 size={14} />
                        </button>
                        <button
                          className="icon-btn tooltip-button"
                          onClick={() => runStoryArchitect("extend_next_arc")}
                          disabled={!detail || Boolean(busy)}
                          data-tooltip="扩展下一阶段"
                          aria-label="扩展下一阶段"
                        >
                          <CalendarPlus size={14} />
                        </button>
                      </>
                    ) : null}
                  </div>
                </div>
                {detail?.story_arcs?.length ? (
                  <div className="story-arc-list">
                    {detail.story_arcs.map((arc) => (
                      <article key={arc.id} className={arc.status === "active" ? "story-arc active" : "story-arc"}>
                        <span>阶段 {arc.arc_no}</span><strong>{arc.title}</strong><p>{arc.objective || "等待细化阶段目标"}</p>
                      </article>
                    ))}
                  </div>
                ) : null}
                {libraryMode === "workbench" ? (
                  <div className="story-bible-review-row">
                    {!detail?.story_bible || detail.story_bible.status !== "confirmed" ? (
                      <button onClick={confirmStoryBible} disabled={!detail || Boolean(busy)}><Check size={14} /> 确认世界观</button>
                    ) : detail.story_bible_review?.status === "pending_human_confirmation" ? (
                      <button onClick={confirmStoryBibleReview} disabled={Boolean(busy)}><Check size={14} /> 确认审校结论</button>
                    ) : (
                      <button onClick={reviewStoryBible} disabled={Boolean(busy)}><Eye size={14} /> 审校一致性</button>
                    )}
                  </div>
                ) : null}
                {detail?.story_bible_review && (
                  <details className="story-bible-review" open={detail.story_bible_review.status === "pending_human_confirmation"}>
                    <summary>一致性审校 · {detail.story_bible_review.verdict} · {detail.story_bible_review.issues.length} 项</summary>
                    <p>{detail.story_bible_review.summary}</p>
                    {detail.story_bible_review.issues.map((issue, index) => (
                      <article key={`${issue.title}-${index}`} className={`canon-issue ${issue.severity}`}>
                        <strong>{issue.title}</strong><span>{issue.domain} · {issue.severity}</span>
                        <p>{issue.conflict}</p><p>{issue.impact}</p>
                        {libraryMode === "workbench" ? (
                          <button onClick={() => createTargetedRework(issue)} disabled={Boolean(busy)}><RefreshCcw size={14} /> 交给故事架构 Agent 修复</button>
                        ) : null}
                      </article>
                    ))}
                  </details>
                )}
              </section>}

              <div className="library-layout">
                <section className="library-canvas">
                  {libraryMode === "workbench" && showKnowledgeComposer && (
                    <section className="library-composer">
                      <div className="library-composer-head">
                        <strong>{editingKnowledgeCardId ? "编辑资料卡" : `补充${librarySection === "setting" ? "设定" : librarySection === "outline" ? "大纲任务" : "角色"}`}</strong>
                        <button className="icon-btn" onClick={resetKnowledgeComposer} title="关闭"><ChevronLeft size={15} /></button>
                      </div>
                      {librarySection === "setting" && (
                        <Select
                          value={knowledgeCategory}
                          onChange={setKnowledgeCategory}
                          options={[
                            ["world", "世界观"], ["cultivation", "修行体系"], ["map", "地图与地点"],
                            ["faction", "势力与组织"], ["taboo", "禁忌与边界"], ["item", "重要物件"],
                          ].map(([value, label]) => ({ value, label }))}
                        />
                      )}
                      <input value={knowledgeTitle} onChange={(event) => setKnowledgeTitle(event.target.value)} placeholder="资料标题" />
                      <textarea rows={5} value={knowledgeContent} onChange={(event) => setKnowledgeContent(event.target.value)} placeholder="资料内容" />
                      <div className="button-row">
                        <button onClick={() => saveKnowledgeCard("pending_human_approval")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>保存待确认</button>
                        <button className="btn-primary" onClick={() => saveKnowledgeCard("approved")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>确认并启用</button>
                      </div>
                    </section>
                  )}

                  <div className="knowledge-grid">
                    {libraryFocus !== "foreshadowing" && librarySourceSections.length > 0 && (
                      <details className={libraryMode === "official" ? "source-artifact-reference official-source-reference" : "source-artifact-reference"} open>
                        {libraryMode === "workbench" ? (
                          <summary>
                            <span>候选原稿（Markdown 参考）</span>
                            <span className="source-artifact-reference-meta">
                              <small>{librarySourceSections.length} 个章节 · 卡片来自同一份原稿</small>
                              {libraryArtifactSummary && (
                                <button
                                  type="button"
                                  className="icon-btn danger"
                                  onClick={(event) => {
                                    event.preventDefault();
                                    event.stopPropagation();
                                    void deleteLibrarySourceArtifact();
                                  }}
                                  title="清空全部世界观内容"
                                  aria-label="清空全部世界观内容"
                                >
                                  <Trash2 size={14} />
                                </button>
                              )}
                            </span>
                          </summary>
                        ) : <summary className="official-source-reference-summary" aria-hidden="true" />}
                        <div className="source-artifact-reference-body">
                          {librarySourceSections.map((section, index) => (
                            <article className="managed-knowledge-card source-knowledge-card" key={`${section.title}-${index}`}>
                              <div className="managed-card-head">
                                {libraryMode === "workbench" && (
                                  <div className="managed-card-actions">
                                    <button className="secondary-action" onClick={() => editKnowledgeSection(section)}>
                                      <Edit3 size={14} /> 编辑为设定卡
                                    </button>
                                    <button className="project-delete-btn" onClick={() => void deleteLibrarySourceSection(section)} title="删除候选卡片" aria-label={`删除候选卡片 ${section.title}`}>
                                      <Trash2 size={14} />
                                    </button>
                                  </div>
                                )}
                              </div>
                              <KnowledgeSectionCard section={section} />
                            </article>
                          ))}
                        </div>
                      </details>
                    )}
                    {libraryFocus === "foreshadowing" && visibleForeshadowings.map((item) => (
                      <article className="managed-knowledge-card" key={`foreshadowing-${item.id}`}>
                        <KnowledgeSectionCard section={{ title: item.title, content: item.content.split("\n") }} />
                      </article>
                    ))}
                    {libraryFocus !== "foreshadowing" && libraryCards.map((card) => (
                      <article className="managed-knowledge-card" key={card.id}>
                        <div className="managed-card-head">
                          <span className={`library-status ${card.status}`}>{card.status === "approved" ? "已确认" : card.status === "pending_human_approval" ? "待确认" : "已归档"}</span>
                          <div className="managed-card-actions">
                              {libraryMode === "workbench" && (
                              <button className="icon-btn" onClick={() => editKnowledgeCard(card)} title="编辑资料卡"><Edit3 size={14} /></button>
                              )}
                              {libraryMode === "workbench" && card.status === "pending_human_approval" && <button className="icon-btn" onClick={() => updateKnowledgeCardStatus(card, "approved")} title="确认并启用"><Check size={14} /></button>}
                              {libraryMode === "workbench" && card.status !== "archived" && <button className="icon-btn" onClick={() => updateKnowledgeCardStatus(card, "archived")} title="归档资料卡"><Trash2 size={14} /></button>}
                              <button className="icon-btn danger" onClick={() => deleteKnowledgeCard(card)} title="彻底删除资料卡"><Trash2 size={14} /></button>
                            </div>
                        </div>
                        <KnowledgeSectionCard section={{ title: card.title, content: card.content.split("\n") }} />
                      </article>
                    ))}
                    {libraryFocus === "foreshadowing" && visibleForeshadowings.length === 0 && <div className="empty-state">暂无伏笔</div>}
                    {libraryFocus !== "foreshadowing" && librarySourceSections.length === 0 && libraryCards.length === 0 && <div className="empty-state">暂无资料</div>}
                  </div>
                </section>

                {libraryMode === "workbench" && libraryFocus !== "setting" && <aside className="library-side">
                  <details className="library-side-panel foreshadowing-panel library-side-disclosure">
                    <summary className="library-side-summary">
                      <span><Sparkles size={14} /> 伏笔账本</span>
                      <small>{visibleForeshadowings.length} 条</small>
                    </summary>
                    <div className="foreshadowing-panel-body">
                      {libraryMode === "workbench" ? (
                        <button onClick={() => (showForeshadowingComposer ? resetForeshadowingComposer() : setShowForeshadowingComposer(true))} disabled={!detail || Boolean(busy)}><Plus size={14} /> 登记伏笔</button>
                      ) : (
                        <span className="read-only-note">只读</span>
                      )}
                      {libraryMode === "workbench" && showForeshadowingComposer && (
                        <div className="foreshadowing-composer">
                          <strong>{editingForeshadowingId ? "编辑伏笔" : "登记伏笔"}</strong>
                          <input value={foreshadowingTitle} onChange={(event) => setForeshadowingTitle(event.target.value)} placeholder="伏笔标题" />
                          <textarea rows={3} value={foreshadowingContent} onChange={(event) => setForeshadowingContent(event.target.value)} placeholder="伏笔内容" />
                          <Select
                            value={String(foreshadowingPayoffChapterId ?? "")}
                            onChange={(value) => setForeshadowingPayoffChapterId(Number(value) || null)}
                            options={[
                              { value: "", label: "回收章节（可选）" },
                              ...(detail?.chapters ?? []).map((chapter) => ({ value: String(chapter.id), label: chapter.title })),
                            ]}
                          />
                          <input value={foreshadowingPayoffNote} onChange={(event) => setForeshadowingPayoffNote(event.target.value)} placeholder="回收里程碑" />
                          <div className="button-row">
                            <button onClick={() => saveForeshadowing("pending_human_approval")} disabled={!foreshadowingTitle.trim() || !foreshadowingContent.trim() || Boolean(busy)}>保存</button>
                            <button className="btn-primary" onClick={() => saveForeshadowing("active")} disabled={!foreshadowingTitle.trim() || !foreshadowingContent.trim() || Boolean(busy)}>确认追踪</button>
                          </div>
                        </div>
                      )}
                      <div className="foreshadowing-list">
                        {visibleForeshadowings.map((item: Foreshadowing) => {
                          const payoffChapter = detail?.chapters.find((chapter) => chapter.id === item.planned_payoff_chapter_id);
                          const statusLabel = item.status === "active" ? "追踪中" : item.status === "ready_for_payoff" ? "可回收" : item.status === "resolved" ? "已回收" : "待确认";
                          return (
                            <article key={item.id} className="foreshadowing-item">
                              <div className="managed-card-head"><span className={`library-status ${item.status}`}>{statusLabel}</span>{libraryMode === "workbench" && <div className="managed-card-actions">
                                <button className="icon-btn" onClick={() => editForeshadowing(item)} title="编辑伏笔"><Edit3 size={14} /></button>
                                {item.status === "pending_human_approval" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "active")} title="确认追踪"><Check size={14} /></button>}
                                {item.status === "active" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "ready_for_payoff")} title="标记可回收"><Sparkles size={14} /></button>}
                                {item.status === "ready_for_payoff" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "resolved")} title="标记已回收"><Check size={14} /></button>}
                              </div>}</div>
                              <strong>{item.title}</strong><p>{item.content}</p><span>{payoffChapter?.title ?? (item.planned_payoff_note || "尚未安排回收")}</span>
                            </article>
                          );
                        })}
                        {visibleForeshadowings.length === 0 && <div className="empty-inline">还没有登记伏笔</div>}
                      </div>
                    </div>
                  </details>
                </aside>}
              </div>
            </section>) : null}
          <>
          {/* Center: Editor */}
          {mainSurface === "workbench" && (<section className="editor">
            <div className="editor-toolbar">
              <div>
                <div className="editor-title-line">
                  <h2>{stages.find((stage) => stage.id === selectedStage)?.label}</h2>
                  <span className="workspace-mode-badge draft">草稿 / 候选</span>
                </div>
                <p>{selectedChapter ? selectedChapter.title : "整书资料"}</p>
              </div>
              <div className="button-row">
                <button onClick={() => runAgent(selectedStage)} disabled={!detail || Boolean(busy)}>
                  <Play size={14} /> {selectedBookArtifactCanIterate ? "基于当前版本迭代" : selectedStage === "revision" ? "生成修订" : selectedStage === "draft" ? "生成草稿" : "生成"}
                </button>
                {selectedArtifact?.stage !== "review" && (
                  <button
                    className="btn-primary"
                    onClick={approveArtifact}
                    disabled={!selectedArtifact || selectedArtifactApproved || Boolean(busy)}
                  >
                    <Check size={14} /> {selectedArtifactApproved ? "已通过" : "审核通过"}
                  </button>
                )}
                {selectedArtifactSupportsAdoption && (
                  <button
                    onClick={() => selectedArtifactProposals.length > 0 ? setShowAdoptionDrawer(true) : prepareArtifactAdoptions()}
                    disabled={Boolean(busy)}
                  >
                    <Rows3 size={14} /> 整理资料
                    {selectedArtifactProposals.filter((proposal) => proposal.status === "pending").length > 0
                      ? ` (${selectedArtifactProposals.filter((proposal) => proposal.status === "pending").length})`
                      : ""}
                  </button>
                )}
                <DropdownMenu
                  label="更多"
                  className="toolbar-more"
                  triggerClassName="toolbar-more-trigger"
                  menuClassName="toolbar-more-menu"
                  menuWidth={168}
                  align="end"
                >
                    <button
                      onClick={() => runAgent(selectedStage, "fresh")}
                      disabled={!detail || Boolean(busy)}
                    >
                      <RefreshCcw size={14} /> {selectedBookArtifactCanIterate ? "整版重写" : selectedStage === "revision" ? "整版重写" : "重新生成"}
                    </button>
                    <button
                      onClick={deleteSelectedArtifact}
                      disabled={!selectedArtifact || Boolean(busy) || Boolean(selectedArtifactDeleteBlockReason)}
                      title={selectedArtifactDeleteBlockReason ?? "删除当前版本"}
                    >
                      <Trash2 size={14} />
                      {selectedArtifactDeleteBlockReason ?? "删除当前版本"}
                    </button>
                </DropdownMenu>
              </div>
            </div>

            <div className="workflow-strip" aria-label="章节创作流程">
              <div className="workflow-steps">
                {productionStages.map((stage, index) => (
                  <div className="workflow-step-group" key={stage.id}>
                    <button
                      type="button"
                      className={stage.id === selectedStage ? "workflow-step active" : "workflow-step"}
                      onClick={() => {
                        setSelectedStage(stage.id);
                        setSelectedArtifactId(null);
                      }}
                    >
                      <span className="workflow-step-index">{index + 1}</span>
                      <span>{stage.label}</span>
                    </button>
                    {index < productionStages.length - 1 && <ChevronRight className="workflow-step-arrow" size={14} />}
                  </div>
                ))}
              </div>
            </div>

            <details className="version-drawer">
              <summary>版本与对比 {visibleArtifacts.length > 0 ? `(${visibleArtifacts.length})` : ""}</summary>
              <div className="artifact-tabs">
                {visibleArtifacts.map((artifact) => (
                  <button
                    key={artifact.id}
                    className={artifact.id === selectedArtifact?.id ? "artifact-tab active" : "artifact-tab"}
                    onClick={() => {
                      setSelectedArtifactId(artifact.id);
                      setExplicitArchitectSourceId(
                        artifact.chapter_id == null &&
                        (artifact.stage === "setting" || artifact.stage === "outline" || artifact.stage === "characters")
                          ? artifact.id
                          : null
                      );
                    }}
                  >
                    v{artifact.version} · {stageLabel(artifact.stage)} · {artifact.status}
                    {selectedChapter?.current_artifact_id === artifact.id ? " · 当前正文" : ""}
                  </button>
                ))}
              </div>

              {selectedArtifact && visibleArtifacts.length > 1 && (
                <div className="compare-toolbar">
                  <span>对比基准</span>
                  <Select
                    value={String(compareArtifactId ?? "")}
                    onChange={(value) => setCompareArtifactId(Number(value) || null)}
                    options={[
                      { value: "", label: "不对比" },
                      ...visibleArtifacts
                      .filter((artifact) => artifact.id !== selectedArtifact.id)
                      .map((artifact) => ({ value: String(artifact.id), label: `v${artifact.version} · ${artifact.status}` })),
                    ]}
                  />
                </div>
              )}
            </details>

            <article className={streamingRun ? "artifact-view streaming-artifact" : "artifact-view"}>
              {streamingRun ? (
                <>
                  <div className="artifact-meta">
                    <strong>{stageLabel(streamingRun.stage)}正在生成</strong>
                    <span>实时输出 · 待确认</span>
                    <button
                      type="button"
                      className="secondary-action"
                      onClick={() => void cancelStreamingAgentRun()}
                      disabled={busy === "停止 Agent" || streamingRun.status === "cancellation_requested"}
                    >
                      {busy === "停止 Agent" ? <Loader2 size={14} className="spin" /> : <X size={14} />}
                      {streamingRun.status === "cancellation_requested" ? "正在停止…" : "停止生成"}
                    </button>
                  </div>
                  <pre>{streamingRun.output || "正在等待模型返回内容..."}</pre>
                </>
              ) : selectedArtifact ? (
                <>
                  <div className="artifact-meta">
                    <strong>
                      {selectedArtifact.title}
                      {selectedArtifactIsCurrentBody && <span className="current-body-badge">当前正文</span>}
                    </strong>
                    <span>{new Date(selectedArtifact.created_at).toLocaleString()}</span>
                  </div>
                  {selectedBookArtifactCanIterate && (
                    <div className="empty-inline">局部迭代模式</div>
                  )}
                  {selectedArtifact.stage === "review" && reviewSourceArtifact && (
                    <div className="artifact-meta-sub">
                      <span>被审原文 · v{reviewSourceArtifact.version}</span>
                    </div>
                  )}
                  {selectedArtifact.stage === "review" && reviewSourceArtifact ? (
                    <pre className="review-source-content">{reviewSourceArtifact.content}</pre>
                  ) : (
                    <pre>{selectedArtifact.content}</pre>
                  )}
                </>
              ) : selectedArtifactSummary && selectedArtifactQuery.isFetching ? (
                <div className="empty-state"><Loader2 size={18} className="spin" /> 正在加载产物正文…</div>
              ) : (
                <div className="empty-state">暂无产物</div>
              )}
            </article>

            {selectedArtifact && compareArtifact && selectedArtifact.id !== compareArtifact.id && (
              <ArtifactDiffPanel selectedArtifact={selectedArtifact} compareArtifact={compareArtifact} />
            )}

            {selectedArtifact?.stage === "review" && (
            <div className="review-board">
              {selectedReviewIssues.length > 0 ? selectedReviewIssues.map((issue, index) => (
                <section className="review-card" key={`${issue.location}-${index}`}>
                  <div className="review-card-head">
                    <strong>{issue.issue_type}</strong>
                    <span>{issue.severity}</span>
                    </div>
                    <p><b>位置：</b>{issue.location}</p>
                    <p><b>原因：</b>{issue.reason}</p>
                    {issue.evidence_quote && <p><b>依据：</b>{issue.evidence_quote}</p>}
                    {issue.action_evidence_quote && <p><b>动作依据：</b>{issue.action_evidence_quote}</p>}
                    <p><b>建议：</b>{issue.suggestion}</p>
                </section>
              )) : (
                <div className="empty-state compact">结果非结构化，显示原文</div>
              )}
            </div>
            )}
            {selectedArtifact?.stage === "review" &&
              ledgerContinuityReport &&
              selectedArtifact.parent_artifact_id === ledgerContinuityReport.artifact_id && (
              <section className="ledger-report" aria-label="连续性核对结果">
                <div className="ledger-report-head">
                  <strong>连续性核对</strong>
                  <span>{ledgerContinuityReport.issues.length > 0 ? `${ledgerContinuityReport.issues.length} 条需核对` : "未发现直接冲突"}</span>
                </div>
                <p>{ledgerContinuityReport.summary}</p>
                {ledgerContinuityReport.issues.map((issue, index) => (
                  <article className="ledger-issue" key={`${issue.entity_label}-${issue.candidate_quote}-${index}`}>
                    <strong>{issue.entity_label}</strong>
                    <span>{issue.severity}</span>
                    <p>{issue.reason}</p>
                    <p><b>候选稿：</b>{issue.candidate_quote}</p>
                    <p><b>{issue.source_chapter}：</b>{issue.source_quote}</p>
                    <small>{issue.suggestion}</small>
                  </article>
                ))}
              </section>
            )}
          </section>)}

          {/* Right: Agent chat is only needed while working on drafts and story materials. */}
          {mainSurface !== "official" && (
          <aside className="assistant-panel assistant-panel-v2">
            <header className="assistant-workspace-header">
              <div className="assistant-workspace-identity">
                <div className="assistant-workspace-avatar"><Sparkles size={15} /></div>
                <div>
                  <strong>Agent 工作区</strong>
                  <Select
                    className="assistant-agent-select"
                    value={selectedStage}
                    onChange={selectAssistantStage}
                    aria-label="切换当前 Agent"
                    options={stages.map((stage) => {
                      const agent = agentCatalog.find((item) => item.stage === stage.id);
                      return {
                        value: stage.id,
                        label: `${agent?.name ?? `${stage.label} Agent`} · ${stage.label}`,
                      };
                    })}
                  />
                </div>
              </div>
              <button
                type="button"
                className="assistant-new-chat"
                onClick={() => {
                  setAssistantMessages([]);
                  setInstruction("");
                  setLiveToolEvents([]);
                  setLastAgentRun(null);
                }}
                title="新建会话"
                aria-label="新建会话"
              >
                <Plus size={14} />
              </button>
            </header>

            <div className="assistant-context-strip">
              <span className={busy ? "assistant-status busy" : "assistant-status"}>
                <span className="assistant-status-dot" />
                {busy ? "Agent 执行中" : "已就绪"}
              </span>
              {selectedChapter && <span className="assistant-context-chip">章节 · {selectedChapter.title}</span>}
              <span className="assistant-context-chip">上下文自动选择</span>
            </div>

            <div className="assistant-chat-feed">
              <article className="assistant-message assistant-message-agent">
                <div className="assistant-message-avatar"><Sparkles size={13} /></div>
                <div className="assistant-message-body">
                  <div className="assistant-message-meta"><strong>Book Agent</strong><span>刚刚</span></div>
                  <p>我会围绕当前阶段协助你推进创作。你可以直接描述想改什么，也可以让我继续生成、检查连续性或整理资料。</p>
                  <div className="assistant-suggestion-list">
                    <button type="button" onClick={() => useAssistantPrompt("基于当前设定，给出下一步最值得推进的创作建议")}>下一步建议 <ChevronRight size={12} /></button>
                    <button type="button" onClick={() => useAssistantPrompt("检查当前内容是否存在角色或时间线矛盾")}>检查连续性 <ChevronRight size={12} /></button>
                  </div>
                </div>
              </article>

              {assistantMessages.map((message) => (
                <article
                  className={message.role === "user" ? "assistant-message assistant-message-user" : "assistant-message assistant-message-agent"}
                  key={message.id}
                >
                  {message.role === "assistant" && <div className="assistant-message-avatar"><Sparkles size={13} /></div>}
                  <div className="assistant-message-body">
                    {message.role === "assistant" && (
                      <div className="assistant-message-meta"><strong>Book Agent</strong><span>刚刚</span></div>
                    )}
                    <p>{message.content}</p>
                  </div>
                </article>
              ))}

              {streamingRun && (
                <article className="assistant-run-card assistant-run-card-live">
                  <div className="assistant-run-card-head">
                    <span><Loader2 size={13} className="spin" /> {stageLabel(streamingRun.stage)}正在生成</span>
                    <small>实时输出</small>
                  </div>
                  <div className="assistant-run-steps" aria-label="Agent 执行步骤">
                    {assistantToolTimeline(liveToolEvents).length > 0
                      ? assistantToolTimeline(liveToolEvents).map((tool) => (
                        <div className={`assistant-run-step assistant-run-step-${tool.status}`} key={tool.id}>
                          <span className="assistant-run-step-mark">{tool.status === "success" ? "✓" : tool.status === "running" ? "•" : "!"}</span>
                          <span>{assistantToolLabel(tool.toolKey)}{tool.summary ? ` · ${tool.summary}` : ""}</span>
                          <small>{tool.status === "success" ? `${tool.elapsedMs ?? 0} ms` : tool.status === "running" ? "执行中" : tool.status === "rejected" ? "已拒绝" : "失败"}</small>
                        </div>
                      ))
                      : [
                        { label: "准备创作上下文", status: streamingRun.output ? "done" : "active" },
                        { label: "检索相关故事资料", status: streamingRun.output ? "active" : "pending" },
                        { label: "生成候选版本", status: "pending" },
                      ].map((step) => (
                        <div className={`assistant-run-step assistant-run-step-${step.status}`} key={step.label}>
                          <span className="assistant-run-step-mark">{step.status === "done" ? "✓" : step.status === "active" ? "•" : ""}</span>
                          <span>{step.label}</span>
                          <small>{step.status === "done" ? "已完成" : step.status === "active" ? "执行中" : "等待"}</small>
                        </div>
                      ))}
                  </div>
                  <p>{streamingRun.output || "Agent 正在整理上下文并生成候选内容…"}</p>
                </article>
              )}

              {thinkingRounds.map((round) => (
                <details
                  key={round.id}
                  className={`assistant-thinking-panel${round.active ? " assistant-thinking-panel-current" : ""}`}
                  open={round.active || undefined}
                >
                  <summary>
                    <span><Sparkles size={12} /> {round.active ? "思考中" : "思考过程"}</span>
                    <small>{round.active ? "实时更新" : "已完成"}</small>
                  </summary>
                  <p>{round.content || "正在分析当前任务……"}</p>
                </details>
              ))}

              {!streamingRun && lastAgentRun && (
                <>
                  {assistantToolTimeline(liveToolEvents).length > 0 && (
                    <div className="assistant-run-steps assistant-run-steps-history">
                      {assistantToolTimeline(liveToolEvents).map((tool) => (
                        <div className={`assistant-run-step assistant-run-step-${tool.status}`} key={tool.id}>
                          <span className="assistant-run-step-mark">{tool.status === "success" ? "✓" : tool.status === "running" ? "•" : "!"}</span>
                          <span>{assistantToolLabel(tool.toolKey)}{tool.summary ? ` · ${tool.summary}` : ""}</span>
                          <small>{tool.status === "success" ? `${tool.elapsedMs ?? 0} ms` : tool.status === "running" ? "执行中" : tool.status === "rejected" ? "已拒绝" : "失败"}</small>
                        </div>
                      ))}
                    </div>
                  )}
                  <AgentRunInspector
                    run={lastAgentRun}
                    proposals={[]}
                    busy={Boolean(busy)}
                    mode="compact"
                    onApplyProposal={() => undefined}
                    onRejectProposal={() => undefined}
                  />
                  {actionProposals.length > 0 && (
                    <button type="button" className="assistant-proposal-link" onClick={() => {
                      const target = document.querySelector<HTMLElement>(".agent-run-inspector");
                      target?.scrollIntoView({ behavior: "smooth", block: "start" });
                    }}>
                      查看 {actionProposals.length} 条待确认提案 <ChevronRight size={12} />
                    </button>
                  )}
                </>
              )}
            </div>

            <div className="assistant-composer">
              <div className="assistant-composer-hint">
                <span>⌘ Enter 发送</span>
                {referenceMaterials.length > 0 && <span>{selectedReferenceIds.size} 份参考已启用</span>}
              </div>
              <textarea
                className="assistant-chat-input"
                rows={3}
                value={instruction}
                placeholder={selectedBookArtifactCanIterate ? "告诉 Agent 只改哪里，其他内容保持不变…" : "描述你想继续创作、修改或检查的内容…"}
                onChange={(event) => setInstruction(event.target.value)}
                onKeyDown={(event) => {
                  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                    event.preventDefault();
                    submitAssistantPrompt();
                  }
                }}
              />
              <div className="assistant-composer-actions">
                <div className="assistant-composer-tools">
                  <button type="button" onClick={() => setInstruction("继续推进当前阶段，给出一个可直接采用的版本")}>继续生成</button>
                  <button type="button" onClick={previewAgentContext} disabled={!detail || Boolean(busy)}><Eye size={12} /> 上下文</button>
                </div>
                <button type="button" className="assistant-send-button" onClick={submitAssistantPrompt} disabled={!detail || !instruction.trim() || Boolean(busy)}>
                  {busy ? <Loader2 size={14} className="spin" /> : <Send size={14} />}
                  {busy ? "执行中" : "发送"}
                </button>
              </div>
            </div>

            <details className="assistant-advanced-controls">
              <summary><SlidersHorizontal size={14} /> 高级控制 <span>参考资料、提案与局部修订</span></summary>
              <div className="assistant-advanced-content">
            <section className="panel next-action-panel">
              <div className="panel-title">
                <PenLine size={14} />
                创作指令
              </div>
              <div className="stage-hint">当前阶段：{stageLabel(selectedStage)}</div>
              <input
                ref={referenceFileInputRef}
                className="visually-hidden"
                type="file"
                accept=".txt,text/plain"
                onChange={importReferenceFile}
              />
              <textarea
                rows={4}
                value={instruction}
                placeholder={
                  selectedBookArtifactCanIterate
                    ? "例如：只调整第 3 节"
                    : "人工指令"
                }
                onChange={(event) => setInstruction(event.target.value)}
              />
              <section className={`reference-selection-panel${referenceMaterials.length === 0 ? " empty" : ""}`}>
                <div className="reference-selection-head">
                  <div className="reference-selection-title">
                    <BookOpen size={14} />
                    <div>
                      <strong>仿写参考</strong>
                      <span>{referenceMaterials.length > 0 ? `${selectedReferenceIds.size} / ${referenceMaterials.length} 份启用` : "未导入"}</span>
                    </div>
                  </div>
                  <div className="reference-selection-actions">
                    {referenceMaterials.length > 0 && (
                      <label title="本次生成是否使用仿写参考">
                        <input
                          type="checkbox"
                          checked={activeReferenceSelection.enabled}
                          onChange={(event) => updateActiveReferenceSelection({ enabled: event.target.checked })}
                        />
                        启用
                      </label>
                    )}
                    <button
                      type="button"
                      className="icon-btn tooltip-button"
                      onClick={() => referenceFileInputRef.current?.click()}
                      disabled={!detail || Boolean(busy)}
                      title="导入 TXT"
                      aria-label="导入 TXT"
                    >
                      <Plus size={14} />
                    </button>
                  </div>
                </div>
                {referenceMaterials.length > 0 && (
                  <div className="reference-selection-list">
                    {referenceMaterials.map((material) => (
                      <article className="reference-selection-material" key={material.id}>
                        <label title={material.enabled ? material.file_name : "资料已停用"}>
                          <input
                            type="checkbox"
                            checked={material.enabled && selectedReferenceIds.has(material.id)}
                            onChange={() => toggleReferenceSource(material.id)}
                            disabled={!material.enabled || !activeReferenceSelection.enabled}
                          />
                          <span>{material.file_name}</span>
                        </label>
                        <small>{material.char_count.toLocaleString()} 字</small>
                        <button
                          type="button"
                          className="icon-btn"
                          onClick={() => void removeReferenceMaterial(material)}
                          title="移除参考"
                          aria-label={`移除参考 ${material.file_name}`}
                          disabled={Boolean(busy)}
                        >
                          <Trash2 size={13} />
                        </button>
                        <details className="reference-material-options">
                          <summary>设置</summary>
                          <div className="reference-tag-list">
                            {(["style", "structure"] as ReferenceTag[]).map((tag) => (
                              <label key={tag}>
                                <input
                                  type="checkbox"
                                  checked={material.tags.includes(tag)}
                                  onChange={(event) => {
                                    const tags = event.target.checked
                                      ? [...material.tags, tag]
                                      : material.tags.filter((item) => item !== tag);
                                    if (tags.length > 0) void updateReferenceMaterial(material, { tags });
                                  }}
                                  disabled={Boolean(busy) || (material.tags.length === 1 && material.tags.includes(tag))}
                                />
                                {referenceTagLabel(tag)}
                              </label>
                            ))}
                          </div>
                        </details>
                      </article>
                    ))}
                  </div>
                )}
              </section>
              <button
                className="btn-primary"
                onClick={() => runAgent(selectedStage)}
                disabled={!detail || Boolean(busy)}
              >
                <Sparkles size={14} /> {selectedBookArtifactCanIterate ? "生成修订" : selectedStage === "revision" ? "生成修订" : selectedStage === "draft" ? "生成草稿" : "生成候选"}
              </button>
              <button
                className="secondary-action"
                onClick={previewAgentContext}
                disabled={!detail || Boolean(busy)}
              >
                <Eye size={14} /> 预览生成上下文
              </button>
              {contextPreview && (
                <section className="context-preview-panel">
                  <div className="context-preview-head">
                    <div>
                      <strong>{agentCatalog.find((agent) => agent.stage === contextPreview.stage)?.name ?? "当前 Agent"}将使用的上下文</strong>
                      <span>{contextPreview.total_chars.toLocaleString()} 字符 · 约 {contextPreview.estimated_tokens.toLocaleString()} tokens</span>
                    </div>
                    <button className="icon-btn" onClick={() => setContextPreview(null)} title="关闭上下文预览" aria-label="关闭上下文预览">×</button>
                  </div>
                  <p className="context-preview-note">预览 · 可能产生费用 · 只读</p>
                  <details className="context-preview-section">
                    <summary>Agent 角色规则</summary>
                    <pre>{contextPreview.system_prompt}</pre>
                  </details>
                  {contextPreview.segments.map((segment) => (
                    <details className="context-preview-section" key={`${segment.label}-${segment.chars}`}>
                      <summary>
                        <span>{segment.label}</span>
                        <small>{segment.chars.toLocaleString()} 字符{segment.truncated ? " · 已截断预览" : ""}</small>
                      </summary>
                      <pre>{segment.content}</pre>
                    </details>
                  ))}
                </section>
              )}
            </section>

            <AgentRunInspector
              run={lastAgentRun}
              proposals={actionProposals}
              busy={Boolean(busy)}
              onApplyProposal={(proposal) => void applyAgentProposal(proposal)}
              onRejectProposal={(proposal) => void rejectAgentProposal(proposal)}
            />

            <section className="panel next-action-panel confirm-panel">
              <div className="panel-title">
                <Check size={14} />
                确认与修订
              </div>
              {selectedArtifact?.stage === "review" && (
                <>
                  <textarea
                    rows={4}
                    value={revisionFeedback}
                    placeholder="修订要求"
                    onChange={(event) => setRevisionFeedback(event.target.value)}
                  />
                  <button
                    className="secondary-action"
                    onClick={requestRevision}
                    disabled={!canRequestRevision || Boolean(busy)}
                  >
                    <RefreshCcw size={14} /> 请求修订
                  </button>
                </>
              )}
              {canRunReview && selectedArtifact?.stage !== "review" && (
                <button
                  className="secondary-action"
                  onClick={() => runAgent("review")}
                  disabled={!detail || Boolean(busy)}
                >
                  <Play size={14} /> 提交试读
                </button>
              )}
              <div className="local-patch-tool">
                <div className="tool-subtitle">局部改写</div>
                <textarea
                  rows={4}
                  value={patchFindText}
                  placeholder="原文片段"
                  onChange={(event) => setPatchFindText(event.target.value)}
                />
                <textarea
                  rows={4}
                  value={aiPatchInstruction}
                  placeholder="局部改写要求"
                  onChange={(event) => setAiPatchInstruction(event.target.value)}
                />
                <button
                  onClick={reviseSelectedArtifactSpanWithAi}
                  disabled={
                    !selectedArtifactSupportsLocalPatch ||
                    !patchFindText.trim() ||
                    !aiPatchInstruction.trim() ||
                    Boolean(busy)
                  }
                >
                  <Sparkles size={14} /> AI 局部改写
                </button>
                <textarea
                  rows={4}
                  value={patchReplaceText}
                  placeholder="替换文本（留空删除）"
                  onChange={(event) => setPatchReplaceText(event.target.value)}
                />
                <button
                  onClick={replaceSelectedArtifactSpan}
                  disabled={
                    !selectedArtifactSupportsLocalPatch ||
                    !patchFindText.trim() ||
                    Boolean(busy)
                  }
                >
                  <Edit3 size={14} /> 局部替换
                </button>
              </div>
              {selectedArtifact && !selectedArtifactApproved && (
                <>
                  <textarea
                    rows={2}
                    value={approvalNote}
                    placeholder="确认备注，可为空"
                    onChange={(event) => setApprovalNote(event.target.value)}
                  />
                  <button
                    className="approve-action"
                    onClick={approveArtifact}
                    disabled={Boolean(busy)}
                  >
                    <Check size={14} /> 通过当前产物
                  </button>
                </>
              )}
            </section>

            <details className="tools-group" open>
              <summary>质量与连续性</summary>
              <div className="tools-group-content">

            <section className="panel">
              <div className="panel-title">
                <BarChart3 size={14} />
                质量检查
              </div>
              <button onClick={analyzeQuality} disabled={!selectedArtifact || Boolean(busy)}>
                <BarChart3 size={14} /> 检查当前产物
              </button>
              <button
                onClick={analyzeChapterGate}
                disabled={
                  !detail ||
                  !selectedChapter ||
                  !selectedArtifact ||
                  (selectedArtifact.stage !== "draft" && selectedArtifact.stage !== "revision") ||
                  Boolean(busy)
                }
              >
                <Check size={14} /> 通过前检查
              </button>
              {chapterGateReport?.recommended_action === "split" && (
                <button
                  onClick={generateSplitPlan}
                  disabled={!detail || !selectedChapter || !selectedArtifact || Boolean(busy)}
                >
                  <Rows3 size={14} /> 生成重规划方案
                </button>
              )}
              {chapterGateReport && (
                <div className="quality-report">
                  <p className="quality-subtle">
                    检查对象：
                    {gateArtifact
                      ? `${gateArtifact.stage === "revision" ? "修订稿" : "草稿"} v${gateArtifact.version} · artifact #${gateArtifact.id}`
                      : `artifact #${chapterGateReport.artifact_id}`}
                  </p>
                  <div className={`quality-score ${chapterGateReport.passed ? "strong" : "weak"}`}>
                    <strong>{chapterGateReport.blockers.length}</strong>
                    <span>{chapterGateReport.passed ? "通过" : "阻断"}</span>
                  </div>
                  <p className="quality-summary">{chapterGateReport.summary}</p>
                  <p className="quality-summary">
                    建议动作：{recommendationLabel(chapterGateReport.recommended_action)} · {chapterGateReport.action_reason}
                  </p>
                  <div className="quality-warnings">
                    {chapterGateReport.blockers.map((blocker, index) => (
                      <article className="quality-warning" key={`${blocker.kind}-${blocker.title}-${index}`}>
                        <strong>{blocker.title}</strong>
                        <p>{blocker.detail}</p>
                        <span>{blocker.kind} · {blocker.severity} · {blocker.suggestion}</span>
                      </article>
                    ))}
                    {chapterGateReport.blockers.length === 0 && (
                      <div className="empty-inline">无硬阻断</div>
                    )}
                  </div>
                </div>
              )}
              {chapterSplitPlan && (
                <div className="quality-report">
                  <div className="quality-score needs_revision">
                    <strong>重规划</strong>
                    <span>{chapterSplitPlan.suggested_current_title} {"->"} {chapterSplitPlan.suggested_next_title}</span>
                  </div>
                  <p className="quality-summary">{chapterSplitPlan.rationale}</p>
                  <div className="button-row split-plan-actions">
                    <button onClick={useSplitPlanForRevision} disabled={Boolean(busy)}>
                      <RefreshCcw size={14} /> 写入修订要求
                    </button>
                    <button onClick={createOrOpenNextChapterFromSplit} disabled={Boolean(busy)}>
                      <Plus size={14} /> 创建/打开下一章
                    </button>
                    <button onClick={applySplitCurrentTitle} disabled={!selectedChapter || Boolean(busy)}>
                      <Edit3 size={14} /> 应用当前章标题
                    </button>
                  </div>
                  <div className="quality-warnings">
                    <article className="quality-warning">
                      <strong>当前章任务</strong>
                      <p>{chapterSplitPlan.current_chapter_mission}</p>
                      <span>建议标题：{chapterSplitPlan.suggested_current_title}</span>
                    </article>
                    <article className="quality-warning">
                      <strong>下一章任务</strong>
                      <p>{chapterSplitPlan.next_chapter_mission}</p>
                      <span>建议标题：{chapterSplitPlan.suggested_next_title}</span>
                    </article>
                  </div>
                  <div className="split-plan-grid">
                    <article className="quality-warning">
                      <strong>当前章主保留</strong>
                      <ul className="split-plan-list">
                        {chapterSplitPlan.keep_in_current.map((item, index) => (
                          <li key={`keep-${index}`}>{item}</li>
                        ))}
                      </ul>
                    </article>
                    <article className="quality-warning">
                      <strong>后移到下一章</strong>
                      <ul className="split-plan-list">
                        {chapterSplitPlan.move_to_next.map((item, index) => (
                          <li key={`move-${index}`}>{item}</li>
                        ))}
                      </ul>
                    </article>
                    <article className="quality-warning">
                      <strong>当前章收尾节拍</strong>
                      <ul className="split-plan-list">
                        {chapterSplitPlan.carryover_closing_beats.map((item, index) => (
                          <li key={`close-${index}`}>{item}</li>
                        ))}
                      </ul>
                    </article>
                    <article className="quality-warning">
                      <strong>下一章开场节拍</strong>
                      <ul className="split-plan-list">
                        {chapterSplitPlan.next_chapter_opening_beats.map((item, index) => (
                          <li key={`open-${index}`}>{item}</li>
                        ))}
                      </ul>
                    </article>
                  </div>
                </div>
              )}
              {qualityReport && (
                <div className="quality-report">
                  <p className="quality-subtle">
                    检查对象：
                    {qualityArtifact
                      ? `${qualityArtifact.stage === "revision" ? "修订稿" : qualityArtifact.stage === "draft" ? "草稿" : qualityArtifact.stage} v${qualityArtifact.version} · artifact #${qualityArtifact.id}`
                      : `artifact #${qualityReport.artifact_id}`}
                  </p>
                  <div className={`quality-score ${qualityReport.verdict}`}>
                    <strong>{qualityReport.score}</strong>
                    <span>{qualityVerdictLabel(qualityReport.verdict)}</span>
                  </div>
                  <p className="quality-summary">{qualityReport.summary}</p>
                  <div className="quality-metrics">
                    {qualityReport.metrics.slice(0, 8).map((metric) => (
                      <div className="quality-metric" key={metric.label}>
                        <span>{metric.label}</span>
                        <strong>{formatMetricValue(metric.value, metric.unit)}</strong>
                      </div>
                    ))}
                  </div>
                  <div className="quality-warnings">
                    {qualityReport.warnings.slice(0, 4).map((warning) => (
                      <article className="quality-warning" key={warning.title}>
                        <strong>{warning.title}</strong>
                        <p>{warning.detail}</p>
                        <span>{warning.suggestion}</span>
                      </article>
                    ))}
                  </div>
                </div>
              )}
            </section>

            <section className="panel">
              <div className="panel-title">
                <Rows3 size={14} />
                连续性审校
              </div>
              <button
                onClick={checkLedgerContinuity}
                disabled={!selectedArtifact || (selectedArtifact.stage !== "draft" && selectedArtifact.stage !== "revision") || Boolean(busy)}
              >
                <Rows3 size={14} /> 状态账本核对
              </button>
              <button
                onClick={reviewContinuity}
                disabled={!detail || detail.chapters.length < 2 || Boolean(busy)}
              >
                <Rows3 size={14} /> 审校多章衔接
              </button>
              {continuityReport && (
                <div className="quality-report">
                  <div className={`quality-score ${continuityReport.verdict}`}>
                    <strong>{continuityReport.chapter_titles.length}</strong>
                    <span>{qualityVerdictLabel(continuityReport.verdict)}</span>
                  </div>
                  <p className="quality-summary">{continuityReport.summary}</p>
                  <div className="quality-warnings">
                    {continuityReport.issues.map((issue, index) => (
                      <article className="quality-warning" key={`${issue.issue_type}-${index}`}>
                        <strong>{issue.issue_type}</strong>
                        <p>{issue.reason}</p>
                        <span>{issue.chapters.join(" / ")} · {issue.suggestion}</span>
                      </article>
                    ))}
                  </div>
                </div>
              )}
            </section>

            </div>
            </details>

            <details className="tools-group">
              <summary>资料检索</summary>
              <div className="tools-group-content">
            <section className="panel">
              <div className="panel-title">
                <Search size={14} />
                历史检索
              </div>
              <textarea
                rows={3}
                value={contextQuery}
                placeholder="搜索旧人物、物件、线索或事件"
                onChange={(event) => {
                  setContextQuery(event.target.value);
                  setContextRerank(null);
                }}
              />
              <div className="context-search-actions">
                <button onClick={searchContext} disabled={!detail || !contextQuery.trim() || Boolean(busy)}>
                  <Search size={14} /> 检索全书资料
                </button>
                <button
                  className="secondary-action"
                  onClick={rerankContext}
                  disabled={!detail || contextSnippets.length === 0 || Boolean(busy)}
                >
                  <Sparkles size={14} /> AI 筛选
                </button>
              </div>
              {contextSnippets.length > 0 && (
                <div className="context-result-group">
                  <div className="context-result-group-head">
                    <strong>原始召回结果</strong>
                    <span>{contextSnippets.length} 条</span>
                  </div>
                  <div className="context-results">
                  {contextSnippets.map((snippet, index) => (
                    <article className="context-snippet" key={`${snippet.source_label}-${snippet.matched_term}-${index}`}>
                      <div className="context-snippet-head">
                        <strong>{snippet.source_label}</strong>
                        <span>{snippet.matched_term}</span>
                      </div>
                      <p>{snippet.content}</p>
                    </article>
                  ))}
                  </div>
                </div>
              )}
              {contextRerank && (
                <div className="context-result-group context-rerank-group">
                  <div className="context-result-group-head">
                    <strong>AI 筛选结果</strong>
                    <span>{contextRerank.status === "fallback" ? "原始候选回退" : `${contextRerank.selected.length} 条`}</span>
                  </div>
                  {contextRerank.error && <p className="context-rerank-error">{contextRerank.error}</p>}
                  {contextRerank.selected.length > 0 ? (
                    <div className="context-results">
                      {contextRerank.selected.map((snippet) => (
                        <article className="context-snippet" key={`reranked-${snippet.candidate_id}`}>
                          <div className="context-snippet-head">
                            <strong>{snippet.source_label}</strong>
                            <span>{snippet.category} · {snippet.matched_term}</span>
                          </div>
                          <p>{snippet.content}</p>
                          <small>{snippet.reason}</small>
                        </article>
                      ))}
                    </div>
                  ) : <p className="context-rerank-empty">暂无相关候选</p>}
                </div>
              )}
            </section>


            </div>
            </details>

            <details className="tools-group">
              <summary>协作记录</summary>
              <div className="tools-group-content">
            <section className="panel">
              <div className="panel-title">
                <MessageSquare size={14} />
                最近协作
              </div>
              <div className="activity-list">
                {visibleMessages.map((message) => (
                  <article className="activity-item" key={message.id}>
                    <div className="activity-item-head">
                      <strong>{roleLabel(message.role)}</strong>
                      <span>{new Date(message.created_at).toLocaleString()}</span>
                    </div>
                    <p>{message.content}</p>
                  </article>
                ))}
                {visibleMessages.length === 0 && <div className="empty-inline">还没有协作记录</div>}
              </div>
            </section>

            <section className="panel">
              <div className="panel-title">
                <Sparkles size={14} />
                最近运行
              </div>
              <div className="activity-list">
                {visibleRuns.map((run) => (
                  <article className="activity-item" key={run.id}>
                    <div className="activity-item-head">
                      <strong>{stageLabel(run.stage)}</strong>
                      <span>{run.elapsed_ms} ms</span>
                    </div>
                    <p>
                      {run.status === "success"
                        ? "执行成功"
                        : run.status === "streaming"
                          ? `正在接收输出${run.output_chars ? ` · ${run.output_chars} 字符` : ""}`
                          : run.error ?? "执行失败"}
                    </p>
                  </article>
                ))}
                {visibleRuns.length === 0 && <div className="empty-inline">还没有运行记录</div>}
              </div>
            </section>

            {exportText && (
              <section className="panel">
                <div className="panel-title">
                  <Download size={14} />
                  导出
                </div>
                <textarea readOnly rows={6} value={exportText} />
                <button className="secondary-action" onClick={downloadExportedMarkdown}>
                  <Download size={14} /> 下载 Markdown 文件
                </button>
              </section>
            )}
              </div>
            </details>
              </div>
            </details>
          </aside>
          )}
          </>
        </div>
      </section>

      <NewProjectModal
        isOpen={showNewProjectModal}
        onClose={() => {
          setShowNewProjectModal(false);
          setNewProject(defaultProject);
        }}
        onSubmit={createProject}
        formData={newProject}
        onFormChange={setNewProject}
        busy={Boolean(busy)}
      />

      {projectDraft && (
        <ProjectEditorModal
          isOpen={showProjectEditor}
          onClose={() => setShowProjectEditor(false)}
          onSubmit={updateProject}
          formData={projectDraft}
          onFormChange={setProjectDraft}
          busy={Boolean(busy)}
        />
      )}

      {projectPendingDeletion && (
        <div
          className="modal-overlay"
          role="presentation"
          onClick={() => {
            if (!busy) setProjectPendingDeletion(null);
          }}
        >
          <section
            className="modal project-delete-confirmation"
            role="dialog"
            aria-modal="true"
            aria-labelledby="project-delete-title"
            onClick={(event) => event.stopPropagation()}
          >
            <header className="modal-header">
              <h2 id="project-delete-title">删除书籍</h2>
            </header>
            <div className="modal-body">
              <p>确定删除《{projectPendingDeletion.title}》吗？</p>
              <p className="project-delete-warning">该书籍的章节、产物和记录都会一并删除，且无法恢复。</p>
            </div>
            <footer className="modal-footer">
              <button onClick={() => setProjectPendingDeletion(null)} disabled={Boolean(busy)}>取消</button>
              <button className="btn-danger" onClick={() => void deleteProject(projectPendingDeletion)} disabled={Boolean(busy)}>
                {busy === "删除书籍" ? "正在删除..." : "删除书籍"}
              </button>
            </footer>
          </section>
        </div>
      )}

      <AdoptionDrawer
        open={showAdoptionDrawer}
        proposals={selectedArtifactProposals as AdoptionProposal[]}
        chapters={detail?.chapters ?? []}
        knowledgeCards={detail?.knowledge_cards ?? []}
        foreshadowings={detail?.foreshadowings ?? []}
        busy={Boolean(busy)}
        onClose={() => setShowAdoptionDrawer(false)}
        onExtract={prepareArtifactAdoptions}
        onSave={saveAdoptionProposal}
        onApply={applyAdoptionProposals}
        onReject={rejectAdoptionProposals}
      />
    </main>
  );
}
