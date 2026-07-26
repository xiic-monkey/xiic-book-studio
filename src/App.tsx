import {
  AlertCircle,
  BarChart3,
  Box,
  BookOpen,
  CalendarDays,
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
  Play,
  Plus,
  RefreshCcw,
  Save,
  Search,
  Settings,
  Sparkles,
  Trash2,
  Users,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import { createPortal } from "react-dom";
import { api } from "./api";
import { ArtifactDiffPanel } from "./components/ArtifactDiffPanel";
import { ContinuityLibraryPanel } from "./components/ContinuityLibraryPanel";
import type { EntityTimelineEntry, StoryIndexStatus } from "./components/ContinuityLibraryPanel";
import { KnowledgeSectionCard, parseKnowledgeSections } from "./components/KnowledgeSectionCard";
import type {
  AdoptionProposal,
  AiSettings,
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
  ProjectDetail,
  QualityReport,
  ReviewIssue,
  SaveWritingSkill,
  SaveForeshadowingInput,
  SaveKnowledgeCardInput,
  Stage,
  StoryContextSnippet,
  StoryArchitectMode,
  StoryEntity,
  StoryEvent,
  StoryEventParticipant,
  StoryFact,
  StoryIndexSource,
  StorySearchStatus,
  WorkflowRun,
  WritingSkill,
  ContextPreview,
} from "./types";
import { NewProjectModal } from "./components/NewProjectModal";
import { ProjectEditorModal } from "./components/ProjectEditorModal";
import { SettingsView } from "./components/SettingsView";
import { AdoptionDrawer } from "./components/AdoptionDrawer";

const foundationStages: Array<{ id: Stage; label: string; scope: "book" }> = [
  { id: "setting", label: "世界与规则", scope: "book" },
  { id: "outline", label: "阶段大纲", scope: "book" },
  { id: "characters", label: "角色卡", scope: "book" },
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
  has_api_key: false,
};

type ViewMode = "main" | "settings";
type MainSurface = "official" | "workbench" | "library";
type ContentSurface = "official" | "workbench";
type LibrarySection = "setting" | "outline" | "characters";
type LibraryFocus = LibrarySection | "items" | "events" | "foreshadowing";
type SettingsCategory = "ai" | "agents" | "skills" | "editor" | "data" | "appearance";
type AgentRunMode = "smart" | "fresh";
const SIDEBAR_WIDTH_STORAGE_KEY = "book-studio.sidebar-width";
const SIDEBAR_COLLAPSED_STORAGE_KEY = "book-studio.sidebar-collapsed";
const SIDEBAR_DEFAULT_WIDTH = 280;
const SIDEBAR_COLLAPSED_WIDTH = 52;
const SIDEBAR_MIN_WIDTH = 220;
const SIDEBAR_MAX_WIDTH = 420;

function clampSidebarWidth(width: number) {
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, width));
}

function resolveChapterBody(detail: ProjectDetail | null, chapter: Chapter | null) {
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

export function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [selectedProjectId, setSelectedProjectId] = useState<number | null>(null);
  const [selectedChapterId, setSelectedChapterId] = useState<number | null>(null);
  const [selectedStage, setSelectedStage] = useState<Stage>("setting");
  const [selectedArtifactId, setSelectedArtifactId] = useState<number | null>(null);
  const [explicitArchitectSourceId, setExplicitArchitectSourceId] = useState<number | null>(null);
  const [newProject, setNewProject] = useState<NewProject>(defaultProject);
  const [projectDraft, setProjectDraft] = useState<ProjectUpdate | null>(null);
  const [settings, setSettings] = useState<AiSettings>(defaultSettings);
  const [storySearchStatus, setStorySearchStatus] = useState<StorySearchStatus | null>(null);
  const [writingSkills, setWritingSkills] = useState<WritingSkill[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [instruction, setInstruction] = useState("");
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
  const [streamingRun, setStreamingRun] = useState<WorkflowRun | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exportText, setExportText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [contextQuery, setContextQuery] = useState("");
  const [contextSnippets, setContextSnippets] = useState<StoryContextSnippet[]>([]);
  const [contextPreview, setContextPreview] = useState<ContextPreview | null>(null);
  const [storyBibleNote, setStoryBibleNote] = useState("");
  const [showAdoptionDrawer, setShowAdoptionDrawer] = useState(false);

  const [viewMode, setViewMode] = useState<ViewMode>("main");
  const [mainSurface, setMainSurface] = useState<MainSurface>("official");
  const [libraryOriginSurface, setLibraryOriginSurface] = useState<ContentSurface>("official");
  const [librarySection, setLibrarySection] = useState<LibrarySection>("setting");
  const [libraryFocus, setLibraryFocus] = useState<LibraryFocus>("setting");
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
  const chapterToolsRef = useRef<HTMLDetailsElement | null>(null);
  const [chapterToolsOpen, setChapterToolsOpen] = useState(false);
  const [chapterToolsPosition, setChapterToolsPosition] = useState({ top: 0, left: 0 });

  const visibleContentSurface: ContentSurface = mainSurface === "library" ? libraryOriginSurface : mainSurface;

  function updateChapterToolsPosition() {
    const summary = chapterToolsRef.current?.querySelector("summary");
    if (!summary) return;
    const rect = summary.getBoundingClientRect();
    const menuWidth = 168;
    const viewportPadding = 8;
    const left = Math.max(
      viewportPadding,
      Math.min(rect.left, window.innerWidth - menuWidth - viewportPadding)
    );
    setChapterToolsPosition({ top: rect.bottom + 6, left });
  }

  useEffect(() => {
    if (!chapterToolsOpen) return;
    const update = () => updateChapterToolsPosition();
    update();
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [chapterToolsOpen]);

  function openLibrary(focus?: LibraryFocus) {
    if (mainSurface !== "library") setLibraryOriginSurface(mainSurface);
    const section = focus && foundationStages.some((stage) => stage.id === focus)
      ? focus as LibrarySection
      : null;
    if (section) {
      setLibraryFocus(section);
      setLibrarySection(section);
      setSelectedStage(section);
      setSelectedArtifactId(null);
    } else {
      setLibraryFocus(focus ?? "foreshadowing");
    }
    setSelectedLibraryEntityId(null);
    resetKnowledgeComposer();
    setMainSurface("library");
  }

  useEffect(() => {
    void refreshProjects();
    void refreshWritingSkills();
  }, []);

  useEffect(() => {
    if (!selectedProjectId) return;
    activeProjectRequestRef.current = selectedProjectId;
    void refreshDetail(selectedProjectId);
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
  }, [detail, selectedChapterId]);

  const selectedChapter = useMemo(
    () => detail?.chapters.find((chapter) => chapter.id === selectedChapterId) ?? null,
    [detail, selectedChapterId]
  );

  const visibleArtifacts = useMemo(() => {
    if (!detail) return [];
    const stageMeta = stages.find((stage) => stage.id === selectedStage);
    return detail.artifacts
      .filter((artifact) => artifact.stage === selectedStage)
      .filter((artifact) =>
        stageMeta?.scope === "chapter" ? artifact.chapter_id === selectedChapterId : artifact.chapter_id == null
      )
      .sort((a, b) => b.version - a.version);
  }, [detail, selectedStage, selectedChapterId]);

  const selectedArtifact = useMemo(() => {
    return visibleArtifacts.find((artifact) => artifact.id === selectedArtifactId) ?? visibleArtifacts[0] ?? null;
  }, [visibleArtifacts, selectedArtifactId]);

  const libraryArtifact = useMemo(() => {
    if (!detail) return null;
    return detail.artifacts
      .filter((artifact) => artifact.stage === librarySection && artifact.chapter_id == null)
      .sort((a, b) => {
        const approvalDelta = Number(b.status === "approved") - Number(a.status === "approved");
        if (approvalDelta !== 0) return approvalDelta;
        return b.version - a.version;
      })[0] ?? null;
  }, [detail, librarySection]);

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
    return (detail.knowledge_cards ?? []).filter(categoryMatch);
  }, [detail, librarySection]);

  const visibleForeshadowings = useMemo(
    () => detail?.foreshadowings?.filter((item) => item.status !== "archived") ?? [],
    [detail]
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
      void loadProjectDetail(selectedProjectId).catch(() => {
        // The next poll can recover from a transient detail request failure.
      });
    }, 2000);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [hasActiveIndexJobs, selectedProjectId]);

  const activeStoryArc = useMemo(
    () => detail?.story_arcs?.find((arc) => arc.status === "active") ?? null,
    [detail]
  );

  useEffect(() => {
    if (!detail || !selectedChapter) return;

    const selectedStillExists =
      selectedArtifactId == null || detail.artifacts.some((artifact) => artifact.id === selectedArtifactId);
    if (selectedArtifact && selectedStillExists) return;
    if (!bodyStages.includes(selectedStage) && selectedArtifactId == null) return;

    const body = resolveChapterBody(detail, selectedChapter);
    if (!body) return;
    if (body.stage !== selectedStage) setSelectedStage(body.stage);
    if (body.id !== selectedArtifactId) setSelectedArtifactId(body.id);
  }, [detail, selectedChapter, selectedArtifact, selectedArtifactId, selectedStage]);

  const compareArtifact = useMemo(() => {
    if (!compareArtifactId) return null;
    return visibleArtifacts.find((artifact) => artifact.id === compareArtifactId) ?? null;
  }, [visibleArtifacts, compareArtifactId]);

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
    return selectedArtifactApproved ? "当前已批准资料不能删除" : null;
  }, [selectedArtifact, selectedArtifactApproved, selectedArtifactIsCurrentBody]);

  const gateArtifact = useMemo(() => {
    if (!detail || !chapterGateReport) return null;
    return detail.artifacts.find((artifact) => artifact.id === chapterGateReport.artifact_id) ?? null;
  }, [detail, chapterGateReport]);

  const qualityArtifact = useMemo(() => {
    if (!detail || !qualityReport) return null;
    return detail.artifacts.find((artifact) => artifact.id === qualityReport.artifact_id) ?? null;
  }, [detail, qualityReport]);

  const currentChapterBody = useMemo(() => {
    if (!detail || !selectedChapter?.current_artifact_id) return null;
    return detail.artifacts.find((artifact) => artifact.id === selectedChapter.current_artifact_id) ?? null;
  }, [detail, selectedChapter]);

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
      if (!activeProjectRequestRef.current && list[0]) openProject(list[0].id);
    });
  }

  function openProject(projectId: number | null) {
    activeProjectRequestRef.current = projectId;
    setSelectedProjectId(projectId);
    setSelectedChapterId(null);
    setSelectedStage("setting");
    setSelectedArtifactId(null);
    setCompareArtifactId(null);
    setStreamingRun(null);
    setQualityReport(null);
    setContinuityReport(null);
    setChapterGateReport(null);
    setChapterSplitPlan(null);
    setDetail((current) => (current?.project.id === projectId ? current : null));
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
    await runTask("刷新项目", async () => {
      await loadProjectDetail(projectId);
    });
  }

  async function loadProjectDetail(projectId: number) {
    const next = await api.getProject(projectId);
    if (activeProjectRequestRef.current !== projectId) return false;
    applyProjectDetail(next);
    return true;
  }

  function applyProjectDetail(next: ProjectDetail) {
    setDetail(next);
    setSettings(next.settings);
    setStreamingRun(next.workflow_runs.find((run) => run.status === "streaming") ?? null);
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

  function beginStreamingPoll(projectId: number) {
    let active = true;
    let inFlight: Promise<void> | null = null;
    const poll = (finalRefresh = false) => {
      if (!active && !finalRefresh) return Promise.resolve();
      if (inFlight) return inFlight;
      inFlight = (async () => {
        try {
          await loadProjectDetail(projectId);
        } catch {
          // The command result still reports the actionable error; polling should stay quiet.
        } finally {
          inFlight = null;
        }
      })();
      return inFlight;
    };
    void poll();
    const interval = window.setInterval(() => void poll(), 600);
    return async () => {
      active = false;
      window.clearInterval(interval);
      // The HTTP command can complete just after the last interval tick. Always
      // await one final detail read before leaving the streaming state behind.
      await poll(true);
    };
  }

  async function refreshWritingSkills() {
    try {
      const list = await api.listWritingSkills();
      setWritingSkills(list);
    } catch (err) {
      setError(String(err));
    }
  }

  async function runTask(label: string, task: () => Promise<void>) {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      await task();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function createProject() {
    await runTask("新建项目", async () => {
      const project = await api.createProject(newProject);
      setProjects([project, ...projects]);
      openProject(project.id);
      setNotice("项目已创建");
      setShowNewProjectModal(false);
      setNewProject(defaultProject);
    });
  }

  async function updateProject() {
    if (!projectDraft || !detail) return;
    await runTask("保存项目", async () => {
      const updated = await api.updateProject(projectDraft);
      setProjects((current) =>
        current.map((project) => (project.id === updated.id ? updated : project))
      );
      setProjectDraft({
        id: updated.id,
        title: updated.title,
        genre: updated.genre,
        target_words: updated.target_words,
        premise: updated.premise,
        status: updated.status,
      });
      setShowProjectEditor(false);
      await refreshDetail(updated.id);
      setNotice("项目信息已更新");
    });
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

  async function handleSaveSettings(savedSettings: AiSettings, key: string) {
    await runTask("保存设置", async () => {
      const saved = await api.saveAiSettings({
        ...savedSettings,
        api_key: key.trim() || null,
      });
      setSettings(saved);
      setApiKey("");
      setNotice("AI 设置已保存");
    });
  }

  async function handleTestConnection(_currentSettings: AiSettings, _key: string) {
    await runTask("测试连接", async () => {
      const result = await api.testAiConnection({
        base_url: _currentSettings.base_url,
        model: _currentSettings.model,
        temperature: _currentSettings.temperature,
        thinking_enabled: _currentSettings.thinking_enabled,
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
      setMainSurface(libraryOriginSurface);
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
    setMainSurface("official");
  }

  async function rebuildLibraryIndex() {
    if (!detail) return;
    await runTask("更新资料索引", async () => {
      const jobs = await api.retryIndexJobs({ project_id: detail.project.id });
      await loadProjectDetail(detail.project.id);
      const queued = jobs.filter((job) => job.status === "pending").length;
      setNotice(queued > 0 ? `资料索引已加入后台队列：${queued} 个任务` : "没有需要更新的索引任务");
    });
  }

  async function rebuildStorySearchIndex() {
    if (!detail) return;
    await runTask("重建本地检索", async () => {
      const status = await api.rebuildStorySearchIndex({ project_id: detail.project.id });
      setStorySearchStatus(status);
      await refreshDetail(detail.project.id);
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
      await refreshDetail(detail.project.id);
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
      await refreshDetail(detail.project.id);
      setNotice(status === "active" ? "伏笔已确认并加入追踪" : "伏笔已保存，等待人工确认");
    });
  }

  async function updateKnowledgeCardStatus(card: KnowledgeCard, status: "approved" | "archived") {
    if (!detail) return;
    await runTask(status === "approved" ? "确认资料卡" : "归档资料卡", async () => {
      await api.saveKnowledgeCard({ ...card, status });
      await refreshDetail(detail.project.id);
      setNotice(status === "approved" ? "资料卡已确认并加入写作依据" : "资料卡已归档，不再作为写作依据");
    });
  }

  async function updateForeshadowingStatus(
    item: Foreshadowing,
    status: "active" | "ready_for_payoff" | "resolved" | "archived"
  ) {
    if (!detail) return;
    await runTask("更新伏笔", async () => {
      await api.saveForeshadowing({ ...item, status });
      await refreshDetail(detail.project.id);
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
      selectChapter(chapter);
      setChapterDraft("");
      await refreshDetail(detail.project.id);
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
      if (chapterToolsRef.current?.open) chapterToolsRef.current.open = false;
      await refreshDetail(detail.project.id);
      setNotice(`已删除 ${chapterTitle}`);
    });
  }

  async function renameCurrentChapter() {
    if (!detail || !selectedChapter) return;
    const title = chapterDraft.trim();
    if (!title) return;
    await runTask("重命名章节", async () => {
      const updated = await api.updateChapter({
        id: selectedChapter.id,
        title,
        status: selectedChapter.status,
      });
      setSelectedChapterId(updated.id);
      setSelectedArtifactId(null);
      setChapterDraft("");
      await refreshDetail(detail.project.id);
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
      selectChapter(chapter);
      await refreshDetail(detail.project.id);
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
      message = "写作前请先在这里确认创作基准。";
    } else if (!activeStoryArc) {
      message = "写作前请先确认当前故事阶段。";
    } else if (!detail.story_bible_review) {
      message = "创作基准已确认，请先运行一致性审校。";
    } else if (detail.story_bible_review.status !== "confirmed") {
      message = "请先人工确认最新的一致性审校结论。";
    }

    if (!message) return false;
    setLibraryOriginSurface("workbench");
    setLibrarySection("setting");
    setLibraryFocus("setting");
    setMainSurface("library");
    setNotice(null);
    setError(message);
    return true;
  }

  async function runAgent(
    stage: Stage = selectedStage,
    mode: AgentRunMode = "smart",
  ) {
    if (!detail) return;
    if (stage === "setting" || stage === "outline" || stage === "characters") {
      return runStoryArchitect(architectModeByStage[stage], mode);
    }
    if (detail.project.id !== selectedProjectId) {
      setError("书籍切换尚未完成，请等待当前书籍加载后再运行 Agent。");
      return;
    }
    if (redirectToStoryBibleIfDraftBlocked(stage)) return;
    setMainSurface("workbench");
    const meta = stages.find((item) => item.id === stage);
    await runTask("运行 Agent", async () => {
      const sourceArtifactId =
        stage === "review"
          ? selectedArtifact &&
            selectedArtifact.chapter_id === selectedChapterId &&
            (selectedArtifact.stage === "draft" || selectedArtifact.stage === "revision")
            ? selectedArtifact.id
            : null
          : null;
      const stopPolling = beginStreamingPoll(detail.project.id);
      let result;
      try {
        result = await api.runAgentStep({
          project_id: detail.project.id,
          stage,
          chapter_id: meta?.scope === "chapter" ? selectedChapterId : null,
          user_instruction: instruction.trim() || null,
          source_artifact_id: sourceArtifactId,
        });
      } finally {
        await stopPolling();
      }
      setSelectedStage(stage);
      setSelectedArtifactId(result.artifact.id);
      setInstruction("");
      await loadProjectDetail(detail.project.id);
      if (stage === "review" && sourceArtifactId) {
        try {
          setLedgerContinuityReport(
            await api.checkArtifactLedgerContinuity({
              project_id: detail.project.id,
              artifact_id: sourceArtifactId,
            })
          );
        } catch {
          // Trial reading already completed. The ledger is an additional, non-blocking check.
          setLedgerContinuityReport(null);
        }
      }
      setNotice(
        mode === "smart" && sourceArtifactId
          ? `${meta?.label ?? "Agent"}已基于当前版本生成 v${result.artifact.version}`
          : `${meta?.label ?? "Agent"}已生成 v${result.artifact.version}`
      );
    });
  }

  async function runStoryArchitect(
    architectMode: StoryArchitectMode,
    runMode: AgentRunMode = "smart",
  ) {
    if (!detail) return;
    const stage = artifactStageForArchitectMode(architectMode);
    setMainSurface("library");
    await runTask("运行故事架构 Agent", async () => {
      const explicitSource = detail.artifacts.find((artifact) => artifact.id === explicitArchitectSourceId);
      const sourceArtifactId = runMode === "smart"
        && explicitSource?.stage === stage
        && explicitSource.chapter_id == null
        ? explicitSource.id
        : null;
      const stopPolling = beginStreamingPoll(detail.project.id);
      let result;
      try {
        result = await api.runStoryArchitect({
          project_id: detail.project.id,
          mode: architectMode,
          arc_id: activeStoryArc?.id ?? null,
          user_instruction: instruction.trim() || null,
          source_artifact_id: sourceArtifactId,
        });
      } finally {
        await stopPolling();
      }
      setSelectedStage(stage);
      setSelectedArtifactId(result.artifact.id);
      setExplicitArchitectSourceId(null);
      setInstruction("");
      await loadProjectDetail(detail.project.id);
      setNotice(`${architectModeLabel[architectMode]}已生成 v${result.artifact.version}`);
    });
  }

  async function createTargetedRework(issue: CanonIssue) {
    if (!detail) return;
    const architectMode = resolveArchitectMode(issue.owner_mode);
    const stage = artifactStageForArchitectMode(architectMode);
    setMainSurface("library");
    await runTask("定向返工故事资料", async () => {
      const stopPolling = beginStreamingPoll(detail.project.id);
      let result;
      try {
        result = await api.createTargetedRework({
          project_id: detail.project.id,
          mode: architectMode,
          arc_id: activeStoryArc?.id ?? null,
          user_instruction: issue.rework_instruction,
          source_artifact_id: null,
        });
      } finally {
        await stopPolling();
      }
      setSelectedStage(stage);
      setSelectedArtifactId(result.artifact.id);
      await loadProjectDetail(detail.project.id);
      setNotice(`已生成针对“${issue.title}”的候选资料版本`);
    });
  }

  async function confirmStoryBible() {
    if (!detail) return;
    await runTask("确认创作基准", async () => {
      await api.confirmStoryBible({ project_id: detail.project.id, note: storyBibleNote });
      setStoryBibleNote("");
      await loadProjectDetail(detail.project.id);
      setNotice("创作基准与当前故事阶段已确认");
    });
  }

  async function reviewStoryBible() {
    if (!detail) return;
    await runTask("审校创作基准", async () => {
      await api.reviewStoryBible({ project_id: detail.project.id });
      await loadProjectDetail(detail.project.id);
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
        note: storyBibleNote,
      });
      setStoryBibleNote("");
      await loadProjectDetail(detail.project.id);
      setNotice("一致性审校已人工确认，可以继续正文创作");
    });
  }

  function agentSourceArtifactId(stage: Stage) {
    const meta = stages.find((item) => item.id === stage);
    if (stage === "review") {
      return selectedArtifact &&
        selectedArtifact.chapter_id === selectedChapterId &&
        (selectedArtifact.stage === "draft" || selectedArtifact.stage === "revision")
        ? selectedArtifact.id
        : null;
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
      const preview = await api.previewAgentContext({
        project_id: detail.project.id,
        stage: selectedStage,
        chapter_id: meta?.scope === "chapter" ? selectedChapterId : null,
        user_instruction: instruction.trim() || null,
        source_artifact_id: agentSourceArtifactId(selectedStage),
      });
      setContextPreview(preview);
    });
  }

  async function approveArtifact() {
    if (!detail || !selectedArtifact) return;
    if (selectedArtifactApproved) {
      setNotice("当前产物已经通过");
      return;
    }
    await runTask("人工通过", async () => {
      const approvedArtifact = selectedArtifact;
      await api.approveStage(detail.project.id, approvedArtifact.stage, approvedArtifact.id, approvalNote);
      setApprovalNote("");
      await refreshDetail(detail.project.id);
      await refreshStorySearchStatus(detail.project.id);
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
      await refreshDetail(detail.project.id);
      await refreshStorySearchStatus(detail.project.id);
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
      await loadProjectDetail(detail.project.id);
      setNotice(proposals.length > 0 ? `已整理出 ${proposals.length} 条待确认资料` : "未发现可靠的资料变更");
    });
  }

  async function saveAdoptionProposal(proposalId: number, data: Record<string, unknown>) {
    if (!detail) return;
    await runTask("保存资料候选", async () => {
      await api.updateAdoptionProposal({ proposal_id: proposalId, data });
      await loadProjectDetail(detail.project.id);
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
      await loadProjectDetail(detail.project.id);
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
      await loadProjectDetail(detail.project.id);
      setNotice(`已拒绝 ${proposalIds.length} 条资料变更`);
    });
  }

  async function requestRevision() {
    if (!detail || !selectedArtifact) return;
    await runTask("请求修订", async () => {
      const stopPolling = beginStreamingPoll(detail.project.id);
      let result;
      try {
        result = await api.requestRevision({
          project_id: detail.project.id,
          artifact_id: selectedArtifact.id,
          feedback: revisionFeedback,
        });
      } finally {
        await stopPolling();
      }
      setSelectedStage("revision");
      setSelectedArtifactId(result.artifact.id);
      setRevisionFeedback("");
      setReviewIssues([]);
      await loadProjectDetail(detail.project.id);
      setNotice("修订稿已生成");
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
      setSelectedStage(result.artifact.stage as Stage);
      setSelectedArtifactId(result.artifact.id);
      setPatchFindText("");
      setPatchReplaceText("");
      await refreshDetail(detail.project.id);
      setNotice(`局部替换已生成 ${result.artifact.title} v${result.artifact.version}`);
    });
  }

  async function reviseSelectedArtifactSpanWithAi() {
    if (!detail || !selectedArtifact) return;
    if (!selectedArtifactSupportsLocalPatch) {
      setNotice("当前产物暂不支持 AI 局部修订");
      return;
    }
    await runTask("AI 局部修订", async () => {
      const result = await api.reviseArtifactSpanWithAi({
        project_id: detail.project.id,
        artifact_id: selectedArtifact.id,
        find_text: patchFindText,
        instruction: aiPatchInstruction,
      });
      setSelectedStage(result.artifact.stage as Stage);
      setSelectedArtifactId(result.artifact.id);
      setPatchFindText("");
      setPatchReplaceText("");
      setAiPatchInstruction("");
      await refreshDetail(detail.project.id);
      setNotice(`AI 局部修订已生成 ${result.artifact.title} v${result.artifact.version}`);
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
      await refreshDetail(detail.project.id);
      setNotice(`已删除版本 v${deletedVersion}`);
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
      await refreshDetail(detail.project.id);
      setNotice(`已清理 ${result.deleted_artifact_ids.length} 个历史版本`);
    });
  }

  async function analyzeQuality() {
    if (!selectedArtifact) return;
    await runTask("质量检查", async () => {
      const report = await api.analyzeArtifactQuality(selectedArtifact.id);
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
    setMainSurface("workbench");
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
        id: selectedChapter.id,
        title,
        status: selectedChapter.status,
      });
      await refreshDetail(detail.project.id);
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
      await refreshDetail(detail.project.id);
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
      setNotice("请选择一份章节草稿或修订稿进行状态账本核对");
      return;
    }
    await runTask("状态账本核对", async () => {
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
      setNotice(snippets.length > 0 ? `找到 ${snippets.length} 条历史上下文` : "没有找到相关历史上下文");
    });
  }

  async function exportMarkdown() {
    if (!detail) return;
    await runTask("导出", async () => {
      const markdown = await api.exportProject(detail.project.id);
      setExportText(markdown);
      await navigator.clipboard?.writeText(markdown).catch(() => undefined);
      setNotice("Markdown 已生成，并尝试复制到剪贴板");
    });
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
        projectId={detail?.project.id ?? null}
        storySearchStatus={storySearchStatus}
        apiKey={apiKey}
        settingsCategory={settingsCategory}
        onSettingsCategoryChange={setSettingsCategory}
        onBack={() => setViewMode("main")}
        onSaveSettings={handleSaveSettings}
        onTestConnection={handleTestConnection}
        onRefreshModels={handleRefreshModels}
        onRefreshStorySearchStatus={refreshStorySearchStatus}
        onRebuildStorySearch={rebuildStorySearchIndex}
        agents={detail?.agents ?? []}
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
          ) : false ? (
            <section className="library-workspace">
              <header className="library-header">
                <div>
                  <h2>书籍资料</h2>
                  <p>这里是会随创作逐步扩展的设定、大纲、角色与伏笔账本。</p>
                </div>
                <div className="button-row">
                  <button onClick={() => setShowKnowledgeComposer((current) => !current)} disabled={!detail || Boolean(busy)}>
                    <Plus size={14} /> 补充资料
                  </button>
                </div>
              </header>

              {(libraryFocus === "characters" || libraryFocus === "items" || libraryFocus === "events") && (
                <section className="continuity-library">
                  <header className="continuity-library-head">
                    <div>
                      <div className="panel-title">
                        {libraryFocus === "characters" ? <Users size={15} /> : libraryFocus === "items" ? <Box size={15} /> : <CalendarDays size={15} />}
                        连续性资料
                      </div>
                      <h3>{libraryFocus === "characters" ? "角色时间线" : libraryFocus === "items" ? "物品与资源" : "事件时间线"}</h3>
                      <p>仅显示已通过正式正文中有原文出处的记录。点击章节可回到正式内容。</p>
                      {storyIndexStatus.approved > 0 && (
                        <div className={storyIndexStatus.failed.length > 0 ? "library-index-status has-error" : "library-index-status"}>
                          <span>资料索引</span>
                          <strong>已覆盖 {storyIndexStatus.succeeded}/{storyIndexStatus.approved} 章</strong>
                          {storyIndexStatus.pending > 0 && <small>{storyIndexStatus.pending} 章待更新</small>}
                          {storyIndexStatus.failed.length > 0 && (
                            <small title={storyIndexStatus.failed.map(({ chapter, source }) => `第 ${chapter.chapter_no} 章：${source?.error ?? "更新失败"}`).join("\n")}>
                              {storyIndexStatus.failed.length} 章更新失败
                            </small>
                          )}
                        </div>
                      )}
                    </div>
                    <button onClick={rebuildLibraryIndex} disabled={!detail || Boolean(busy)}>
                      <RefreshCcw size={14} /> 更新资料索引
                    </button>
                  </header>

                  {libraryFocus === "events" ? (
                    <div className="event-timeline">
                      {(detail?.story_events ?? []).map((event: StoryEvent) => {
                        const chapter = detail?.chapters.find((item) => item.id === event.narrative_chapter_id);
                        const participants = participantsByEvent.get(event.id) ?? [];
                        return (
                          <article key={event.id} className="timeline-event">
                            <div className="timeline-marker" />
                            <div className="timeline-event-body">
                              <div className="timeline-event-head">
                                <div><span>{chapter ? `第 ${chapter.chapter_no} 章` : "章节未定"}</span><strong>{event.title}</strong></div>
                                {event.story_time && <small>{event.story_time}</small>}
                              </div>
                              <p>{event.summary}</p>
                              {participants.length > 0 && (
                                <div className="timeline-participants">
                                  {participants.map((participant) => (
                                    <button key={`${participant.event_id}-${participant.entity_id}-${participant.role}`} onClick={() => {
                                      openLibrary("characters");
                                      setSelectedLibraryEntityId(participant.entity_id);
                                    }}>
                                      {participant.entity_name}<span>{participant.role}</span>
                                    </button>
                                  ))}
                                </div>
                              )}
                              <blockquote>{event.source_quote}</blockquote>
                              {chapter && <button className="timeline-source" onClick={() => openTimelineChapter(chapter.id)}>查看正式正文</button>}
                            </div>
                          </article>
                        );
                      })}
                      {(detail?.story_events?.length ?? 0) === 0 && <div className="empty-state compact">暂无事件索引。通过章节正文后会自动更新，也可手动更新索引。</div>}
                    </div>
                  ) : (
                    <div className="entity-timeline-layout">
                      <nav className="entity-list" aria-label={libraryFocus === "characters" ? "角色列表" : "物品与资源列表"}>
                        {visibleTimelineEntities.map((entity: StoryEntity) => (
                          <button
                            key={entity.id}
                            className={entity.id === selectedLibraryEntity?.id ? "active" : ""}
                            onClick={() => setSelectedLibraryEntityId(entity.id)}
                          >
                            <strong>{entity.name}</strong>
                            <span>{entity.kind === "character" ? "角色" : entity.kind === "resource" ? "资源" : "物品"}</span>
                          </button>
                        ))}
                        {visibleTimelineEntities.length === 0 && <div className="empty-inline">暂无可用索引</div>}
                      </nav>

                      <section className="entity-detail">
                        {selectedLibraryEntity ? (
                          <>
                            <header className="entity-detail-head">
                              <div>
                                <span>{selectedLibraryEntity?.kind === "character" ? "角色" : selectedLibraryEntity?.kind === "resource" ? "资源" : "物品"}</span>
                                <h4>{selectedLibraryEntity?.name}</h4>
                              </div>
                              <small>截至已索引章节</small>
                            </header>
                            {selectedEntityCurrentFacts.length > 0 && (
                              <div className="entity-current-state">
                                {selectedEntityCurrentFacts.map((fact) => (
                                  <div key={fact.dimension}>
                                    <span>{fact.dimension}</span>
                                    <strong>{fact.value}</strong>
                                  </div>
                                ))}
                              </div>
                            )}
                            <div className="entity-timeline">
                              {selectedEntityTimeline.map((entry) => {
                                const chapter = detail?.chapters.find((item) => item.id === entry.chapterId);
                                if (entry.type === "event") {
                                  return (
                                    <article className="entity-timeline-entry event" key={`event-${entry.id}`}>
                                      <span className="entity-timeline-chapter">{chapter ? `第 ${chapter.chapter_no} 章` : "章节未定"}</span>
                                      <strong>{entry.event.title}</strong>
                                      <p>{entry.event.summary}</p>
                                      <blockquote>{entry.event.source_quote}</blockquote>
                                      {chapter && <button className="timeline-source" onClick={() => openTimelineChapter(chapter.id)}>查看正式正文</button>}
                                    </article>
                                  );
                                }
                                return (
                                  <article className="entity-timeline-entry" key={`fact-${entry.id}`}>
                                    <span className="entity-timeline-chapter">{chapter ? `第 ${chapter.chapter_no} 章` : "章节未定"}</span>
                                    <strong>{entry.fact.dimension}</strong>
                                    <p>{entry.fact.value}</p>
                                    <blockquote>{entry.fact.source_quote}</blockquote>
                                    {chapter && <button className="timeline-source" onClick={() => openTimelineChapter(chapter.id)}>查看正式正文</button>}
                                  </article>
                                );
                              })}
                              {selectedEntityTimeline.length === 0 && <div className="empty-inline">尚无该实体的状态变化记录</div>}
                            </div>
                          </>
                        ) : <div className="empty-state compact">选择一个{libraryFocus === "characters" ? "角色" : "物品"}查看时间线。</div>}
                      </section>
                    </div>
                  )}
                </section>
              )}

              {!["characters", "items", "events"].includes(libraryFocus) && (
              <div className="library-layout">
                <nav className="library-nav" aria-label="书籍资料分类">
                  {foundationStages.map((stage) => (
                    <button
                      key={stage.id}
                      className={librarySection === stage.id ? "active" : ""}
                      onClick={() => {
                        setLibrarySection(stage.id as LibrarySection);
                        setSelectedStage(stage.id);
                        setSelectedArtifactId(null);
                        setShowKnowledgeComposer(false);
                      }}
                    >
                      <strong>{stage.label}</strong>
                      <span>{stage.id === "setting" ? "世界、规则与边界" : stage.id === "outline" ? "章节任务与推进" : "角色卡与关系"}</span>
                    </button>
                  ))}
                </nav>

                <section className="library-canvas">
                  <div className="library-canvas-head">
                    <div>
                      <h3>{librarySection === "setting" ? "设定资料" : librarySection === "outline" ? "章节大纲" : "角色卡"}</h3>
                      <p>{libraryArtifact ? "当前确认版本已拆分为可阅读卡片。" : "尚未生成该类资料。"}</p>
                    </div>
                    <button onClick={() => runAgent(librarySection)} disabled={!detail || Boolean(busy)}>
                      <RefreshCcw size={14} /> {libraryArtifact ? "迭代资料" : "生成资料"}
                    </button>
                  </div>

                  {showKnowledgeComposer && (
                    <section className="library-composer">
                      <div className="library-composer-head">
                        <strong>补充{librarySection === "setting" ? "设定" : librarySection === "outline" ? "大纲任务" : "角色"}</strong>
                        <button className="icon-btn" onClick={() => setShowKnowledgeComposer(false)} title="关闭">
                          <ChevronLeft size={15} />
                        </button>
                      </div>
                      {librarySection === "setting" && (
                        <select value={knowledgeCategory} onChange={(event) => setKnowledgeCategory(event.target.value)}>
                          <option value="world">世界观</option>
                          <option value="cultivation">修行体系</option>
                          <option value="map">地图与地点</option>
                          <option value="faction">势力与组织</option>
                          <option value="taboo">禁忌与边界</option>
                          <option value="item">重要物件</option>
                        </select>
                      )}
                      <input value={knowledgeTitle} onChange={(event) => setKnowledgeTitle(event.target.value)} placeholder="资料标题" />
                      <textarea rows={5} value={knowledgeContent} onChange={(event) => setKnowledgeContent(event.target.value)} placeholder="写明可确认的事实、边界和后续可用方式" />
                      <div className="button-row">
                        <button onClick={() => saveKnowledgeCard("pending_human_approval")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>保存待确认</button>
                        <button className="btn-primary" onClick={() => saveKnowledgeCard("approved")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>确认并启用</button>
                      </div>
                    </section>
                  )}

                  <div className="knowledge-grid">
                    {librarySourceSections.map((section) => <KnowledgeSectionCard key={section.title} section={section} />)}
                    {libraryCards.map((card) => (
                      <KnowledgeSectionCard key={card.id} section={{ title: card.title, content: card.content.split("\n") }} />
                    ))}
                    {librarySourceSections.length === 0 && libraryCards.length === 0 && (
                      <div className="empty-state">尚无资料。先生成一版，再由人工逐步补充和确认。</div>
                    )}
                  </div>
                </section>

                <aside className="library-side">
                  <section className="library-side-panel">
                    <div className="panel-title"><FileText size={14} /> 当前章节引用</div>
                    <strong>{selectedChapter?.title ?? "未选择章节"}</strong>
                    <p>{selectedChapter ? "正文生产会自动读取已确认资料与相关伏笔。" : "选择章节后查看该章会引用的资料。"}</p>
                  </section>
                  <section className="library-side-panel foreshadowing-panel">
                    <div className="panel-title"><Sparkles size={14} /> 伏笔账本</div>
                    <button onClick={() => setShowForeshadowingComposer((current) => !current)} disabled={!detail || Boolean(busy)}>
                      <Plus size={14} /> 登记伏笔
                    </button>
                    {showForeshadowingComposer && (
                      <div className="foreshadowing-composer">
                        <input value={foreshadowingTitle} onChange={(event) => setForeshadowingTitle(event.target.value)} placeholder="伏笔标题" />
                        <textarea rows={3} value={foreshadowingContent} onChange={(event) => setForeshadowingContent(event.target.value)} placeholder="埋设内容与读者当前应感知到的信息" />
                        <select value={foreshadowingPayoffChapterId ?? ""} onChange={(event) => setForeshadowingPayoffChapterId(Number(event.target.value) || null)}>
                          <option value="">预期回收章节（可稍后指定）</option>
                          {detail?.chapters.map((chapter) => <option key={chapter.id} value={chapter.id}>{chapter.title}</option>)}
                        </select>
                        <input value={foreshadowingPayoffNote} onChange={(event) => setForeshadowingPayoffNote(event.target.value)} placeholder="或填写回收里程碑" />
                        <div className="button-row">
                          <button onClick={() => saveForeshadowing("pending_human_approval")} disabled={!foreshadowingTitle.trim() || !foreshadowingContent.trim() || Boolean(busy)}>保存</button>
                          <button className="btn-primary" onClick={() => saveForeshadowing("active")} disabled={!foreshadowingTitle.trim() || !foreshadowingContent.trim() || Boolean(busy)}>确认追踪</button>
                        </div>
                      </div>
                    )}
                    <div className="foreshadowing-list">
                      {visibleForeshadowings.map((item: Foreshadowing) => {
                        const payoffChapter = detail?.chapters.find((chapter) => chapter.id === item.planned_payoff_chapter_id);
                        return (
                          <article key={item.id} className="foreshadowing-item">
                            <strong>{item.title}</strong>
                            <p>{item.content}</p>
                            <span>{payoffChapter?.title ?? (item.planned_payoff_note || "尚未安排回收")}</span>
                          </article>
                        );
                      })}
                      {visibleForeshadowings.length === 0 && <div className="empty-inline">还没有登记伏笔</div>}
                    </div>
                  </section>
                </aside>
              </div>
              )}
            </section>
          ) : (
            <>
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
                  <button className="sidebar-action" onClick={() => setShowNewProjectModal(true)}>
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
              <h1>{detail?.project.title ?? "选择或新建一个小说项目"}</h1>
              {detail && (
                <div className="project-tags" aria-label="书籍信息">
                  <span>{detail.project.genre || "未设置题材"}</span>
                  <span>预计 {detail.project.target_words.toLocaleString()} 字</span>
                  <span>{detail.chapters.length} 章</span>
                </div>
              )}
            </div>
            {!detail && <p>设定、大纲、角色、写作、试读、修订，每一步都由人工确认推进。</p>}
            {detail && (
              <div className="surface-switch" role="tablist" aria-label="内容区域">
                <button
                  className={visibleContentSurface === "official" ? "active" : ""}
                  onClick={() => setMainSurface("official")}
                >
                  <BookOpen size={14} /> 正式内容
                </button>
                <button
                  className={visibleContentSurface === "workbench" ? "active" : ""}
                  onClick={() => setMainSurface("workbench")}
                >
                  <Sparkles size={14} /> 创作工作台
                </button>
              </div>
            )}
          </div>
          <div className="topbar-actions">
            <button onClick={() => refreshDetail()} disabled={!detail || Boolean(busy)}>
              <RefreshCcw size={14} /> 刷新
            </button>
            <button onClick={() => setShowProjectEditor(true)} disabled={!detail || Boolean(busy)}>
              <Edit3 size={14} /> 编辑书籍
            </button>
            <button className="icon-btn" onClick={() => setShowNewProjectModal(true)} title="新建书籍">
              <Plus size={18} />
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
              <div className="section-head">
                <h2>书籍资料</h2>
              </div>
              <div className="library-shortcuts">
                {foundationStages.map((stage) => (
                  <button
                    key={stage.id}
                    className={mainSurface === "library" && libraryFocus === stage.id ? "active" : ""}
                    onClick={() => openLibrary(stage.id as LibrarySection)}
                  >
                    <span>{stage.label}</span>
                  </button>
                ))}
                <button
                  className={mainSurface === "library" && libraryFocus === "items" ? "active" : ""}
                  onClick={() => openLibrary("items")}
                >
                  <span>物品</span>
                  <small>{detail?.story_entities?.filter((entity) => entity.kind === "item" || entity.kind === "resource").length ?? 0}</small>
                </button>
                <button
                  className={mainSurface === "library" && libraryFocus === "events" ? "active" : ""}
                  onClick={() => openLibrary("events")}
                >
                  <span>事件</span>
                  <small>{detail?.story_events?.length ?? 0}</small>
                </button>
                <button
                  className={mainSurface === "library" && libraryFocus === "foreshadowing" ? "active" : ""}
                  onClick={() => openLibrary()}
                >
                  <span>伏笔</span>
                  <small>{detail?.foreshadowings?.filter((item) => item.status !== "resolved").length ?? 0}</small>
                </button>
              </div>

              <div className="section-head">
                <h2>章节</h2>
                <details
                  ref={chapterToolsRef}
                  className="lane-tools"
                  onToggle={(event) => setChapterToolsOpen(event.currentTarget.open)}
                >
                  <summary>管理</summary>
                </details>
              </div>
            </div>
            <div className="lane-scroll">
              <div className="chapter-list">
                {detail?.chapters.map((chapter) => (
                  <button
                    key={chapter.id}
                    className={
                      mainSurface !== "library" && chapter.id === selectedChapterId
                        ? "chapter active"
                        : "chapter"
                    }
                    onClick={() => selectChapter(chapter)}
                  >
                    <span>{chapter.title}</span>
                    {mainSurface === "workbench" && <small>{chapter.current_artifact_id ? "已采纳" : "待创作"}</small>}
                  </button>
                ))}
                {detail && detail.chapters.length === 0 && (
                  <p className="empty-hint">暂无章节</p>
                )}
              </div>

              {mainSurface === "workbench" && (
                <>
                  <div className="section-head">
                    <h2>流水线</h2>
                  </div>
                  <div className="stage-list">
                    {productionStages.map((stage) => (
                      <button
                        key={stage.id}
                        className={stage.id === selectedStage ? "stage active" : "stage"}
                        onClick={() => {
                          setSelectedStage(stage.id);
                          setSelectedArtifactId(null);
                        }}
                      >
                        <span>{stage.label}</span>
                      </button>
                    ))}
                  </div>
                </>
              )}
            </div>
          </section>

          {chapterToolsOpen && createPortal(
            <div
              className="lane-tools-menu"
              style={{ top: chapterToolsPosition.top, left: chapterToolsPosition.left }}
            >
              <button onClick={createChapter} disabled={!detail || Boolean(busy)}>
                <Plus size={14} /> 新增章节
              </button>
              <button onClick={continueNextChapter} disabled={!detail || Boolean(busy)}>
                <Play size={14} /> 下一章
              </button>
              <button onClick={renameCurrentChapter} disabled={!selectedChapter || Boolean(busy)}>
                <Save size={14} /> 重命名
              </button>
              <button
                className="danger"
                onClick={deleteCurrentChapter}
                disabled={!selectedChapter || Boolean(busy)}
              >
                <Trash2 size={14} /> 删除章节
              </button>
              {mainSurface === "workbench" && (
                <button onClick={clearSelectedChapterHistory} disabled={!selectedChapter || Boolean(busy)}>
                  <Trash2 size={14} /> 清历史
                </button>
              )}
              <input
                value={chapterDraft}
                onChange={(event) => setChapterDraft(event.target.value)}
                placeholder={selectedChapter ? `当前：${selectedChapter.title}` : "新章节标题"}
              />
            </div>,
            document.body
          )}

          {mainSurface === "official" ? (
            <section className="editor official-editor">
              <div className="editor-toolbar">
                <div>
                  <h2>正式内容</h2>
                  <p>{selectedChapter ? selectedChapter.title : "选择章节查看已采纳正文"}</p>
                </div>
                <div className="button-row">
                  {currentChapterBody && (
                    <button className="icon-btn" onClick={copyCurrentChapterBody} title="复制本章正文" aria-label="复制本章正文">
                      <Copy size={16} />
                    </button>
                  )}
                  <button onClick={() => setMainSurface("workbench")} disabled={!detail}>
                    <Sparkles size={14} /> 进入创作工作台
                  </button>
                </div>
              </div>
              <article className="official-manuscript">
                {selectedChapter ? (
                  currentChapterBody ? (
                    <>
                      <pre>{currentChapterBody.content}</pre>
                    </>
                  ) : (
                    <div className="official-empty">
                      <strong>本章还没有正式正文</strong>
                      <span>草稿和修订稿需要在创作工作台里人工采纳后，才会进入这里。</span>
                      <button onClick={() => setMainSurface("workbench")}>
                        <Sparkles size={14} /> 去工作台处理候选稿
                      </button>
                    </div>
                  )
                ) : (
                  <div className="official-empty">
                    <strong>未选择章节</strong>
                    <span>选择章节后可查看已采纳正文。</span>
                  </div>
                )}
              </article>
            </section>
          ) : mainSurface === "library" && ["characters", "items", "events"].includes(libraryFocus) ? (
            <ContinuityLibraryPanel
              focus={libraryFocus as "characters" | "items" | "events"}
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
                <div>
                  <h2>创作基准</h2>
                  <p>故事架构 Agent 统一维护世界、阶段大纲、角色与伏笔；资料视图会随创作逐步扩展。</p>
                </div>
                <button onClick={() => (showKnowledgeComposer ? resetKnowledgeComposer() : setShowKnowledgeComposer(true))} disabled={!detail || Boolean(busy)}>
                  <Plus size={14} /> 补充资料
                </button>
              </header>

              <section className="story-bible-overview">
                <div className="story-bible-summary">
                  <div>
                    <span className={`library-status ${detail?.story_bible?.status ?? "draft"}`}>
                      {detail?.story_bible?.status === "confirmed" ? "已确认" : "待确认"}
                    </span>
                    <h3>故事架构 Agent</h3>
                    <p>{activeStoryArc ? `当前阶段：${activeStoryArc.title}` : "尚未确认进行中的故事阶段"}</p>
                  </div>
                  <div className="story-bible-actions">
                    <button
                      className="btn-primary"
                      onClick={() => runStoryArchitect(libraryArtifact ? "refine_canon" : "initialize")}
                      disabled={!detail || Boolean(busy)}
                    >
                      <Sparkles size={14} /> {libraryArtifact ? "补充创作基准" : "初始化创作基准"}
                    </button>
                    <button onClick={() => runStoryArchitect("plan_current_arc")} disabled={!detail || Boolean(busy)}>
                      <Rows3 size={14} /> 细化当前阶段
                    </button>
                    <details className="toolbar-more">
                      <summary>更多</summary>
                      <div className="toolbar-more-menu">
                        <button onClick={() => runStoryArchitect("extend_next_arc")} disabled={!detail || Boolean(busy)}><ChevronRight size={14} /> 扩展下一阶段</button>
                        <button onClick={() => runStoryArchitect("design_characters")} disabled={!detail || Boolean(busy)}><MessageSquare size={14} /> 补充角色</button>
                      </div>
                    </details>
                  </div>
                </div>
                <div className="story-bible-status-row">
                  <span>基准版本 v{detail?.story_bible?.canon_version ?? 0}</span>
                  <span>{detail?.story_arcs?.length ?? 0} 个故事阶段</span>
                  <span>{detail?.story_bible_review?.status === "confirmed" ? "一致性已确认" : "一致性待审校"}</span>
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
                <div className="story-bible-review-row">
                  <input value={storyBibleNote} onChange={(event) => setStoryBibleNote(event.target.value)} placeholder="人工确认备注，可为空" />
                  {!detail?.story_bible || detail.story_bible.status !== "confirmed" ? (
                    <button onClick={confirmStoryBible} disabled={!detail || Boolean(busy)}><Check size={14} /> 确认创作基准</button>
                  ) : detail.story_bible_review?.status === "pending_human_confirmation" ? (
                    <button onClick={confirmStoryBibleReview} disabled={Boolean(busy)}><Check size={14} /> 确认审校结论</button>
                  ) : (
                    <button onClick={reviewStoryBible} disabled={Boolean(busy)}><Eye size={14} /> 审校一致性</button>
                  )}
                </div>
                {detail?.story_bible_review && (
                  <details className="story-bible-review" open={detail.story_bible_review.status === "pending_human_confirmation"}>
                    <summary>一致性审校 · {detail.story_bible_review.verdict} · {detail.story_bible_review.issues.length} 项</summary>
                    <p>{detail.story_bible_review.summary}</p>
                    {detail.story_bible_review.issues.map((issue, index) => (
                      <article key={`${issue.title}-${index}`} className={`canon-issue ${issue.severity}`}>
                        <strong>{issue.title}</strong><span>{issue.domain} · {issue.severity}</span>
                        <p>{issue.conflict}</p><p>{issue.impact}</p>
                        <button onClick={() => createTargetedRework(issue)} disabled={Boolean(busy)}><RefreshCcw size={14} /> 交给故事架构 Agent 修复</button>
                      </article>
                    ))}
                  </details>
                )}
              </section>

              <div className="library-layout">
                <section className="library-canvas">
                  <div className="library-canvas-head">
                    <div>
                      <h3>{librarySection === "setting" ? "设定资料" : librarySection === "outline" ? "章节大纲" : "角色卡"}</h3>
                      <p>{libraryArtifact ? "当前确认版本已拆分为可阅读卡片。" : "尚未生成该类资料。"}</p>
                    </div>
                    <button onClick={() => runAgent(librarySection)} disabled={!detail || Boolean(busy)}>
                      <RefreshCcw size={14} /> {libraryArtifact ? "用故事架构 Agent 迭代" : "用故事架构 Agent 生成"}
                    </button>
                  </div>

                  {showKnowledgeComposer && (
                    <section className="library-composer">
                      <div className="library-composer-head">
                        <strong>{editingKnowledgeCardId ? "编辑资料卡" : `补充${librarySection === "setting" ? "设定" : librarySection === "outline" ? "大纲任务" : "角色"}`}</strong>
                        <button className="icon-btn" onClick={resetKnowledgeComposer} title="关闭"><ChevronLeft size={15} /></button>
                      </div>
                      {librarySection === "setting" && (
                        <select value={knowledgeCategory} onChange={(event) => setKnowledgeCategory(event.target.value)}>
                          <option value="world">世界观</option><option value="cultivation">修行体系</option><option value="map">地图与地点</option><option value="faction">势力与组织</option><option value="taboo">禁忌与边界</option><option value="item">重要物件</option>
                        </select>
                      )}
                      <input value={knowledgeTitle} onChange={(event) => setKnowledgeTitle(event.target.value)} placeholder="资料标题" />
                      <textarea rows={5} value={knowledgeContent} onChange={(event) => setKnowledgeContent(event.target.value)} placeholder="写明可确认的事实、边界和后续可用方式" />
                      <div className="button-row">
                        <button onClick={() => saveKnowledgeCard("pending_human_approval")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>保存待确认</button>
                        <button className="btn-primary" onClick={() => saveKnowledgeCard("approved")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>确认并启用</button>
                      </div>
                    </section>
                  )}

                  <div className="knowledge-grid">
                    {librarySourceSections.map((section) => <KnowledgeSectionCard key={section.title} section={section} />)}
                    {libraryCards.map((card) => (
                      <article className="managed-knowledge-card" key={card.id}>
                        <div className="managed-card-head">
                          <span className={`library-status ${card.status}`}>{card.status === "approved" ? "已确认" : card.status === "pending_human_approval" ? "待确认" : "已归档"}</span>
                          <div className="managed-card-actions">
                            <button className="icon-btn" onClick={() => editKnowledgeCard(card)} title="编辑资料卡"><Edit3 size={14} /></button>
                            {card.status === "pending_human_approval" && <button className="icon-btn" onClick={() => updateKnowledgeCardStatus(card, "approved")} title="确认并启用"><Check size={14} /></button>}
                            {card.status !== "archived" && <button className="icon-btn" onClick={() => updateKnowledgeCardStatus(card, "archived")} title="归档资料卡"><Trash2 size={14} /></button>}
                          </div>
                        </div>
                        <KnowledgeSectionCard section={{ title: card.title, content: card.content.split("\n") }} />
                      </article>
                    ))}
                    {librarySourceSections.length === 0 && libraryCards.length === 0 && <div className="empty-state">尚无资料。先生成一版，再由人工逐步补充和确认。</div>}
                  </div>
                </section>

                <aside className="library-side">
                  <section className="library-side-panel">
                    <div className="panel-title"><FileText size={14} /> 当前章节引用</div>
                    <strong>{selectedChapter?.title ?? "未选择章节"}</strong>
                    <p>{selectedChapter ? "正文生产会自动读取已确认资料与相关伏笔。" : "选择章节后查看该章会引用的资料。"}</p>
                  </section>
                  <section className="library-side-panel foreshadowing-panel">
                    <div className="panel-title"><Sparkles size={14} /> 伏笔账本</div>
                    <button onClick={() => (showForeshadowingComposer ? resetForeshadowingComposer() : setShowForeshadowingComposer(true))} disabled={!detail || Boolean(busy)}><Plus size={14} /> 登记伏笔</button>
                    {showForeshadowingComposer && (
                      <div className="foreshadowing-composer">
                        <strong>{editingForeshadowingId ? "编辑伏笔" : "登记伏笔"}</strong>
                        <input value={foreshadowingTitle} onChange={(event) => setForeshadowingTitle(event.target.value)} placeholder="伏笔标题" />
                        <textarea rows={3} value={foreshadowingContent} onChange={(event) => setForeshadowingContent(event.target.value)} placeholder="埋设内容与读者当前应感知到的信息" />
                        <select value={foreshadowingPayoffChapterId ?? ""} onChange={(event) => setForeshadowingPayoffChapterId(Number(event.target.value) || null)}>
                          <option value="">预期回收章节（可稍后指定）</option>
                          {detail?.chapters.map((chapter) => <option key={chapter.id} value={chapter.id}>{chapter.title}</option>)}
                        </select>
                        <input value={foreshadowingPayoffNote} onChange={(event) => setForeshadowingPayoffNote(event.target.value)} placeholder="或填写回收里程碑" />
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
                            <div className="managed-card-head"><span className={`library-status ${item.status}`}>{statusLabel}</span><div className="managed-card-actions">
                              <button className="icon-btn" onClick={() => editForeshadowing(item)} title="编辑伏笔"><Edit3 size={14} /></button>
                              {item.status === "pending_human_approval" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "active")} title="确认追踪"><Check size={14} /></button>}
                              {item.status === "active" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "ready_for_payoff")} title="标记可回收"><Sparkles size={14} /></button>}
                              {item.status === "ready_for_payoff" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "resolved")} title="标记已回收"><Check size={14} /></button>}
                            </div></div>
                            <strong>{item.title}</strong><p>{item.content}</p><span>{payoffChapter?.title ?? (item.planned_payoff_note || "尚未安排回收")}</span>
                          </article>
                        );
                      })}
                      {visibleForeshadowings.length === 0 && <div className="empty-inline">还没有登记伏笔</div>}
                    </div>
                  </section>
                </aside>
              </div>
            </section>
          ) : (
            <>
          {/* Center: Editor */}
          <section className="editor">
            <div className="editor-toolbar">
              <div>
                <h2>{stages.find((stage) => stage.id === selectedStage)?.label}</h2>
                <p>{selectedChapter ? selectedChapter.title : "整书资料"}</p>
              </div>
              <div className="button-row">
                <button onClick={() => runAgent(selectedStage)} disabled={!detail || Boolean(busy)}>
                  <Play size={14} /> {selectedBookArtifactCanIterate ? "基于当前版本迭代" : "生成"}
                </button>
                <button
                  className="btn-primary"
                  onClick={approveArtifact}
                  disabled={!selectedArtifact || selectedArtifactApproved || Boolean(busy)}
                >
                  <Check size={14} /> {selectedArtifactApproved ? "已通过" : "人工通过"}
                </button>
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
                <details className="toolbar-more">
                  <summary>更多</summary>
                  <div className="toolbar-more-menu">
                    <button
                      onClick={() => runAgent(selectedStage, "fresh")}
                      disabled={!detail || Boolean(busy)}
                    >
                      <RefreshCcw size={14} /> {selectedBookArtifactCanIterate ? "整版重写" : "重新生成"}
                    </button>
                    <button
                      onClick={deleteSelectedArtifact}
                      disabled={!selectedArtifact || Boolean(busy) || Boolean(selectedArtifactDeleteBlockReason)}
                      title={selectedArtifactDeleteBlockReason ?? "删除当前版本"}
                    >
                      <Trash2 size={14} />
                      {selectedArtifactDeleteBlockReason ?? "删除当前版本"}
                    </button>
                  </div>
                </details>
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
                    v{artifact.version} · {artifact.status}
                    {selectedChapter?.current_artifact_id === artifact.id ? " · 当前正文" : ""}
                  </button>
                ))}
              </div>

              {selectedArtifact && visibleArtifacts.length > 1 && (
                <div className="compare-toolbar">
                  <span>对比基准</span>
                  <select
                    value={compareArtifactId ?? ""}
                    onChange={(event) => setCompareArtifactId(Number(event.target.value) || null)}
                  >
                    <option value="">不对比</option>
                    {visibleArtifacts
                      .filter((artifact) => artifact.id !== selectedArtifact.id)
                      .map((artifact) => (
                        <option key={artifact.id} value={artifact.id}>
                          v{artifact.version} · {artifact.status}
                        </option>
                      ))}
                  </select>
                </div>
              )}
            </details>

            <article className={streamingRun ? "artifact-view streaming-artifact" : "artifact-view"}>
              {streamingRun ? (
                <>
                  <div className="artifact-meta">
                    <strong>{stageLabel(streamingRun.stage)}正在生成</strong>
                    <span>实时草稿，完成后才会形成待人工确认版本</span>
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
                    <div className="empty-inline">
                      当前阶段支持局部 agent 迭代：左侧这个版本会作为上下文保留，配合右侧人工指令只改局部；点“整版重写”则忽略当前版本重做。
                    </div>
                  )}
                  <pre>{selectedArtifact.content}</pre>
                </>
              ) : (
                <div className="empty-state">当前阶段还没有产物</div>
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
                <div className="empty-state compact">试读结果还不是结构化 JSON，先看原文。</div>
              )}
            </div>
            )}
            {selectedArtifact?.stage === "review" &&
              ledgerContinuityReport &&
              selectedArtifact.parent_artifact_id === ledgerContinuityReport.artifact_id && (
              <section className="ledger-report" aria-label="状态账本核对结果">
                <div className="ledger-report-head">
                  <strong>状态账本核对</strong>
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
          </section>

          {/* Right: Assistant Panel */}
          <aside className="assistant-panel">
            <section className="panel next-action-panel">
              <div className="panel-title">
                <MessageSquare size={14} />
                下一步：{stageLabel(selectedStage)}
              </div>
              <p className="next-action-copy">
                {selectedArtifactApproved
                  ? "这个版本已由人工确认。选择下一阶段，或继续做小范围迭代。"
                  : selectedArtifact
                    ? "先阅读当前候选稿，补充必要指令后再生成或人工确认。"
                    : "给 Agent 一条清晰的本章目标，再生成第一个候选版本。"}
              </p>
              <textarea
                rows={4}
                value={instruction}
                placeholder={
                  selectedBookArtifactCanIterate
                    ? "追加局部修改指令，例如：只调整第 3 节的修炼规则，不改前两节结构"
                    : "追加给下一次 Agent 的人工指令"
                }
                onChange={(event) => setInstruction(event.target.value)}
              />
              <button
                className="btn-primary"
                onClick={() => runAgent(selectedStage)}
                disabled={!detail || Boolean(busy)}
              >
                <Sparkles size={14} /> {selectedBookArtifactCanIterate ? "带指令局部迭代" : "带指令运行"}
              </button>
              <button
                className="secondary-action"
                onClick={previewAgentContext}
                disabled={!detail || Boolean(busy)}
              >
                <Eye size={14} /> 查看生成上下文
              </button>
              {contextPreview && (
                <section className="context-preview-panel">
                  <div className="context-preview-head">
                    <div>
                      <strong>{contextPreview.genre_agent.name}将使用的上下文</strong>
                      <span>{contextPreview.total_chars.toLocaleString()} 字符 · 约 {contextPreview.estimated_tokens.toLocaleString()} tokens</span>
                    </div>
                    <button className="icon-btn" onClick={() => setContextPreview(null)} title="关闭上下文预览" aria-label="关闭上下文预览">×</button>
                  </div>
                  <p className="context-preview-note">这是只读预览，内容来自已批准资料、章节状态、历史检索和当前人工指令；不会修改数据库。</p>
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
              {canRunReview && selectedArtifact?.stage !== "review" && (
                <button
                  className="secondary-action"
                  onClick={() => runAgent("review")}
                  disabled={!detail || Boolean(busy)}
                >
                  <Play size={14} /> 提交试读
                </button>
              )}
              {selectedArtifact?.stage === "review" && (
                <>
                  <textarea
                    rows={4}
                    value={revisionFeedback}
                    placeholder="决定采纳哪些试读意见，或写下你的修订要求"
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
                    <Check size={14} /> 人工通过{selectedArtifact.stage === "draft" || selectedArtifact.stage === "revision" ? "并应用正文" : ""}
                  </button>
                </>
              )}
            </section>

            <details className="advanced-tools">
              <summary>检查、版本、检索与更多工具</summary>
              <div className="advanced-tools-content">
            <section className="panel">
              <div className="panel-title">
                <RefreshCcw size={14} />
                修订与局部修改
              </div>
              <button onClick={() => runAgent("review")} disabled={!detail || !canRunReview || Boolean(busy)}>
                <Play size={14} /> 提交试读
              </button>
              <textarea
                rows={5}
                value={revisionFeedback}
                placeholder="人工修订要求，或对试读意见的取舍"
                onChange={(event) => setRevisionFeedback(event.target.value)}
              />
              <button onClick={requestRevision} disabled={!canRequestRevision || Boolean(busy)}>
                <RefreshCcw size={14} /> 请求修订
              </button>
              <div className="local-patch-tool">
                <div className="tool-subtitle">局部版本修订</div>
                <textarea
                  rows={4}
                  value={patchFindText}
                  placeholder="原文片段：从当前产物里复制一段唯一文本"
                  onChange={(event) => setPatchFindText(event.target.value)}
                />
                <textarea
                  rows={4}
                  value={aiPatchInstruction}
                  placeholder="AI 局部修订要求：例如只加强这段对白对撞，不改其他段落"
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
                  <Sparkles size={14} /> AI 局部修订
                </button>
                <textarea
                  rows={4}
                  value={patchReplaceText}
                  placeholder="替换为：只写这段的新文本，可留空表示删除"
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
                  <Edit3 size={14} /> 生成局部版本
                </button>
              </div>
            </section>

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
                <Check size={14} /> 检查通过前状态
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
                      <div className="empty-inline">没有发现硬阻断，仍需人工判断是否可用</div>
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

            <section className="panel">
              <div className="panel-title">
                <Search size={14} />
                历史检索
              </div>
              <textarea
                rows={3}
                value={contextQuery}
                placeholder="搜索旧人物、物件、线索或事件"
                onChange={(event) => setContextQuery(event.target.value)}
              />
              <button onClick={searchContext} disabled={!detail || !contextQuery.trim() || Boolean(busy)}>
                <Search size={14} /> 搜索数据库上下文
              </button>
              {contextSnippets.length > 0 && (
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
              )}
            </section>

            <section className="panel">
              <div className="panel-title">
                <Check size={14} />
                人工确认
              </div>
              <textarea
                rows={3}
                value={approvalNote}
                placeholder="确认备注，可为空"
                onChange={(event) => setApprovalNote(event.target.value)}
              />
              <button
                className="btn-primary"
                onClick={approveArtifact}
                disabled={!selectedArtifact || selectedArtifactApproved || Boolean(busy)}
              >
                <Check size={14} /> {selectedArtifactApproved ? "当前产物已通过" : "通过当前产物"}
              </button>
            </section>

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
                          ? `正在接收输出${run.output ? ` · ${run.output.length} 字符` : ""}`
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
                  Markdown
                </div>
                <textarea readOnly rows={6} value={exportText} />
              </section>
            )}
              </div>
            </details>
          </aside>
            </>
          )}
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
