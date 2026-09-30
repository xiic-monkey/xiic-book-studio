import {
  AlertCircle,
  Archive,
  BarChart3,
  BookOpen,
  Bot,
  Check,
  Copy,
  ChevronLeft,
  ChevronRight,
  Edit3,
  Download,
  Eye,
  FileText,
  History,
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
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, useTransition } from "react";
import type { ChangeEvent, KeyboardEvent, PointerEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "../../api";
import { ArtifactDiffPanel } from "../../components/ArtifactDiffPanel";
import { ContinuityLibraryPanel } from "../../components/ContinuityLibraryPanel";
import type { EntityTimelineEntry, StoryIndexStatus } from "../../components/ContinuityLibraryPanel";
import { KnowledgeSectionCard } from "../../components/KnowledgeSectionCard";
import { Select } from "../../components/Select";
import type {
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
  ChapterPlan,
  Foreshadowing,
  CanonEntry,
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
  SaveCanonEntryInput,
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

const bodyStages: string[] = ["revision", "draft"];
const adoptionActionLabel = "确认采用";
const adoptedStatusLabel = "已采用";

type ChapterFlowState = "empty" | "awaiting_review" | "needs_revision" | "ready_to_adopt" | "adopted";

type ChapterFlow = {
  state: ChapterFlowState;
  label: string;
  actionLabel: string;
  bodyArtifact: Pick<Artifact, "id" | "project_id" | "chapter_id" | "stage" | "title" | "version" | "status" | "parent_artifact_id" | "created_at"> | null;
  reviewArtifact: Artifact | null;
  reviewIssueCount: number;
};

function parseReviewIssues(content: string): ReviewIssue[] {
  try {
    const parsed: unknown = JSON.parse(content);
    return Array.isArray(parsed) ? parsed as ReviewIssue[] : [];
  } catch {
    return [];
  }
}

export function formatReviewInstructions(issues: ReviewIssue[]): string {
  if (issues.length === 0) return "";
  const details = issues.map((issue, index) => {
    const lines = [
      `${index + 1}. [${issue.severity}] ${issue.issue_type}`,
      `位置：${issue.location}`,
      `原因：${issue.reason}`,
      `修订建议：${issue.suggestion}`,
    ];
    if (issue.evidence_quote) lines.push(`依据：${issue.evidence_quote}`);
    if (issue.action_evidence_quote) lines.push(`动作依据：${issue.action_evidence_quote}`);
    return lines.join("\n");
  });
  return [
    "请根据当前候选稿的试读建议进行修订。",
    "优先处理以下问题，其他内容尽量保持不变。请生成新的候选稿，不要直接替换正式正文。",
    "",
    details.join("\n\n"),
  ].join("\n");
}

export function buildChapterAgentPrompt(
  projectTitle: string,
  chapterLabel: string,
  stage: Extract<Stage, "draft" | "review" | "revision">,
  extraInstruction = "",
) {
  const action = stage === "review"
    ? "提交一轮试读检查"
    : stage === "revision"
      ? "生成一版章节修订候选稿"
      : "生成一版新的章节候选版本";
  const prompt = `请由主 Agent 处理《${projectTitle}》的${chapterLabel}：${action}。请先确认任务类型，再委托对应的专业 Agent 执行。结果必须作为候选版本放入工作区供我确认采用，不要直接替换正式正文。`;
  const hint = extraInstruction.trim();
  return hint ? `${prompt}\n\n补充要求：${hint}` : prompt;
}

function artifactStageOr(value: string | null | undefined, fallback: Stage): Stage {
  return asStage(value) ?? fallback;
}

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

const foundationKnowledgeCategories = [
  "world",
  "cultivation",
  "map",
  "faction",
  "taboo",
  "item",
  "rule",
  "outline",
  "chapter_plan",
  "character",
];

function isFoundationCanonEntry(card: Pick<CanonEntry, "category" | "source_chapter_id">) {
  return card.source_chapter_id == null && foundationKnowledgeCategories.includes(card.category);
}

export function currentPlanStatus(
  workspace: (Pick<ProjectWorkspace, "story_bible" | "story_bible_review" | "canonical_fingerprint" | "canon_entries"> &
    Partial<Pick<ProjectWorkspace, "chapter_plans">>) | null,
) {
  if (!workspace) return { label: "待打开项目", tone: "idle" };
  const review = workspace.story_bible_review;
  const reviewIsCurrent = Boolean(review && review.canon_fingerprint === workspace.canonical_fingerprint);
  if (reviewIsCurrent && review?.issues.some((issue) => issue.severity === "major")) {
    return { label: "存在阻断问题", tone: "blocked" };
  }
  if (
    reviewIsCurrent
    && review?.status === "confirmed"
    && workspace.story_bible?.status === "confirmed"
  ) {
    return { label: adoptedStatusLabel, tone: "confirmed" };
  }
  if (reviewIsCurrent && review?.status === "pending_human_confirmation") {
    return { label: "待确认审校", tone: "awaiting" };
  }
  if (workspace.story_bible?.status === "confirmed" || workspace.story_bible?.status === "needs_review" || review) {
    return { label: "待审校", tone: "review" };
  }
  const hasFoundationCards = workspace.canon_entries.some((card) =>
    card.status !== "archived"
    && isFoundationCanonEntry(card),
  );
  const hasChapterPlans = (workspace.chapter_plans ?? []).some((plan) => plan.status !== "archived");
  return { label: hasFoundationCards || hasChapterPlans ? "待确认资料" : "待完善计划", tone: "draft" };
}

function pendingFoundationCardCount(workspace: ProjectWorkspace | null) {
  if (!workspace) return 0;
  return workspace.canon_entries.filter((card) =>
    card.status === "pending_human_approval"
    && isFoundationCanonEntry(card),
  ).length;
}

function pendingChapterPlanCount(workspace: ProjectWorkspace | null) {
  if (!workspace) return 0;
  return (workspace.chapter_plans ?? []).filter((plan) => plan.status === "pending_human_approval").length;
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
type LibraryFocus = LibrarySection | "character-timeline" | "items" | "events" | "foreshadowing";

const libraryFocusMeta: Record<LibraryFocus, { group: string; title: string; description: string }> = {
  setting: {
    group: "创作计划",
    title: "世界观",
    description: "维护故事发生所依赖的世界规则、地点与重要设定。",
  },
  outline: {
    group: "创作计划",
    title: "大纲",
    description: "安排故事阶段与章节任务，确认下一步要写什么。",
  },
  characters: {
    group: "创作计划",
    title: "角色",
    description: "维护角色目标、关系与创作阶段需要遵守的基准。",
  },
  "character-timeline": {
    group: "正文衍生资料",
    title: "角色时间线",
    description: "按角色查看正文中的状态变化与相关事件。",
  },
  events: {
    group: "正文衍生资料",
    title: "事件时间线",
    description: "按正文发生顺序浏览已采用章节中的关键事件。",
  },
  items: {
    group: "正文衍生资料",
    title: "物品状态",
    description: "追踪物品与资源在正文中的持有、位置和状态变化。",
  },
  foreshadowing: {
    group: "主动维护",
    title: "伏笔账本",
    description: "登记、跟进并回收创作者主动维护的线索与承诺。",
  },
};
type SettingsCategory = "ai" | "agents" | "skills" | "editor" | "data" | "appearance";
type AssistantMessageOptions = {
  sourceArtifactId?: number | null;
  stage?: Stage;
  storyArchitectMode?: StoryArchitectMode | null;
  referenceSelection?: ReferenceSelection | null;
};
type AssistantChatMessage = { id: string; role: "user" | "assistant"; content: string; order: number };
type AssistantThinkingRound = { id: string; content: string; active: boolean };
type AssistantTimelineItem =
  | { kind: "thinking"; id: string; content: string; active: boolean; sequence: number; order: number }
  | { kind: "tool"; id: string; toolKey: string; status: "running" | "success" | "failed" | "rejected"; summary?: string; invocationId?: number | null; sequence: number; order: number }
  | { kind: "output"; id: string; content: string; sequence: number; order: number }
  | { kind: "subagent"; id: string; runId: number; title: string; agentKey?: string | null; status: string; sequence: number; order: number };

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
const ASSISTANT_PANEL_WIDTH_STORAGE_KEY = "book-studio.agent-panel-width";
const ASSISTANT_PANEL_DEFAULT_WIDTH = 360;
const ASSISTANT_PANEL_MIN_WIDTH = 300;
const ASSISTANT_PANEL_MAX_WIDTH = 520;
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

function clampAssistantPanelWidth(width: number) {
  return Math.min(ASSISTANT_PANEL_MAX_WIDTH, Math.max(ASSISTANT_PANEL_MIN_WIDTH, width));
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
  search_story_facts: "检索故事事实",
  create_canon_entry: "写入资料",
  update_canon_entry: "更新资料",
  reference_materials: "读取参考资料",
  chapter_memory: "读取章节记忆",
  continuity_check: "检查连续性",
};

function assistantToolLabel(toolKey: string) {
  return assistantToolLabels[toolKey] ?? toolKey.replace(/[_-]+/g, " ");
}

function runStatusLabel(status: string) {
  if (status === "success") return "已完成";
  if (status === "failed") return "失败";
  if (status === "cancelled") return "已停止";
  if (["running", "streaming", "cancellation_requested"].includes(status)) return "执行中";
  return status;
}

function timelineToolStatus(status: string, hasError: boolean): "running" | "success" | "failed" | "rejected" {
  if (hasError) return "failed";
  if (status === "success") return "success";
  if (status === "rejected") return "rejected";
  if (["running", "started", "pending"].includes(status)) return "running";
  return "failed";
}

export function isTopLevelStreamingRun(run: ActiveAgentRun | null | undefined) {
  return Boolean(run && run.parent_run_id == null && run.run_kind !== "orchestrator");
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

function toolInvocationEvents(invocations: import("../../types").ToolInvocation[]): RunEvent[] {
  return invocations.flatMap((invocation, index) => {
    const summary = invocation.error
      || (typeof invocation.result?.summary === "string" ? invocation.result.summary : null)
      || (typeof invocation.result?.message === "string" ? invocation.result.message : null)
      || "工具已返回结果";
    const base = {
      run_id: invocation.run_id ?? 0,
      project_id: invocation.project_id,
      chapter_id: invocation.chapter_id,
      stage: invocation.stage,
      sequence: index * 2 + 1,
      tool_key: invocation.tool_key,
      tool_invocation_id: invocation.id,
      status: invocation.status,
      error: invocation.error,
      elapsed_ms: invocation.elapsed_ms,
      created_at: invocation.created_at,
      parent_run_id: null,
      agent_key: null,
      agent_role: null,
      task_title: null,
    };
    return [
      { ...base, kind: "tool_started" as const, delta: "" },
      { ...base, sequence: index * 2 + 2, kind: "tool_completed" as const, delta: summary },
    ];
  });
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

export function splitThinkingContent(content: string, active: boolean) {
  const trimmed = content.trim();
  const first = trimmed.split(/\n|。|！|？/)[0]?.trim() ?? "";
  const title = first.length > 52 ? `${first.slice(0, 52)}…` : first;
  const rest = first && trimmed.length > first.length
    ? trimmed.slice(first.length).replace(/^[。！？\n]+/, "")
    : "";
  return { title: title || (active ? "思考中" : "思考摘要"), rest };
}

function AssistantThinkingPanel({ content, active }: { content: string; active: boolean }) {
  const { title, rest } = splitThinkingContent(content, active);
  return (
    <details
      className={`assistant-thinking-panel${active ? " assistant-thinking-panel-current" : ""}`}
      open={active}
    >
      <summary>
        <span><Sparkles size={12} /> {title}</span>
        <small>{active ? "实时更新" : "已完成"}</small>
      </summary>
      {rest ? <p>{rest}</p> : null}
    </details>
  );
}

export function compactAssistantOutput(content: string) {
  const trimmed = content.trim();
  const completion = trimmed.match(/^(已完成\s+\d+\s+个子任务)[。.!！]?/);
  if (!completion) return content;

  // 委托运行的前言一律折叠成短状态行，摘录正文留在子任务详情与候选稿面板里。
  const remainder = trimmed.slice(completion[0].length).trim();
  let label = remainder.split(/[:：]/, 1)[0]?.trim() ?? "";
  if (label.length > 40) label = `${label.slice(0, 40)}…`;
  return label ? `${completion[1]} · ${label}` : completion[1];
}

export function compactAssistantModelName(model: string) {
  const trimmed = model.trim();
  const withoutProvider = trimmed.replace(/^(?:deepseek|qwen|openai|anthropic|google|mistral)[-_\/]+/i, "");
  return withoutProvider.replace(/[-_\/]+/g, " ") || trimmed;
}

const assistantEventOrderCache = new WeakMap<RunEvent, number>();

function assistantEventOrder(event: RunEvent) {
  // 排序比较器会反复调用：Date.parse 每个事件只算一次。
  const cached = assistantEventOrderCache.get(event);
  if (cached !== undefined) return cached;
  const timestamp = Date.parse(event.created_at);
  const order = (Number.isFinite(timestamp) ? timestamp : 0) * 1000 + event.sequence;
  assistantEventOrderCache.set(event, order);
  return order;
}

function buildAssistantTimeline(events: RunEvent[]): AssistantTimelineItem[] {
  const sorted = [...events].sort((a, b) => a.sequence - b.sequence);
  const items: AssistantTimelineItem[] = [];
  const openThinking: Extract<AssistantTimelineItem, { kind: "thinking" }>[] = [];
  const openTools = new Map<string, Extract<AssistantTimelineItem, { kind: "tool" }>>();
  for (const event of sorted) {
    if (event.parent_run_id && event.kind === "started") {
      items.push({
        kind: "subagent",
        id: `subagent-${event.run_id}`,
        runId: event.run_id,
        title: event.task_title || event.agent_role || event.agent_key || "子 Agent",
        agentKey: event.agent_key,
        status: event.status,
        sequence: event.sequence,
        order: assistantEventOrder(event),
      });
      continue;
    }
    if (event.kind === "thinking_start") {
      const item: AssistantTimelineItem = { kind: "thinking", id: `thinking-${event.sequence}`, content: "", active: true, sequence: event.sequence, order: assistantEventOrder(event) };
      items.push(item);
      openThinking.push(item);
      continue;
    }
    if (event.kind === "thinking_delta") {
      const current = openThinking[openThinking.length - 1];
      if (current && current.active) {
        current.content += event.delta;
        continue;
      }
      // start 缺失时兜底，避免逐 token 内容被丢弃。
      const item: AssistantTimelineItem = { kind: "thinking", id: `thinking-${event.sequence}`, content: event.delta, active: true, sequence: event.sequence, order: assistantEventOrder(event) };
      items.push(item);
      openThinking.push(item);
      continue;
    }
    if (event.kind === "thinking_end") {
      const current = openThinking.pop();
      if (current) current.active = false;
      continue;
    }
    if (event.kind === "tool_started" && event.tool_key) {
      const item: AssistantTimelineItem = { kind: "tool", id: `tool-${event.sequence}`, toolKey: event.tool_key, status: "running", invocationId: event.tool_invocation_id, sequence: event.sequence, order: assistantEventOrder(event) };
      items.push(item);
      openTools.set(event.tool_key, item);
      continue;
    }
    if (event.kind === "tool_completed" && event.tool_key) {
      const current = openTools.get(event.tool_key);
      if (current) {
        current.status = event.status === "success" ? "success" : event.status === "rejected" ? "rejected" : "failed";
        current.summary = event.delta || event.error || undefined;
        if (event.tool_invocation_id != null) current.invocationId = event.tool_invocation_id;
        openTools.delete(event.tool_key);
      }
      continue;
    }
    if ((event.kind === "output_delta" || event.kind === "output_reset") && event.delta) {
      if (event.kind === "output_reset") continue;
      const previous = items[items.length - 1];
      if (previous?.kind === "output") {
        previous.content += event.delta;
        previous.sequence = event.sequence;
        previous.order = assistantEventOrder(event);
      } else {
        items.push({ kind: "output", id: `output-${event.sequence}`, content: event.delta, sequence: event.sequence, order: assistantEventOrder(event) });
      }
    }
  }
  return items.filter(
    (item) => item.kind !== "thinking" || item.active || item.content.trim().length > 0,
  );
}

function runEventKey(event: RunEvent) {
  return `${event.run_id}:${event.sequence}`;
}

function mergeRunEvents(current: RunEvent[], incoming: RunEvent[]) {
  const merged = new Map(current.map((event) => [runEventKey(event), event]));
  incoming.forEach((event) => merged.set(runEventKey(event), event));
  return [...merged.values()].sort((left, right) => {
    const orderDelta = assistantEventOrder(left) - assistantEventOrder(right);
    return orderDelta || left.run_id - right.run_id || left.sequence - right.sequence;
  });
}

function assistantMessageKey(role: string, content: string) {
  return `${role}:${content.trim()}`;
}

function outlineTextSummary(content: string, fallback: string) {
  const line = content
    .split("\n")
    .map((value) => value.replace(/^#{1,6}\s*/, "").replace(/^[-*]\s*/, "").trim())
    .find((value) => value.length >= 18 && !/^【[^】]+】$/.test(value));
  if (!line) return fallback;
  return line.length > 150 ? `${line.slice(0, 150)}…` : line;
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
  const [assistantHistoryCutoff, setAssistantHistoryCutoff] = useState<number | null>(null);
  const [liveToolEvents, setLiveToolEvents] = useState<RunEvent[]>([]);
  const [assistantTimelineEvents, setAssistantTimelineEvents] = useState<RunEvent[]>([]);
  const [selectedSubagentRunId, setSelectedSubagentRunId] = useState<number | null>(null);
  const [thinkingRounds, setThinkingRounds] = useState<AssistantThinkingRound[]>([]);
  const [workflowStepsCollapsed, setWorkflowStepsCollapsed] = useState(true);
  const [assistantAdvancedOpen, setAssistantAdvancedOpen] = useState(false);
  const [orchestratorParentRunId, setOrchestratorParentRunId] = useState<number | null>(null);
  const [orchestratorCancellationRequested, setOrchestratorCancellationRequested] = useState(false);
  const [delegatedRunEvents, setDelegatedRunEvents] = useState<Record<number, RunEvent[]>>({});
  const [delegatedRunSummaries, setDelegatedRunSummaries] = useState<Record<number, AgentRunSummary>>({});
  const [versionDrawerOpen, setVersionDrawerOpen] = useState(false);
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
  const [searchQuery, setSearchQuery] = useState("");
  const [contextQuery, setContextQuery] = useState("");
  const [contextSnippets, setContextSnippets] = useState<StoryContextSnippet[]>([]);
  const [contextRerank, setContextRerank] = useState<StoryContextRerankResult | null>(null);
  const [contextPreview, setContextPreview] = useState<PreparedContext | null>(null);
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
  const [editingCanonEntryId, setEditingCanonEntryId] = useState<number | null>(null);
  const [showChapterPlanComposer, setShowChapterPlanComposer] = useState(false);
  const [editingChapterPlanId, setEditingChapterPlanId] = useState<number | null>(null);
  const [chapterPlanNo, setChapterPlanNo] = useState(1);
  const [chapterPlanTitle, setChapterPlanTitle] = useState("");
  const [chapterPlanContent, setChapterPlanContent] = useState("");
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
  const [, startViewTransition] = useTransition();
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
  const [assistantPanelWidth, setAssistantPanelWidth] = useState(() => {
    if (typeof window === "undefined") return ASSISTANT_PANEL_DEFAULT_WIDTH;
    const raw = window.localStorage.getItem(ASSISTANT_PANEL_WIDTH_STORAGE_KEY);
    const parsed = Number(raw);
    return Number.isFinite(parsed)
      ? clampAssistantPanelWidth(parsed)
      : ASSISTANT_PANEL_DEFAULT_WIDTH;
  });
  const [assistantPanelResizing, setAssistantPanelResizing] = useState(false);
  const activeProjectRequestRef = useRef<number | null>(null);
  const activeAgentRunIdRef = useRef<number | null>(null);
  const referenceFileInputRef = useRef<HTMLInputElement | null>(null);
  const assistantInputRef = useRef<HTMLTextAreaElement | null>(null);
  const assistantFeedRef = useRef<HTMLDivElement | null>(null);
  const assistantStickToBottomRef = useRef(true);
  const mainFeedScrollTopRef = useRef<number | null>(null);
  const assistantPanelRef = useRef<HTMLElement | null>(null);
  const autoFilledReviewArtifactRef = useRef<string | null>(null);

  function currentContentSurface(): ContentSurface {
    return mainSurface === "library" ? libraryMode : mainSurface;
  }

  function switchContentSurface(surface: ContentSurface) {
    // Leaving a knowledge-library view must actually enter the requested
    // content surface. Keeping mainSurface as "library" makes chapter clicks
    // update context without changing the visible editor.
    setLibraryOriginSurface(surface);
    setMainSurface(surface);
    setLibraryMode(surface);
  }

  function enterWorkbench() {
    // 资料页的“创作工作台”应进入对应的资料工具，而不是跳到章节编辑器。
    // 章节正文页才进入 draft/review/revision 流水线。
    if (mainSurface === "library") {
      if (libraryFocus === "foreshadowing") {
        setLibraryOriginSurface("workbench");
        setLibraryMode("workbench");
        setSelectedChapterId(null);
        setSelectedArtifactId(null);
        return;
      }
      if (libraryFocus === "setting" || libraryFocus === "outline" || libraryFocus === "characters") {
        setLibraryOriginSurface("workbench");
        setLibraryMode("workbench");
        setSelectedChapterId(null);
        setSelectedStage(librarySection);
        setSelectedArtifactId(null);
        if (libraryFocus !== "setting") openKnowledgeEditor();
        return;
      }
      const chapterBody = resolveChapterBody(detail, selectedChapter);
      setSelectedStage(artifactStageOr(chapterBody?.stage, "draft"));
      setSelectedArtifactId(chapterBody?.id ?? null);
      setLibraryMode("workbench");
      setMainSurface("workbench");
      return;
    }
    // 从“正式正文”进入时必须回到章节正文工作台，不能沿用资料页
    // 上一次的 setting/outline/characters 选择，否则会把旧的 Markdown
    // 基础资料误当成当前工作区内容。
    const chapterBody = resolveChapterBody(detail, selectedChapter);
    setSelectedStage(artifactStageOr(chapterBody?.stage, "draft"));
    setSelectedArtifactId(chapterBody?.id ?? null);
    setLibraryMode("workbench");
    setMainSurface("workbench");
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
    resetChapterPlanComposer();
    resetForeshadowingComposer();
    setMainSurface("library");
  }

  function resetChapterPlanComposer() {
    setShowChapterPlanComposer(false);
    setEditingChapterPlanId(null);
    setChapterPlanNo((detail?.chapter_plans?.length ?? 0) + 1);
    setChapterPlanTitle("");
    setChapterPlanContent("");
  }

  function editChapterPlan(plan: ChapterPlan) {
    setEditingChapterPlanId(plan.id);
    setChapterPlanNo(plan.chapter_no);
    setChapterPlanTitle(plan.title);
    setChapterPlanContent(plan.content);
    setShowChapterPlanComposer(true);
  }

  function openChapterPlanEditor(chapterNo?: number) {
    setEditingChapterPlanId(null);
    setChapterPlanNo(chapterNo ?? ((detail?.chapter_plans?.length ?? 0) + 1));
    setChapterPlanTitle(chapterNo ? `第 ${chapterNo} 章` : "");
    setChapterPlanContent("");
    setShowChapterPlanComposer(true);
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
        setStreamingRun(isTopLevelStreamingRun(run) ? run : null);
        activeAgentRunIdRef.current = run?.parent_run_id ?? run?.id ?? null;
        if (run?.run_kind === "orchestrator") {
          setOrchestratorParentRunId(run.id);
        } else if (run?.parent_run_id != null) {
          setOrchestratorParentRunId(run.parent_run_id);
        }
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
      if (!event.parent_run_id) {
        setAssistantTimelineEvents((current) => [...current, event]);
        if (event.stage === "orchestrator" && event.kind === "cancellation_requested") {
          setOrchestratorCancellationRequested(true);
        }
        if (event.stage === "orchestrator" && ["completed", "failed", "cancelled"].includes(event.kind)) {
          setOrchestratorCancellationRequested(false);
        }
      }
      if (event.parent_run_id && event.parent_run_id !== activeAgentRunIdRef.current) {
        return;
      }
      if (event.parent_run_id && event.parent_run_id === activeAgentRunIdRef.current) {
        setDelegatedRunEvents((current) => ({
          ...current,
          [event.run_id]: [...(current[event.run_id] ?? []), event],
        }));
        if (event.kind === "started") {
          setAssistantTimelineEvents((current) => current.some((item) => item.run_id === event.run_id)
            ? current
            : [...current, event]);
        }
        if (event.kind === "started") void refreshDetailBestEffort(event.project_id, "子任务状态");
        if (["completed", "failed", "cancelled"].includes(event.kind)) void hydrateFinishedAgentRun(event);
        return;
      }
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
        // A terminal event closes the current summary even when the worker fails
        // before it can emit a matching thinking_end event.
        setThinkingRounds((current) => current.map((round) => ({ ...round, active: false })));
        if (event.run_id !== orchestratorParentRunId) {
          setStreamingRun((current) => current?.id === event.run_id ? null : current);
        }
        void hydrateFinishedAgentRun(event);
        return;
      }
      if (event.run_id === orchestratorParentRunId) return;
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
            parent_run_id: event.parent_run_id,
            run_kind: event.parent_run_id == null ? "legacy" : "subagent",
            task_title: event.task_title,
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
    if (!detail || !selectedProjectId) return;
    let disposed = false;
    const parentRuns = detail.workflow_runs
      .filter((run) => run.run_kind === "orchestrator")
      .filter((run) => run.chapter_id == null || run.chapter_id === selectedChapterId)
      .slice(0, 8);
    const parentIds = new Set(parentRuns.map((run) => run.id));
    const childRuns = detail.workflow_runs.filter(
      (run) => run.parent_run_id != null && parentIds.has(run.parent_run_id),
    );
    const runIds = [...parentRuns, ...childRuns].map((run) => run.id)
    if (runIds.length === 0) return;

    void Promise.all(runIds.map((runId) => api.listRunEvents(runId)))
      .then((eventLists) => {
        if (disposed) return;
        mergeAssistantRunEvents(eventLists.flat());
      })
      .catch(() => {
        // Persisted messages remain available when historical event loading fails.
      });
    const activeParent = parentRuns.find((run) => ["running", "streaming", "cancellation_requested"].includes(run.status));
    if (activeParent) setOrchestratorParentRunId(activeParent.id);
    return () => {
      disposed = true;
    };
  }, [detail, selectedChapterId, selectedProjectId]);

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
    if (typeof window === "undefined") return;
    window.localStorage.setItem(ASSISTANT_PANEL_WIDTH_STORAGE_KEY, String(assistantPanelWidth));
  }, [assistantPanelWidth]);

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
    if (!hasApprovedFoundation) {
      // 资料阶段（世界观/大纲/角色）不属于任何章节：不拖带章节选择，
      // 否则世界观视图会出现"第 1 章"+ 章节流水线的不一致状态。
      setSelectedStage("setting");
      setSelectedArtifactId(null);
      return;
    }
    selectChapter(firstChapter, "draft");
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

  const libraryCards = useMemo(() => {
    if (!detail) return [];
    const categoryMatch = (card: CanonEntry) => {
      if (librarySection === "characters") return card.category === "character";
      if (librarySection === "outline") return card.category === "outline";
      return ["world", "cultivation", "map", "faction", "taboo", "item", "rule"].includes(card.category);
    };
    const modeMatch = (card: CanonEntry) =>
      libraryMode === "official" ? card.status === "approved" : card.status !== "archived";
    return (detail.canon_entries ?? []).filter((card) => categoryMatch(card) && modeMatch(card));
  }, [detail, librarySection, libraryMode]);

  const outlineCards = useMemo(() => {
    const unique = new Map<string, CanonEntry>();
    for (const card of libraryCards) {
      const key = card.title.replace(/[\s:：，。]+/g, "").toLowerCase();
      const current = unique.get(key);
      if (!current || (card.status === "approved" && current.status !== "approved") || card.id > current.id) {
        unique.set(key, card);
      }
    }
    return [...unique.values()].sort((left, right) => right.id - left.id);
  }, [libraryCards]);

  const outlinePlans = useMemo(() => {
    const plans = detail?.chapter_plans ?? [];
    return [...plans]
      .filter((plan) => libraryMode === "official" ? plan.status === "approved" : plan.status !== "archived")
      .sort((left, right) => left.chapter_no - right.chapter_no || left.id - right.id);
  }, [detail, libraryMode]);

  const visibleForeshadowings = useMemo(
    () => detail?.foreshadowings?.filter((item) =>
      libraryMode === "official"
        ? ["active", "ready_for_payoff", "resolved"].includes(item.status)
        : item.status !== "archived"
    ) ?? [],
    [detail, libraryMode]
  );

  const timelineEntityKind = libraryFocus === "character-timeline"
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
    if (libraryFocus !== "character-timeline" && libraryFocus !== "items") return;
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
    const bodyStage = asStage(body.stage);
    if (bodyStage && bodyStage !== selectedStage) setSelectedStage(bodyStage);
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
    return parseReviewIssues(selectedArtifact.content);
  }, [selectedArtifact]);

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

  const visibleAssistantTimelineEvents = useMemo(
    () => assistantTimelineEvents.filter(
      // Orchestrator output is internal delegation narration. The final answer
      // is persisted as a normal assistant message after the run completes.
      (event) =>
        event.stage !== "orchestrator" &&
        (event.chapter_id == null || event.chapter_id === selectedChapterId),
    ),
    [assistantTimelineEvents, selectedChapterId],
  );

  const orchestratorRunIsActive = useMemo(() => {
    if (orchestratorParentRunId == null) return false;
    return !assistantTimelineEvents.some(
      (event) => event.run_id === orchestratorParentRunId && ["completed", "failed", "cancelled"].includes(event.kind),
    );
  }, [assistantTimelineEvents, orchestratorParentRunId]);

  const assistantFeedItems = useMemo(() => {
    const timeline = buildAssistantTimeline(visibleAssistantTimelineEvents);
    const timelineOutputs = new Set(
      timeline
        .filter((item): item is Extract<AssistantTimelineItem, { kind: "output" }> => item.kind === "output")
        .map((item) => item.content.trim())
        .filter(Boolean),
    );
    const persistedMessages = visibleMessages
      .filter((message) => message.role === "user" || message.role === "assistant")
      .filter((message) => assistantHistoryCutoff == null || Date.parse(message.created_at) >= assistantHistoryCutoff - 2000);
    const persistedItems = persistedMessages
      .filter((message) => message.role !== "assistant" || !timelineOutputs.has(message.content.trim()))
      .map((message) => ({
        kind: message.role === "user" ? "user" as const : "output" as const,
        id: `message-${message.id}`,
        content: message.content,
        order: (Date.parse(message.created_at) || 0) * 1000 + message.id,
      }));
    const persistedKeys = new Set(persistedMessages.map((message) => assistantMessageKey(message.role, message.content)));
    const localItems = assistantMessages
      .filter((message) => !persistedKeys.has(assistantMessageKey(message.role, message.content)))
      .map((message) => ({
        kind: message.role === "user" ? "user" as const : "output" as const,
        id: message.id,
        content: message.content,
        order: message.order,
      }));
    // 交接记忆重建失败必须可见：否则下一章会在无前情记忆的情况下静默续写。
    const memoryWarningItems = detail
      ? detail.workflow_runs
          .filter((run) => run.stage === "chapter_memory" && run.status === "failed")
          .filter((run) => run.chapter_id == null || run.chapter_id === selectedChapterId)
          .filter((run) => assistantHistoryCutoff == null || Date.parse(run.created_at) >= assistantHistoryCutoff - 2000)
          .slice(-3)
          .map((run) => ({
            kind: "memory_warning" as const,
            id: `memory-run-${run.id}`,
            content: run.error?.trim() || "未知错误",
            order: (Date.parse(run.created_at) || 0) * 1000 + run.id,
          }))
      : [];
    return [...persistedItems, ...localItems, ...memoryWarningItems, ...timeline].sort((a, b) => a.order - b.order);
  }, [
    assistantHistoryCutoff,
    assistantMessages,
    detail,
    selectedChapterId,
    visibleAssistantTimelineEvents,
    visibleMessages,
  ]);

  const liveToolTimeline = useMemo(() => assistantToolTimeline(liveToolEvents), [liveToolEvents]);
  const activeLiveTool = liveToolTimeline.some((item) => item.status === "running");
  const completedLiveTool = liveToolTimeline.some((item) => item.status !== "running");
  const waitingForModelOutput = Boolean(streamingRun && !streamingRun.output.trim() && liveToolTimeline.length === 0);
  const pendingActionProposalCount = actionProposals.filter((proposal) => proposal.status === "pending").length;
  const selectedSubagentEvents = useMemo(
    () => (selectedSubagentRunId != null ? delegatedRunEvents[selectedSubagentRunId] ?? [] : []),
    [delegatedRunEvents, selectedSubagentRunId]
  );
  const selectedSubagentTimeline = useMemo(() => buildAssistantTimeline(selectedSubagentEvents), [selectedSubagentEvents]);
  const selectedSubagentSummary = selectedSubagentRunId != null ? delegatedRunSummaries[selectedSubagentRunId] : undefined;
  const selectedSubagentTerminal = selectedSubagentSummary
    ? ["success", "failed", "cancelled"].includes(selectedSubagentSummary.run.status)
    : false;
  const selectedSubagentToolCount = selectedSubagentEvents.filter((event) => event.kind === "tool_completed").length;

  // 过程 = 事件时间线 ∪ 运行摘要里的工具调用。历史运行的事件流可能缺失工具/思考
  // 事件，摘要中的 tool_invocations 是权威数据，缺失的按发生时间补进过程。
  const selectedSubagentProcess = useMemo(() => {
    const items: AssistantTimelineItem[] = [...selectedSubagentTimeline];
    const summary = selectedSubagentSummary;
    if (summary) {
      const represented = new Set(
        items
          .filter((item): item is Extract<AssistantTimelineItem, { kind: "tool" }> => item.kind === "tool" && item.invocationId != null)
          .map((item) => item.invocationId as number)
      );
      for (const invocation of summary.tool_invocations) {
        if (represented.has(invocation.id)) continue;
        items.push({
          kind: "tool",
          id: `invocation-${invocation.id}`,
          toolKey: invocation.tool_key,
          status: timelineToolStatus(invocation.status, Boolean(invocation.error)),
          invocationId: invocation.id,
          sequence: 0,
          order: (Number.isFinite(Date.parse(invocation.created_at)) ? Date.parse(invocation.created_at) : 0) * 1000,
        });
      }
      if (!items.some((item) => item.kind === "output") && summary.run.output.trim()) {
        items.push({
          kind: "output",
          id: "run-output",
          content: summary.run.output,
          sequence: 0,
          order: (Number.isFinite(Date.parse(summary.run.created_at)) ? Date.parse(summary.run.created_at) : 0) * 1000 + 1,
        });
      }
    }
    return items.sort((left, right) => left.order - right.order || left.sequence - right.sequence);
  }, [selectedSubagentTimeline, selectedSubagentSummary]);

  // 子会话的真实工具载荷在运行摘要里：选中时拉取；未结束的运行随工具完成而刷新。
  useEffect(() => {
    if (selectedSubagentRunId == null || selectedSubagentTerminal) return;
    let disposed = false;
    api.getAgentRun(selectedSubagentRunId)
      .then((summary) => {
        if (!disposed) setDelegatedRunSummaries((current) => ({ ...current, [selectedSubagentRunId]: summary }));
      })
      .catch(() => {
        // 事件时间线仍然可见，只是没有可展开的载荷。
      });
    return () => {
      disposed = true;
    };
  }, [selectedSubagentRunId, selectedSubagentTerminal, selectedSubagentToolCount]);

  const feedOutputIds = useMemo(
    () => assistantFeedItems.filter((item) => item.kind === "output").map((item) => item.id),
    [assistantFeedItems]
  );
  const lastOutputItemId = feedOutputIds[feedOutputIds.length - 1] ?? null;
  const liveOutputItemId = streamingRun ? lastOutputItemId : null;

  // 只在用户本来就读底部时跟随滚动；上翻阅读历史时不拽回。
  // 从子会话返回时不跳最新输出，恢复进入前主会话的阅读位置。
  useEffect(() => {
    const feed = assistantFeedRef.current;
    if (!feed) return;
    if (selectedSubagentRunId == null && mainFeedScrollTopRef.current != null) {
      feed.scrollTop = mainFeedScrollTopRef.current;
      mainFeedScrollTopRef.current = null;
      return;
    }
    if (!assistantStickToBottomRef.current) return;
    feed.scrollTo({ top: feed.scrollHeight, behavior: "smooth" });
  }, [assistantFeedItems, selectedSubagentRunId]);

  // 输入框随内容自增高，上限 180px 后内部滚动。
  useEffect(() => {
    const input = assistantInputRef.current;
    if (!input) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 180)}px`;
  }, [instruction]);

  const isChapterWorkbench = Boolean(
    selectedChapter && (
      bodyStages.includes(selectedStage) ||
      selectedStage === "review"
    ),
  );

  const chapterFlow = useMemo<ChapterFlow | null>(() => {
    if (!selectedChapter) return null;

    const reviewSelected = selectedArtifact?.stage === "review";
    const bodyArtifact = reviewSelected
      ? reviewSourceArtifact
      : selectedArtifact && bodyStages.includes(selectedArtifact.stage)
        ? selectedArtifact
        : currentChapterBodySummary;

    if (reviewSelected) {
      const reviewIssueCount = selectedArtifact ? parseReviewIssues(selectedArtifact.content).length : 0;
      return {
        state: reviewIssueCount > 0 ? "needs_revision" : "ready_to_adopt",
        label: reviewIssueCount > 0 ? `试读发现 ${reviewIssueCount} 个问题` : "试读完成，可采用",
        actionLabel: reviewIssueCount > 0 ? "修订建议已就绪" : adoptionActionLabel,
        bodyArtifact,
        reviewArtifact: selectedArtifact,
        reviewIssueCount,
      };
    }

    if (!bodyArtifact) {
      return {
        state: "empty",
        label: "尚无候选稿",
        actionLabel: "生成正文",
        bodyArtifact: null,
        reviewArtifact: null,
        reviewIssueCount: 0,
      };
    }

    const adopted = (
      bodyArtifact.status === "approved" ||
      detail?.approvals.some((approval) => approval.artifact_id === bodyArtifact.id)
    ) && selectedChapter.current_artifact_id === bodyArtifact.id;

    return {
      state: adopted ? "adopted" : "awaiting_review",
      label: adopted ? adoptedStatusLabel : "待试读",
      actionLabel: adopted ? "生成新版本" : "提交试读",
      bodyArtifact,
      reviewArtifact: null,
      reviewIssueCount: 0,
    };
  }, [
    currentChapterBodySummary,
    detail,
    reviewSourceArtifact,
    selectedArtifact,
    selectedChapter,
  ]);

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
    // 重复点击同一本书时不清空会话时间线：清空后没有重新加载路径，
    // 历史事件会永久丢失（仅项目真正切换时才重置会话数据）。
    const projectChanged = projectId !== selectedProjectId;
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
    autoFilledReviewArtifactRef.current = null;
    setInstruction("");
    setAssistantMessages([]);
    setAssistantHistoryCutoff(null);
    setLiveToolEvents([]);
    if (projectChanged) {
      setAssistantTimelineEvents([]);
      setDelegatedRunEvents({});
    }
    setSelectedSubagentRunId(null);
    setAssistantAdvancedOpen(false);
    setOrchestratorParentRunId(null);
    setOrchestratorCancellationRequested(false);
    setThinkingRounds([]);
    setChapterDraft("");
    setContextQuery("");
    setContextSnippets([]);
    setContextRerank(null);
    setContextPreview(null);
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

  function assistantPanelWidthFromPointer(clientX: number) {
    const contentGrid = assistantPanelRef.current?.parentElement;
    const contentRight = contentGrid?.getBoundingClientRect().right ?? window.innerWidth;
    return clampAssistantPanelWidth(contentRight - clientX - 5);
  }

  function finishAssistantPanelResize() {
    setAssistantPanelResizing(false);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }

  function beginAssistantPanelResize(event: PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    setAssistantPanelWidth(assistantPanelWidthFromPointer(event.clientX));
    setAssistantPanelResizing(true);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }

  function resizeAssistantPanel(event: PointerEvent<HTMLDivElement>) {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
    setAssistantPanelWidth(assistantPanelWidthFromPointer(event.clientX));
  }

  function endAssistantPanelResize(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    finishAssistantPanelResize();
  }

  function resizeAssistantPanelWithKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Home") {
      event.preventDefault();
      setAssistantPanelWidth(ASSISTANT_PANEL_MIN_WIDTH);
      return;
    }
    if (event.key === "End") {
      event.preventDefault();
      setAssistantPanelWidth(ASSISTANT_PANEL_MAX_WIDTH);
      return;
    }
    const delta = event.key === "ArrowLeft" ? 16 : event.key === "ArrowRight" ? -16 : 0;
    if (!delta) return;
    event.preventDefault();
    setAssistantPanelWidth((width) => clampAssistantPanelWidth(width + delta));
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
    if (activeProjectRequestRef.current === projectId) {
      // An orchestrator run is rendered in the assistant timeline only; the
      // central surface is reserved for specialist artifact generation.
      setStreamingRun(isTopLevelStreamingRun(activeRun) ? activeRun : null);
      activeAgentRunIdRef.current = activeRun?.parent_run_id ?? activeRun?.id ?? null;
      if (activeRun?.run_kind === "orchestrator") {
        setOrchestratorParentRunId(activeRun.id);
      } else if (activeRun?.parent_run_id != null) {
        setOrchestratorParentRunId(activeRun.parent_run_id);
      }
    }
  }

  function mergeAssistantRunEvents(events: RunEvent[]) {
    const timelineEvents = events.filter((event) => !event.parent_run_id || event.kind === "started");
    if (timelineEvents.length > 0) {
      setAssistantTimelineEvents((current) => mergeRunEvents(current, timelineEvents));
    }
    const childEvents = events.filter((event) => event.parent_run_id != null);
    if (childEvents.length > 0) {
      // 先按 run 分组再合并：逐条 mergeRunEvents 是 O(n²)，万级 token 流会卡死主线程。
      const grouped = new Map<number, RunEvent[]>();
      for (const event of childEvents) {
        const list = grouped.get(event.run_id);
        if (list) list.push(event);
        else grouped.set(event.run_id, [event]);
      }
      setDelegatedRunEvents((current) => {
        const next = { ...current };
        for (const [runId, list] of grouped) {
          next[runId] = mergeRunEvents(next[runId] ?? [], list);
        }
        return next;
      });
    }
  }

  async function syncOrchestratorRun(runId: number) {
    const [summary, events] = await Promise.all([
      api.getAgentRun(runId),
      api.listRunEvents(runId),
    ]);
    if (activeProjectRequestRef.current !== summary.run.project_id) return true;
    mergeAssistantRunEvents(events);
    const terminal = ["success", "failed", "cancelled"].includes(summary.run.status);
    if (!terminal) return false;

    if (summary.run.status === "failed") {
      const message = summary.run.error || "主 Agent 运行失败，请检查配置后重试。";
      setError(message);
      appendAssistantMessage(`run-${runId}-failed`, `这次主 Agent 任务没有完成：${message}`);
    } else if (summary.run.status === "cancelled") {
      appendAssistantMessage(`run-${runId}-cancelled`, "已停止主 Agent 任务，当前内容没有自动应用。");
    } else if (summary.run.output.trim() && !events.some((event) => event.kind === "output_delta" && event.delta.trim())) {
      const started = events.find((event) => event.kind === "started") ?? events[0];
      if (started) {
        setAssistantTimelineEvents((current) => mergeRunEvents(current, [{
          ...started,
          kind: "output_delta",
          delta: summary.run.output,
          status: "success",
          sequence: Math.max(...events.map((event) => event.sequence), 0) + 1,
          tool_key: null,
          tool_invocation_id: null,
          elapsed_ms: summary.run.elapsed_ms,
        }]));
      }
    }
    await refreshDetailBestEffort(summary.run.project_id, "主 Agent 回复");
    return true;
  }

  function watchOrchestratorRun(runId: number) {
    void (async () => {
      for (let attempt = 0; attempt < 2400; attempt += 1) {
        try {
          if (await syncOrchestratorRun(runId)) return;
        } catch {
          // The event subscription remains the fast path; polling retries transient failures.
        }
        await new Promise((resolve) => window.setTimeout(resolve, 500));
      }
    })();
  }

  function appendAssistantMessage(id: string, content: string) {
    setAssistantMessages((current) => {
      if (current.some((message) => message.id === id)) return current;
      return [...current, { id, role: "assistant", content, order: Date.now() * 1000 }];
    });
  }

  async function hydrateFinishedAgentRun(event: RunEvent) {
    if (activeProjectRequestRef.current !== event.project_id) return;
    try {
      const summary = await api.getAgentRun(event.run_id);
      if (activeProjectRequestRef.current !== event.project_id) return;
      setLastAgentRun(summary);
      setWorkflowStepsCollapsed(true);
      setLiveToolEvents((current) => current.length > 0 ? current : toolInvocationEvents(summary.tool_invocations));
      mergeActionProposals(summary.proposals);

      if (event.kind === "failed") {
        const message = event.error ?? summary.run.error ?? "Agent 运行失败，请检查配置后重试。";
        setError(message);
        appendAssistantMessage(`run-${event.run_id}-failed`, `这次${stageLabel(summary.run.stage)}任务没有完成：${message}`);
      } else if (event.kind === "cancelled") {
        setNotice("Agent 运行已取消");
        appendAssistantMessage(`run-${event.run_id}-cancelled`, `已停止${stageLabel(summary.run.stage)}任务，当前内容没有自动应用。`);
      }

      // A completed orchestrator run without delegated work is an ordinary
      // conversation answer, not an artifact-producing task.
      if (event.kind === "completed" && summary.run.run_kind === "orchestrator") {
        // The event timeline is the canonical live rendering. Only add a fallback
        // message when the output event was missed by the live transport.
        const hasTimelineOutput = assistantTimelineEvents.some(
          (item) => item.run_id === event.run_id && item.kind === "output_delta" && item.delta.trim(),
        );
        if (!hasTimelineOutput) {
          const fallback = summary.run.output || "我已理解你的问题；目前不需要启动执行任务。";
          setAssistantTimelineEvents((current) => current.some(
            (item) => item.run_id === event.run_id && item.kind === "output_delta",
          ) ? current : [...current, {
            ...event,
            kind: "output_delta",
            delta: fallback,
            status: "success",
            parent_run_id: null,
            tool_key: null,
            tool_invocation_id: null,
            elapsed_ms: null,
          }]);
        }
        return;
      }

      const isFoundationRun = ["setting", "outline", "characters"].includes(summary.run.stage);
      // Foundation runs now persist canon_entries directly. If an older backend
      // returns a Markdown artifact, never surface it as the current result: that
      // would make the UI look as if the card-first flow had silently regressed.
      if (summary.artifact && isFoundationRun) {
        await refreshDetailBestEffort(event.project_id, "结构化资料卡刷新");
        if (event.kind === "completed") {
          const message = "检测到旧版 Markdown 候选，已隐藏；请用当前版本重新运行，结果会直接写入资料卡。";
          setNotice(message);
          appendAssistantMessage(`run-${event.run_id}-legacy-foundation`, message);
        }
      } else if (summary.artifact) {
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
      } else if (event.kind === "completed" && isFoundationRun) {
        await refreshDetailBestEffort(event.project_id, "结构化资料卡刷新");
        const message = summary.run.output || "故事架构 Agent 已直接更新结构化资料卡。";
        setNotice(message);
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
    setSelectedStage(stage ?? artifactStageOr(currentBody?.stage, "draft"));
    setSelectedArtifactId(stage ? null : currentBody?.id ?? null);
  }

  function openTimelineChapter(chapterId?: number | null) {
    const chapter = detail?.chapters.find((item) => item.id === chapterId);
    if (!chapter) return;
    const body = resolveChapterBody(detail, chapter);
    setSelectedChapterId(chapter.id);
    setSelectedStage(artifactStageOr(body?.stage, "draft"));
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
    setEditingCanonEntryId(null);
    setShowKnowledgeComposer(false);
  }

  function editCanonEntry(card: CanonEntry) {
    setKnowledgeTitle(card.title);
    setKnowledgeContent(card.content);
    setKnowledgeCategory(card.category);
    setEditingCanonEntryId(card.id);
    setShowKnowledgeComposer(true);
  }

  function openKnowledgeEditor() {
    const firstCard = libraryCards[0];
    if (firstCard) {
      editCanonEntry(firstCard);
      return;
    }
    resetKnowledgeComposer();
    setShowKnowledgeComposer(true);
  }

  async function saveChapterPlan(status: "pending_human_approval" | "approved") {
    if (!detail || !chapterPlanTitle.trim() || !chapterPlanContent.trim() || chapterPlanNo < 1) return;
    const existing = detail.chapter_plans?.find((plan) => plan.id === editingChapterPlanId);
    await runTask(
      status === "approved"
        ? adoptionActionLabel
        : editingChapterPlanId ? "更新章节计划" : "保存章节计划",
      async () => {
        await api.saveChapterPlan({
          id: editingChapterPlanId,
          project_id: detail.project.id,
          chapter_no: chapterPlanNo,
          title: chapterPlanTitle.trim(),
          content: chapterPlanContent.trim(),
          status,
          story_arc_id: existing?.story_arc_id ?? activeStoryArc?.id ?? null,
          chapter_id: existing?.chapter_id ?? null,
          source_artifact_id: existing?.source_artifact_id ?? null,
        });
        resetChapterPlanComposer();
        await refreshDetailBestEffort(detail.project.id, "章节计划保存");
        setNotice(status === "approved" ? `章节计划已${adoptionActionLabel}` : "章节计划已保存，等待人工确认");
      },
    );
  }

  async function updateChapterPlanStatus(plan: ChapterPlan, status: "approved" | "archived") {
    if (!detail) return;
    await runTask(status === "approved" ? adoptionActionLabel : "归档章节计划", async () => {
      await api.saveChapterPlan({ ...plan, status });
      await refreshDetailBestEffort(detail.project.id, "章节计划更新");
      setNotice(status === "approved" ? `第 ${plan.chapter_no} 章计划已${adoptionActionLabel}` : `第 ${plan.chapter_no} 章计划已归档`);
    });
  }

  async function deleteChapterPlan(plan: ChapterPlan) {
    if (!detail) return;
    if (!window.confirm(`确定删除第 ${plan.chapter_no} 章计划“${plan.title}”吗？`)) return;
    await runTask("删除章节计划", async () => {
      await api.deleteChapterPlan(detail.project.id, plan.id);
      if (editingChapterPlanId === plan.id) resetChapterPlanComposer();
      await refreshDetailBestEffort(detail.project.id, "章节计划删除");
      setNotice(`已删除第 ${plan.chapter_no} 章计划`);
    });
  }

  async function createChapterFromPlan(plan: ChapterPlan) {
    if (!detail) return;
    await runTask("创建正文", async () => {
      const chapter = await api.createChapterFromPlan(detail.project.id, plan.id);
      await refreshDetailBestEffort(detail.project.id, "正文创建");
      selectChapter(chapter, "draft");
      switchContentSurface("workbench");
      setNotice(`已进入第 ${chapter.chapter_no} 章正文`);
    });
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

  async function saveCanonEntry(status: "pending_human_approval" | "approved") {
    if (!detail || !knowledgeTitle.trim() || !knowledgeContent.trim()) return;
    await runTask(status === "approved" ? adoptionActionLabel : "保存资料卡", async () => {
      const input: SaveCanonEntryInput = {
        id: editingCanonEntryId,
        project_id: detail.project.id,
        category: librarySection === "characters" ? "character" : librarySection === "outline" ? "outline" : knowledgeCategory,
        title: knowledgeTitle.trim(),
        content: knowledgeContent.trim(),
        status,
        source_artifact_id: null,
        source_chapter_id: null,
      };
      await api.saveCanonEntry(input);
      resetKnowledgeComposer();
      await refreshDetailBestEffort(detail.project.id, "资料卡保存");
      setNotice(status === "approved" ? `资料卡已${adoptionActionLabel}并加入写作依据` : "资料卡已保存，等待人工确认");
    });
  }

  async function saveForeshadowing(status: "pending_human_approval" | "active") {
    if (!detail || !foreshadowingTitle.trim() || !foreshadowingContent.trim()) return;
    await runTask(status === "active" ? adoptionActionLabel : "保存伏笔", async () => {
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
      setNotice(status === "active" ? `伏笔已${adoptionActionLabel}并加入追踪` : "伏笔已保存，等待人工确认");
    });
  }

  async function updateCanonEntryStatus(card: CanonEntry, status: "approved" | "archived") {
    if (!detail) return;
    await runTask(status === "approved" ? adoptionActionLabel : "归档资料卡", async () => {
      await api.saveCanonEntry({ ...card, status });
      await refreshDetailBestEffort(detail.project.id, "资料卡更新");
      setNotice(status === "approved" ? `资料卡已${adoptionActionLabel}并加入写作依据` : "资料卡已归档，不再作为写作依据");
    });
  }

  async function deleteCanonEntry(card: CanonEntry) {
    if (!detail) return;
    const confirmed = window.confirm(
      `确定删除资料卡“${card.title}”吗？\n该资料卡会从项目资料中彻底移除，且不可恢复。`
    );
    if (!confirmed) return;
    await runTask("删除资料卡", async () => {
      await api.deleteCanonEntry({ project_id: detail.project.id, card_id: card.id });
      if (editingCanonEntryId === card.id) resetKnowledgeComposer();
      await refreshDetailBestEffort(detail.project.id, "资料卡删除");
      setNotice(`已删除资料卡 ${card.title}`);
    });
  }

  async function updateForeshadowingStatus(
    item: Foreshadowing,
    status: "active" | "ready_for_payoff" | "resolved" | "archived"
  ) {
    if (!detail) return;
    await runTask(status === "active" ? adoptionActionLabel : "更新伏笔", async () => {
      await api.saveForeshadowing({ ...item, status });
      await refreshDetailBestEffort(detail.project.id, "伏笔更新");
      setNotice(
        status === "active"
          ? `伏笔已${adoptionActionLabel}并加入追踪`
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
      message = "请先确认采用创作基准。";
    } else if (!activeStoryArc) {
      message = "请先确认采用故事阶段。";
    } else if (!detail.story_bible_review) {
      message = "请先完成一致性审校。";
    } else if (detail.story_bible_review.canon_fingerprint !== detail.canonical_fingerprint) {
      message = "创作基准已变化，请重新审校。";
    } else if (detail.story_bible_review.issues.some((issue) => issue.severity === "major")) {
      message = "一致性审校有未解决问题。";
    } else if (detail.story_bible_review.status !== "confirmed") {
      message = "请确认采用最新审校结论。";
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

  async function submitAssistantMessage(
    message: string,
    options: AssistantMessageOptions = {},
  ): Promise<boolean> {
    const prompt = message.trim();
    if (!prompt) return false;
    if (!detail) {
      setNotice("请先打开一本书");
      return false;
    }
    if (busy) {
      setNotice("当前操作正在执行");
      return false;
    }
    if (orchestratorRunIsActive) {
      setNotice("主 Agent 仍在处理上一条消息");
      return false;
    }
    if (streamingRun) {
      setNotice("Agent 运行中");
      return false;
    }

    assistantStickToBottomRef.current = true;
    const createdAt = Date.now();
    setAssistantMessages((current) => [
      ...current,
      { id: `user-${createdAt}`, role: "user", content: prompt, order: createdAt * 1000 },
    ]);

    const result = await runTask("主 Agent", async () => {
      const response = await api.startOrchestratorTurn({
        project_id: detail.project.id,
        chapter_id: selectedChapterId,
        message: prompt,
        stage: options.stage ?? selectedStage,
        source_artifact_id: options.sourceArtifactId ?? null,
        story_architect_mode: options.storyArchitectMode ?? null,
        reference_selection: options.referenceSelection ?? activeReferenceSelection,
      });
      if (response.parent_run) {
        activeAgentRunIdRef.current = response.parent_run.id;
        setOrchestratorParentRunId(response.parent_run.id);
        setDelegatedRunEvents({});
        setLiveToolEvents([]);
        setThinkingRounds([]);
        setWorkflowStepsCollapsed(false);
        // The orchestrator is a conversation run. It must never occupy the
        // central artifact/output surface reserved for specialist child runs.
        setStreamingRun(null);
        setLastAgentRun(null);
        watchOrchestratorRun(response.parent_run.id);
      }
      setInstruction("");
      return response;
    });
    return result != null;
  }

  function submitAssistantPrompt() {
    const revisionOptions = chapterFlow?.state === "needs_revision" && chapterFlow.reviewArtifact
      ? { sourceArtifactId: chapterFlow.reviewArtifact.id, stage: "revision" as const }
      : {};
    void submitAssistantMessage(instruction, revisionOptions);
  }

  function openCandidatePanel() {
    document.querySelector(".chapter-candidate-panel")?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  function useAssistantPrompt(prompt: string) {
    setInstruction(prompt);
  }

  async function runAgent(
    stage: Stage = selectedStage,
    sourceArtifactIdOverride?: number | null,
  ) {
    if (!detail) return;
    if (stage === "setting" || stage === "outline" || stage === "characters") {
      return runStoryArchitect(architectModeByStage[stage]);
    }
    switchContentSurface("workbench");
    const chapterTitle = selectedChapter
      ? `第 ${selectedChapter.chapter_no} 章《${selectedChapter.title}》`
      : "当前章节";
    const prompt = buildChapterAgentPrompt(detail.project.title, chapterTitle, stage, instruction);
    const sourceArtifactId = sourceArtifactIdOverride !== undefined
      ? sourceArtifactIdOverride
      : stage === "review"
        ? agentSourceArtifactId(stage)
        : stage === "revision" && selectedArtifact && selectedArtifact.chapter_id === selectedChapterId
          && ["draft", "revision", "review"].includes(selectedArtifact.stage)
          ? selectedArtifact.id
          : null;
    return submitAssistantMessage(prompt, { sourceArtifactId, stage });
  }

  function appendManualInstruction(prompt: string, extraInstruction = instruction) {
    const hint = extraInstruction.trim();
    return hint ? `${prompt}\n\n补充要求：${hint}` : prompt;
  }

  function focusPlanDiscussion() {
    setInstruction(
      "请和我一起讨论并完善当前创作计划。请先理解我的意图，再决定需要初始化、补充世界观、细化当前阶段、扩展下一阶段、整理角色或修复一致性问题；不要预设固定故事架构模式。",
    );
    assistantInputRef.current?.focus();
  }

  async function confirmCurrentPlan() {
    if (!detail) return;
    const result = await runTask(adoptionActionLabel, async () => {
      const confirmation = await api.confirmCurrentPlan({
        project_id: detail.project.id,
        note: "",
      });
      await refreshDetailBestEffort(detail.project.id, "当前计划确认采用");
      return confirmation;
    });
    if (!result) return;

    if (result.status === "confirmed") {
      setNotice(
        result.approved_card_count > 0
          ? `当前计划已${adoptionActionLabel}，并纳入 ${result.approved_card_count} 张资料卡`
          : `当前计划已${adoptionActionLabel}`,
      );
      return;
    }
    if (result.status === "awaiting_review_confirmation") {
      setNotice(
        result.approved_card_count > 0
          ? `已纳入 ${result.approved_card_count} 张资料卡，审校结论已生成，请查看后再次${adoptionActionLabel}`
          : `审校结论已生成，请查看后再次${adoptionActionLabel}`,
      );
      return;
    }

    const majorIssues = result.review?.issues.filter((issue) => issue.severity === "major") ?? [];
    if (majorIssues.length > 0) {
      const repairPrompt = [
        "请和我一起修复当前创作计划中的一致性问题。请先理解问题，再由主 Agent 自行判断需要委托的故事架构内部模式；不要预设固定模式。",
        ...majorIssues.map((issue, index) => `${index + 1}. 问题：${issue.title}\n影响：${issue.impact}\n修复要求：${issue.rework_instruction}\n冲突：${issue.conflict}`),
      ].join("\n\n");
      setInstruction(repairPrompt);
      assistantInputRef.current?.focus();
    }
    setError(
      result.blockers.length > 0
        ? `当前计划暂不能${adoptionActionLabel}：${result.blockers.join("；")}`
        : `当前计划暂不能${adoptionActionLabel}，请先处理审校中的 major 问题`,
    );
  }

  function runStoryArchitect(architectMode: StoryArchitectMode) {
    if (!detail) return;
    const stage = artifactStageForArchitectMode(architectMode);
    setLibraryMode("workbench");
    setLibraryOriginSurface("workbench");
    setLibrarySection(stage);
    setLibraryFocus(stage);
    setSelectedStage(stage);
    setSelectedArtifactId(null);
    setExplicitArchitectSourceId(null);
    setMainSurface("library");
    const prompt = appendManualInstruction(
      architectMode === "initialize"
        ? `请为《${detail.project.title}》开始建立创作基准，生成这本书可持续创作所需的世界观。请把稳定的世界规则、势力、地点、物件和边界沉淀为待确认的结构化资料卡，不要直接写正文。`
        : architectMode === "refine_canon"
          ? `请为《${detail.project.title}》补充并整理当前世界观，只增加对后续创作有用且能长期成立的规则、势力、地点、物件或边界，并沉淀为待确认的结构化资料卡。`
          : architectMode === "plan_current_arc"
            ? `请为《${detail.project.title}》细化当前故事阶段，形成可执行的阶段目标、冲突、角色变化和近期章节任务，并沉淀为待确认的大纲资料。`
            : architectMode === "extend_next_arc"
              ? `请为《${detail.project.title}》基于当前故事进展提出下一阶段的候选方向，保留未兑现项并说明新条件，沉淀为待确认的大纲资料。`
              : `请为《${detail.project.title}》补充和整理主要角色信息，明确身份、目标、限制、已知信息、关系和长期变化条件。`,
    );
    void submitAssistantMessage(prompt, { storyArchitectMode: architectMode });
  }

  async function confirmStoryBible() {
    if (!detail) return;
    await runTask(adoptionActionLabel, async () => {
      await api.confirmStoryBible({ project_id: detail.project.id, note: "" });
      await refreshDetailBestEffort(detail.project.id, "创作基准确认");
      setNotice(`创作基准与当前故事阶段已${adoptionActionLabel}`);
    });
  }

  async function reviewStoryBible() {
    if (!detail) return;
    await runTask("审校创作基准", async () => {
      await api.reviewStoryBible({ project_id: detail.project.id });
      await refreshDetailBestEffort(detail.project.id, "创作基准审校");
      setNotice("创作基准一致性审校已生成，等待确认采用");
    });
  }

  async function confirmStoryBibleReview() {
    if (!detail?.story_bible_review) return;
    const reviewId = detail.story_bible_review.id;
    await runTask(adoptionActionLabel, async () => {
      await api.confirmStoryBibleReview({
        project_id: detail.project.id,
        review_id: reviewId,
        note: "",
      });
      await refreshDetailBestEffort(detail.project.id, "一致性审校确认采用");
      setNotice(`一致性审校已${adoptionActionLabel}`);
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
    if (!window.confirm(`${adoptionActionLabel}这条 Agent 提案？\n\n${proposal.summary}`)) return;
    await runTask(adoptionActionLabel, async () => {
      await api.applyActionProposal({
        project_id: detail.project.id,
        proposal_id: proposal.id,
        note: `由用户${adoptionActionLabel}`,
      });
      await Promise.all([
        refreshDetailBestEffort(detail.project.id, "Agent 提案确认采用"),
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
      setNotice(`Agent 提案已${adoptionActionLabel}`);
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

  async function approveBodyArtifact(
    artifact: Pick<Artifact, "id" | "project_id" | "chapter_id" | "stage" | "title" | "version" | "status" | "parent_artifact_id" | "created_at">,
  ) {
    if (!detail || !selectedChapter) return;
    const artifactApproved =
      artifact.status === "approved" ||
      detail.approvals.some((approval) => approval.artifact_id === artifact.id);
    if (artifactApproved && selectedChapter.current_artifact_id === artifact.id) return;
    await runTask(adoptionActionLabel, async () => {
      await api.approveStage(detail.project.id, artifact.stage, artifact.id, "");
      await refreshDetailBestEffort(detail.project.id, "正文确认采用");
      await refreshStorySearchStatusBestEffort(detail.project.id, "正文确认采用");
      setSelectedChapterId(selectedChapter.id);
      setSelectedStage(artifactStageOr(artifact.stage, "draft"));
      setSelectedArtifactId(artifact.id);
      setNotice(`已${adoptionActionLabel}${artifact.stage === "revision" ? "修订稿" : "草稿"} v${artifact.version} 作为当前正文`);
    });
  }

  async function runChapterPrimaryAction() {
    if (!chapterFlow) return;

    switch (chapterFlow.state) {
      case "empty":
        await runAgent("draft");
        return;
      case "awaiting_review":
        if (!chapterFlow.bodyArtifact) {
          setNotice("当前没有可试读的候选稿");
          return;
        }
        setSelectedArtifactId(chapterFlow.bodyArtifact.id);
        await runAgent("review", chapterFlow.bodyArtifact.id);
        return;
      case "ready_to_adopt":
        if (!chapterFlow.bodyArtifact) {
          setNotice("试读原稿仍在加载，请稍后再试");
          return;
        }
        await approveBodyArtifact(chapterFlow.bodyArtifact);
        return;
      case "adopted":
        await runAgent("draft");
        return;
    }
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

  async function cancelOrchestratorRun() {
    if (orchestratorParentRunId == null || orchestratorCancellationRequested) return;
    await runTask("停止 Agent", async () => {
      await api.cancelAgentRun(orchestratorParentRunId);
      setOrchestratorCancellationRequested(true);
      setNotice("正在停止 Agent");
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
    setSelectedStage(artifactStageOr(artifact.stage, "draft"));
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
    setInstruction(chapterSplitPlan.revision_prompt_current);
    assistantInputRef.current?.focus();
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
      try {
        await navigator.clipboard?.writeText(markdown);
      } catch {
        // Clipboard permissions are optional; the generated text remains available below.
      }
      setNotice("Markdown 已生成，并尝试复制到剪贴板");
    });
  }

  function stageLabel(stage: string) {
    if (stage === "context_search_plan") return "上下文检索";
    return stages.find((item) => item.id === stage)?.label ?? stage;
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
    if (!selectedArtifact || selectedArtifact.stage !== "review" || selectedReviewIssues.length === 0) return;
    const reviewKey = `${selectedProjectId ?? "project"}:${selectedArtifact.id}`;
    if (autoFilledReviewArtifactRef.current === reviewKey) return;

    autoFilledReviewArtifactRef.current = reviewKey;
    setInstruction(formatReviewInstructions(selectedReviewIssues));
    assistantInputRef.current?.focus();
  }, [selectedArtifact, selectedProjectId, selectedReviewIssues]);

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

  function transitionToView(nextView: ViewMode) {
    startViewTransition(() => setViewMode(nextView));
  }

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
        onBack={() => transitionToView("main")}
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
                <button className="settings-btn" onClick={() => transitionToView("settings")}>
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
                  <h2>资料库</h2>
                  {currentContentSurface() === "official" && <span className="read-only-note">只读</span>}
                </div>
                {currentContentSurface() === "workbench" ? (
                  <>
                    <div className="library-nav-group">
                      <span className="library-nav-group-label">创作计划</span>
                      {foundationStages.map((stage) => (
                        <button
                          key={stage.id}
                          className={mainSurface === "library" && libraryFocus === stage.id ? "active" : ""}
                          onClick={() => openLibrary(stage.id as LibrarySection, "workbench")}
                        >
                          <span>{stage.label}</span>
                        </button>
                      ))}
                    </div>
                    <div className="library-nav-group">
                      <span className="library-nav-group-label">主动维护</span>
                      <button
                        className={mainSurface === "library" && libraryFocus === "foreshadowing" ? "active" : ""}
                        onClick={() => openLibrary("foreshadowing", "workbench")}
                      >
                        <span>伏笔账本</span>
                        <small>{visibleForeshadowings.length}</small>
                      </button>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="library-nav-group">
                      <span className="library-nav-group-label">创作计划</span>
                      <button className={mainSurface === "library" && libraryFocus === "setting" ? "active" : ""} onClick={() => openLibrary("setting", "official")}>
                        <span>世界观</span>
                      </button>
                      <button className={mainSurface === "library" && libraryFocus === "outline" ? "active" : ""} onClick={() => openLibrary("outline", "official")}>
                        <span>大纲</span>
                      </button>
                      <button className={mainSurface === "library" && libraryFocus === "characters" ? "active" : ""} onClick={() => openLibrary("characters", "official")}>
                        <span>角色</span>
                      </button>
                    </div>
                    <div className="library-nav-group">
                      <span className="library-nav-group-label">正文衍生资料</span>
                      <button className={mainSurface === "library" && libraryFocus === "events" ? "active" : ""} onClick={() => openLibrary("events", "official")}>
                        <span>事件时间线</span>
                        <small>{detail?.story_events?.length ?? 0}</small>
                      </button>
                      <button className={mainSurface === "library" && libraryFocus === "character-timeline" ? "active" : ""} onClick={() => openLibrary("character-timeline", "official")}>
                        <span>角色时间线</span>
                        <small>{detail?.story_entities?.filter((entity) => entity.kind === "character").length ?? 0}</small>
                      </button>
                      <button className={mainSurface === "library" && libraryFocus === "items" ? "active" : ""} onClick={() => openLibrary("items", "official")}>
                        <span>物品状态</span>
                        <small>{detail?.story_entities?.filter((entity) => entity.kind === "item" || entity.kind === "resource").length ?? 0}</small>
                      </button>
                    </div>
                    <div className="library-nav-group">
                      <span className="library-nav-group-label">主动维护</span>
                      <button className={mainSurface === "library" && libraryFocus === "foreshadowing" ? "active" : ""} onClick={() => openLibrary("foreshadowing", "official")}>
                        <span>伏笔账本</span>
                        <small>{visibleForeshadowings.length}</small>
                      </button>
                    </div>
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
                    {currentContentSurface() === "workbench" && <small>{chapter.current_artifact_id ? adoptedStatusLabel : "待创作"}</small>}
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
                  <span className="workspace-mode-badge official">{adoptedStatusLabel} · 只读</span>
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
          ) : mainSurface === "library" && libraryMode === "official" && ["character-timeline", "items", "events"].includes(libraryFocus) ? (
            <ContinuityLibraryPanel
              focus={libraryFocus as "character-timeline" | "items" | "events"}
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
                openLibrary(kind === "character" ? "character-timeline" : "items");
                setSelectedLibraryEntityId(entityId);
              }}
              onOpenChapter={openTimelineChapter}
            />
          ) : mainSurface === "library" ? (
            <section className="library-workspace">
              <header className="library-header">
                <div className="library-header-title">
                  <div>
                    <span className="library-context-label">{libraryFocusMeta[libraryFocus].group}</span>
                    <h2>{libraryFocusMeta[libraryFocus].title}</h2>
                    <p>{libraryFocusMeta[libraryFocus].description}</p>
                  </div>
                </div>
                {libraryMode === "workbench" && libraryFocus !== "setting" && (
                  <>
                    {libraryFocus === "foreshadowing" && (
                      <button onClick={() => (showForeshadowingComposer ? resetForeshadowingComposer() : setShowForeshadowingComposer(true))} disabled={!detail || Boolean(busy)}>
                        <Plus size={14} /> 登记伏笔
                      </button>
                    )}
                    {libraryFocus === "characters" && (
                      <button onClick={() => runStoryArchitect("design_characters")} disabled={!detail || Boolean(busy)}>
                        <Sparkles size={14} /> 补充角色
                      </button>
                    )}
                    {libraryFocus === "outline" ? (
                      <>
                        <button onClick={() => runStoryArchitect("plan_current_arc")} disabled={!detail || Boolean(busy)}>
                          <Sparkles size={14} /> 生成章节计划
                        </button>
                        <button onClick={() => (showChapterPlanComposer ? resetChapterPlanComposer() : openChapterPlanEditor())} disabled={!detail || Boolean(busy)}>
                          <Plus size={14} /> 手动添加
                        </button>
                      </>
                    ) : libraryFocus !== "foreshadowing" ? (
                      <button onClick={() => (showKnowledgeComposer ? resetKnowledgeComposer() : setShowKnowledgeComposer(true))} disabled={!detail || Boolean(busy)}>
                        <Plus size={14} /> 补充资料
                      </button>
                    ) : null}
                  </>
                )}
              </header>
              {libraryMode === "workbench" && libraryFocus === "setting" && <section className="story-bible-overview">
                <div className="story-bible-plan">
                  <div className="story-bible-plan-copy">
                    <div className="story-bible-plan-title">
                      <strong>当前创作计划</strong>
                      <span className={`story-bible-plan-status ${currentPlanStatus(detail).tone}`}>
                        {currentPlanStatus(detail).label}
                      </span>
                    </div>
                    <div className="story-bible-status-row">
                      <span>世界观、大纲、角色统一维护</span>
                      {detail?.story_bible && <span>Canon v{detail.story_bible.canon_version}</span>}
                      {pendingFoundationCardCount(detail) > 0 && <span>待确认资料卡 {pendingFoundationCardCount(detail)} 张</span>}
                      {pendingChapterPlanCount(detail) > 0 && <span>待确认章节计划 {pendingChapterPlanCount(detail)} 个</span>}
                      {detail?.story_bible_review && <span>审校 {detail.story_bible_review.status === "confirmed" ? adoptedStatusLabel : "待处理"}</span>}
                    </div>
                  </div>
                  <div className="story-bible-plan-actions">
                    <button
                      type="button"
                      className="secondary-action"
                      onClick={focusPlanDiscussion}
                      disabled={!detail || Boolean(busy)}
                    >
                      <MessageSquare size={14} /> 和主 Agent 讨论并完善创作计划
                    </button>
                    <button
                      type="button"
                      className="btn-primary"
                      onClick={confirmCurrentPlan}
                      disabled={!detail || Boolean(busy)}
                    >
                      <Check size={14} /> {adoptionActionLabel}
                    </button>
                  </div>
                </div>
                {detail?.story_bible_review && (
                  <details className="story-bible-review">
                    <summary>
                      一致性审校 · {detail.story_bible_review.issues.length > 0 ? `${detail.story_bible_review.issues.length} 项` : "未发现问题"}
                    </summary>
                    <p>{detail.story_bible_review.summary}</p>
                    {detail.story_bible_review.issues.map((issue, index) => (
                      <article key={`${issue.title}-${index}`} className={`canon-issue ${issue.severity}`}>
                        <strong>{issue.title}</strong><span>{issue.domain} · {issue.severity}</span>
                        <p>{issue.conflict}</p><p>{issue.impact}</p>
                        <p>修复要求：{issue.rework_instruction}</p>
                      </article>
                    ))}
                  </details>
                )}
              </section>}

              {libraryFocus === "outline" && (
                <section className="outline-board" aria-label="章节大纲">
                  <header className="outline-board-head">
                    <div>
                      <strong>章节大纲</strong>
                      <span>{outlinePlans.length > 0 ? `${outlinePlans.length} 个计划` : "暂无章节计划"}</span>
                    </div>
                    {detail?.story_arcs?.[0] && <small>当前阶段：{detail.story_arcs.find((arc) => arc.status === "active")?.title ?? detail.story_arcs[0].title}</small>}
                  </header>
                  {libraryMode === "workbench" && showChapterPlanComposer && (
                    <section className="library-composer chapter-plan-composer">
                      <div className="library-composer-head">
                        <strong>{editingChapterPlanId ? "编辑章节计划" : "新增章节计划"}</strong>
                        <button className="icon-btn" onClick={resetChapterPlanComposer} title="关闭"><ChevronLeft size={15} /></button>
                      </div>
                      <div className="chapter-plan-fields">
                        <label>
                          <span>章节序号</span>
                          <input
                            type="number"
                            min={1}
                            value={chapterPlanNo}
                            onChange={(event) => setChapterPlanNo(Math.max(1, Number(event.target.value) || 1))}
                          />
                        </label>
                        <label>
                          <span>计划标题</span>
                          <input value={chapterPlanTitle} onChange={(event) => setChapterPlanTitle(event.target.value)} placeholder="例如：雨夜入城" />
                        </label>
                      </div>
                      <textarea rows={7} value={chapterPlanContent} onChange={(event) => setChapterPlanContent(event.target.value)} placeholder="写清本章目标、阻力、关键行动和离开状态" />
                      <div className="button-row">
                        <button onClick={() => saveChapterPlan("pending_human_approval")} disabled={!chapterPlanTitle.trim() || !chapterPlanContent.trim() || Boolean(busy)}>
                          <Save size={14} /> 保存待确认
                        </button>
                        <button className="btn-primary" onClick={() => saveChapterPlan("approved")} disabled={!chapterPlanTitle.trim() || !chapterPlanContent.trim() || Boolean(busy)}>
                          <Check size={14} /> {adoptionActionLabel}
                        </button>
                      </div>
                    </section>
                  )}
                  {outlinePlans.length > 0 ? (
                    <div className="outline-chapter-list">
                      {outlinePlans.map((plan) => {
                        const chapter = detail?.chapters.find((item) => item.id === plan.chapter_id)
                          ?? detail?.chapters.find((item) => item.chapter_no === plan.chapter_no);
                        const hasBody = Boolean(chapter?.current_artifact_id);
                        return (
                          <article className="outline-chapter-row" key={plan.id}>
                            <span className="outline-chapter-number">{plan.chapter_no}</span>
                            <span className="outline-chapter-copy">
                              <strong>{plan.title || `第 ${plan.chapter_no} 章`}</strong>
                              <small>{outlineTextSummary(plan.content, "暂无章节任务")}</small>
                            </span>
                            <span className={`outline-chapter-status ${plan.status}`}>
                              {plan.status === "approved" ? (hasBody ? "已有正文" : adoptedStatusLabel) : "待确认"}
                            </span>
                            <span className="outline-plan-actions">
                              {libraryMode === "workbench" && plan.status === "pending_human_approval" && (
                                <button className="icon-btn" onClick={() => void updateChapterPlanStatus(plan, "approved")} disabled={Boolean(busy)} title={`${adoptionActionLabel}章节计划`}><Check size={14} /></button>
                              )}
                              {libraryMode === "workbench" && (
                                <button className="icon-btn" onClick={() => editChapterPlan(plan)} disabled={Boolean(busy)} title="编辑章节计划"><Edit3 size={14} /></button>
                              )}
                              {plan.status === "approved" && (
                                plan.chapter_id ? (
                                  <button className="icon-btn" onClick={() => {
                                    if (chapter) {
                                      selectChapter(chapter, "draft");
                                      switchContentSurface("workbench");
                                    }
                                  }} disabled={!chapter || Boolean(busy)} title="打开正文"><ChevronRight size={15} /></button>
                                ) : (
                                  <button className="outline-plan-create" onClick={() => void createChapterFromPlan(plan)} disabled={Boolean(busy)} title="创建正文">
                                    <Plus size={14} /> 创建正文
                                  </button>
                                )
                              )}
                              {libraryMode === "workbench" && (
                                <>
                                  {plan.status === "approved" && <button className="icon-btn" onClick={() => void updateChapterPlanStatus(plan, "archived")} disabled={Boolean(busy)} title="归档章节计划"><Archive size={14} /></button>}
                                  <button className="icon-btn danger" onClick={() => void deleteChapterPlan(plan)} disabled={Boolean(busy)} title="删除章节计划"><Trash2 size={14} /></button>
                                </>
                              )}
                            </span>
                          </article>
                        );
                      })}
                    </div>
                  ) : (
                    <div className="empty-state">
                      <strong>暂无章节计划</strong>
                      <span>先生成一组章节计划，或手动添加第一章。</span>
                      {libraryMode === "workbench" && (
                        <div className="button-row">
                          <button onClick={() => runStoryArchitect("plan_current_arc")} disabled={!detail || Boolean(busy)}><Sparkles size={14} /> 生成章节计划</button>
                          <button className="secondary-action" onClick={() => openChapterPlanEditor(1)} disabled={!detail || Boolean(busy)}><Plus size={14} /> 手动添加</button>
                        </div>
                      )}
                    </div>
                  )}
                  {outlineCards.length > 0 && (
                    <details className="outline-source-notes">
                      <summary>兼容的大纲资料 · {outlineCards.length} 条</summary>
                      <div>
                        {outlineCards.map((card) => (
                          <article key={card.id}>
                            <strong>{card.title}</strong>
                            <p>{outlineTextSummary(card.content, "暂无摘要")}</p>
                          </article>
                        ))}
                      </div>
                    </details>
                  )}
                </section>
              )}

              <div className="library-layout">
                <section className="library-canvas">
                  {libraryMode === "workbench" && libraryFocus !== "outline" && libraryFocus !== "foreshadowing" && showKnowledgeComposer && (
                    <section className="library-composer">
                      <div className="library-composer-head">
                        <strong>{editingCanonEntryId ? "编辑资料卡" : `补充${librarySection === "setting" ? "设定" : librarySection === "outline" ? "大纲任务" : "角色"}`}</strong>
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
                        <button onClick={() => saveCanonEntry("pending_human_approval")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>保存待确认</button>
                        <button className="btn-primary" onClick={() => saveCanonEntry("approved")} disabled={!knowledgeTitle.trim() || !knowledgeContent.trim() || Boolean(busy)}>
                          <Check size={14} /> {adoptionActionLabel}
                        </button>
                      </div>
                    </section>
                  )}

                  {libraryFocus === "foreshadowing" && (
                    <section className="foreshadowing-ledger">
                      <div className="foreshadowing-ledger-head">
                        <div>
                          <strong>伏笔清单</strong>
                          <span>{visibleForeshadowings.length} 条 · 只记录创作者主动维护的线索</span>
                        </div>
                        {libraryMode === "official" && <span className="read-only-note">只读</span>}
                      </div>
                      {libraryMode === "workbench" && showForeshadowingComposer && (
                        <section className="library-composer foreshadowing-composer">
                          <div className="library-composer-head">
                            <strong>{editingForeshadowingId ? "编辑伏笔" : "登记伏笔"}</strong>
                            <button className="icon-btn" onClick={resetForeshadowingComposer} title="关闭"><ChevronLeft size={15} /></button>
                          </div>
                          <input value={foreshadowingTitle} onChange={(event) => setForeshadowingTitle(event.target.value)} placeholder="伏笔标题" />
                          <textarea rows={4} value={foreshadowingContent} onChange={(event) => setForeshadowingContent(event.target.value)} placeholder="伏笔内容" />
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
                            <button onClick={() => saveForeshadowing("pending_human_approval")} disabled={!foreshadowingTitle.trim() || !foreshadowingContent.trim() || Boolean(busy)}>保存待确认</button>
                            <button className="btn-primary" onClick={() => saveForeshadowing("active")} disabled={!foreshadowingTitle.trim() || !foreshadowingContent.trim() || Boolean(busy)}>
                              <Check size={14} /> {adoptionActionLabel}
                            </button>
                          </div>
                        </section>
                      )}
                      <div className="foreshadowing-list">
                        {visibleForeshadowings.map((item: Foreshadowing) => {
                          const payoffChapter = detail?.chapters.find((chapter) => chapter.id === item.planned_payoff_chapter_id);
                          const statusLabel = item.status === "active" ? "追踪中" : item.status === "ready_for_payoff" ? "可回收" : item.status === "resolved" ? "已回收" : "待确认";
                          return (
                            <article key={item.id} className="foreshadowing-item">
                              <div className="managed-card-head">
                                <span className={`library-status ${item.status}`}>{statusLabel}</span>
                                {libraryMode === "workbench" && <div className="managed-card-actions">
                                  <button className="icon-btn" onClick={() => editForeshadowing(item)} title="编辑伏笔"><Edit3 size={14} /></button>
                                  {item.status === "pending_human_approval" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "active")} title={`${adoptionActionLabel}伏笔`}><Check size={14} /></button>}
                                  {item.status === "active" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "ready_for_payoff")} title="标记可回收"><Sparkles size={14} /></button>}
                                  {item.status === "ready_for_payoff" && <button className="icon-btn" onClick={() => updateForeshadowingStatus(item, "resolved")} title="标记已回收"><Check size={14} /></button>}
                                </div>}
                              </div>
                              <strong>{item.title}</strong>
                              <p>{item.content}</p>
                              <span>{payoffChapter?.title ?? (item.planned_payoff_note || "尚未安排回收")}</span>
                            </article>
                          );
                        })}
                        {visibleForeshadowings.length === 0 && <div className="empty-state">还没有登记伏笔</div>}
                      </div>
                    </section>
                  )}

                  {libraryFocus !== "foreshadowing" && <div className="knowledge-grid">
                    {libraryFocus !== "outline" && libraryCards.map((card) => (
                      <article className="managed-knowledge-card" key={card.id}>
                        <div className="managed-card-head">
                          <span className={`library-status ${card.status}`}>{card.status === "approved" ? adoptedStatusLabel : card.status === "pending_human_approval" ? "待确认" : "已归档"}</span>
                          <strong className="managed-card-title">{card.title}</strong>
                          <div className="managed-card-actions">
                              {libraryMode === "workbench" && (
                              <button className="icon-btn" onClick={() => editCanonEntry(card)} title="编辑资料卡"><Edit3 size={14} /></button>
                              )}
                              {libraryMode === "workbench" && card.status === "pending_human_approval" && <button className="icon-btn" onClick={() => updateCanonEntryStatus(card, "approved")} title={`${adoptionActionLabel}资料卡`}><Check size={14} /></button>}
                              {libraryMode === "workbench" && card.status !== "archived" && <button className="icon-btn" onClick={() => updateCanonEntryStatus(card, "archived")} title="归档资料卡"><Trash2 size={14} /></button>}
                              <button className="icon-btn danger" onClick={() => deleteCanonEntry(card)} title="彻底删除资料卡"><Trash2 size={14} /></button>
                            </div>
                        </div>
                        <KnowledgeSectionCard section={{ title: card.title, content: card.content.split("\n") }} />
                      </article>
                    ))}
                    {libraryFocus !== "outline" && libraryCards.length === 0 && (
                      <div className="empty-state">
                        {libraryArtifactSummary
                          ? "当前没有结构化资料卡。旧版 Markdown 资料不会自动作为卡片显示，请重新运行故事架构 Agent。"
                          : "暂无资料"}
                      </div>
                    )}
                  </div>}
                </section>

              </div>
            </section>) : null}
          <>
          {/* Center: Editor */}
          {mainSurface === "workbench" && (<section className="editor">
            <div className="editor-toolbar">
              <div>
                <div className="editor-title-line">
                  <h2>{isChapterWorkbench ? "章节工作台" : stages.find((stage) => stage.id === selectedStage)?.label}</h2>
                  <span className="workspace-mode-badge draft">
                    {isChapterWorkbench ? chapterFlow?.label ?? "章节候选" : "草稿 / 候选"}
                  </span>
                </div>
                <p>{selectedChapter && !foundationStages.some((stage) => stage.id === selectedStage) ? selectedChapter.title : "整书资料"}</p>
              </div>
              <div className="button-row chapter-toolbar-actions">
                {isChapterWorkbench && chapterFlow ? (
                  <>
                    {chapterFlow.state === "needs_revision" ? (
                      <div className="chapter-agent-handoff" role="status">
                        <MessageSquare size={14} /> 修订建议已填入 Agent
                      </div>
                    ) : (
                      <button
                        className="btn-primary chapter-primary-action"
                        onClick={() => void runChapterPrimaryAction()}
                        disabled={!detail || Boolean(busy) || (chapterFlow.state === "ready_to_adopt" && !chapterFlow.bodyArtifact)}
                      >
                        {chapterFlow.state === "ready_to_adopt" ? <Check size={14} /> : chapterFlow.state === "adopted" ? <Sparkles size={14} /> : <Play size={14} />}
                        {chapterFlow.actionLabel}
                      </button>
                    )}
                    {chapterFlow.state === "needs_revision" && chapterFlow.bodyArtifact && (
                      <button
                        className="secondary-action"
                        onClick={() => void approveBodyArtifact(chapterFlow.bodyArtifact!)}
                        disabled={Boolean(busy)}
                        title="跳过本轮修订，确认采用当前候选稿"
                      >
                        <Check size={14} /> {adoptionActionLabel}
                      </button>
                    )}
                  </>
                ) : (
                  <button onClick={() => runAgent(selectedStage)} disabled={!detail || Boolean(busy)}>
                    <Play size={14} /> {selectedBookArtifactCanIterate ? "基于当前版本迭代" : selectedStage === "revision" ? "生成修订" : "生成"}
                  </button>
                )}
                <button
                  type="button"
                  className="toolbar-icon-btn"
                  onClick={() => setVersionDrawerOpen((open) => !open)}
                  aria-expanded={versionDrawerOpen}
                  title={versionDrawerOpen ? "收起历史版本" : `历史版本${visibleArtifacts.length > 0 ? ` (${visibleArtifacts.length})` : ""}`}
                >
                  <History size={15} />
                  {visibleArtifacts.length > 0 && <span className="toolbar-icon-badge">{visibleArtifacts.length}</span>}
                </button>
                <DropdownMenu
                  label="更多"
                  className="toolbar-more"
                  triggerClassName="toolbar-more-trigger"
                  menuClassName="toolbar-more-menu"
                  menuWidth={168}
                  align="end"
                >
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

            {!isChapterWorkbench && productionStages.some((stage) => stage.id === selectedStage) && <div className="workflow-strip" aria-label="章节创作流程">
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
            </div>}

            {versionDrawerOpen && (
              <div className="version-drawer">
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
              </div>
            )}

            <article className={streamingRun ? "artifact-view streaming-artifact" : "artifact-view"}>
              {streamingRun ? (
                <>
                  <div className="artifact-meta">
                    <strong>生成中</strong>
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
                  {streamingRun.output ? <pre>{streamingRun.output}</pre> : <div className="streaming-placeholder" aria-label="等待输出"><Loader2 size={18} className="spin" /></div>}
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

          {/* Right: Agent chat belongs only to the workbench, never to read-only official content. */}
          {currentContentSurface() === "workbench" && (
          <>
          <div
            className={assistantPanelResizing ? "assistant-resize-handle dragging" : "assistant-resize-handle"}
            role="separator"
            aria-label="调整 Agent 侧栏宽度"
            aria-orientation="vertical"
            aria-valuemin={ASSISTANT_PANEL_MIN_WIDTH}
            aria-valuemax={ASSISTANT_PANEL_MAX_WIDTH}
            aria-valuenow={assistantPanelWidth}
            tabIndex={0}
            title="拖动调整 Agent 侧栏宽度"
            onPointerDown={beginAssistantPanelResize}
            onPointerMove={resizeAssistantPanel}
            onPointerUp={endAssistantPanelResize}
            onPointerCancel={endAssistantPanelResize}
            onKeyDown={resizeAssistantPanelWithKeyboard}
          />
          <aside
            ref={assistantPanelRef}
            className="assistant-panel assistant-panel-v2"
            style={{ width: assistantPanelWidth, minWidth: assistantPanelWidth }}
          >
            <header className="assistant-workspace-header">
              <div className="assistant-workspace-identity">
                <div className="assistant-workspace-avatar"><Sparkles size={15} /></div>
                <div>
                  <strong>Agent 工作区</strong>
                  <span className="assistant-agent-role">主 Agent · 对话与任务编排</span>
                </div>
              </div>
              <div className="assistant-workspace-actions">
                {orchestratorRunIsActive && (
                  <button
                    type="button"
                    className="assistant-stop-run"
                    onClick={() => void cancelOrchestratorRun()}
                    disabled={orchestratorCancellationRequested || busy === "停止 Agent"}
                    title={orchestratorCancellationRequested ? "正在停止 Agent" : "停止 Agent"}
                    aria-label={orchestratorCancellationRequested ? "正在停止 Agent" : "停止 Agent"}
                  >
                    {orchestratorCancellationRequested ? <Loader2 size={14} className="spin" /> : <X size={14} />}
                  </button>
                )}
                <button
                  type="button"
                  className="assistant-new-chat"
                  onClick={() => {
                    assistantStickToBottomRef.current = true;
                    mainFeedScrollTopRef.current = null;
                    setAssistantMessages([]);
                    setAssistantHistoryCutoff(Date.now());
                    setInstruction("");
                    setLiveToolEvents([]);
                    setAssistantTimelineEvents([]);
                    setSelectedSubagentRunId(null);
                    setLastAgentRun(null);
                    setAssistantAdvancedOpen(false);
                    setOrchestratorParentRunId(null);
                    setOrchestratorCancellationRequested(false);
                    setDelegatedRunEvents({});
                    setDelegatedRunSummaries({});
                    setThinkingRounds([]);
                  }}
                  title="新建会话"
                  aria-label="新建会话"
                >
                  <Plus size={14} />
                </button>
              </div>
            </header>

            <div className="assistant-context-strip">
              <span role="status" className={busy || orchestratorRunIsActive ? "assistant-status busy" : "assistant-status"}>
                <span className="assistant-status-dot" />
                {orchestratorCancellationRequested ? "正在停止" : busy || orchestratorRunIsActive ? "Agent 执行中" : "已就绪"}
              </span>
              {selectedChapter && <span className="assistant-context-chip">章节 · {selectedChapter.title}</span>}
              <span className="assistant-context-chip">{isChapterWorkbench ? `章节状态 · ${chapterFlow?.label ?? "候选稿"}` : `当前阶段 · ${stageLabel(selectedStage)}`}</span>
            </div>

            <div
              ref={assistantFeedRef}
              className={`assistant-chat-feed${selectedSubagentRunId != null ? " assistant-chat-feed-subagent" : ""}`}
            >
              {assistantFeedItems.length === 0 && selectedSubagentRunId == null && (
                <article className="assistant-message assistant-message-agent assistant-empty-state">
                  <div className="assistant-message-body">
                    <div className="assistant-message-meta"><strong>Book Agent</strong><span>准备好了</span></div>
                    <div className="assistant-suggestion-list">
                      <button type="button" onClick={() => useAssistantPrompt("基于当前设定，给出下一步最值得推进的创作建议")}>下一步建议 <ChevronRight size={12} /></button>
                      <button type="button" onClick={() => useAssistantPrompt("检查当前内容是否存在角色或时间线矛盾")}>检查连续性 <ChevronRight size={12} /></button>
                    </div>
                  </div>
                </article>
              )}

              {selectedSubagentRunId == null && assistantFeedItems.map((item) => {
                if (item.kind === "user") {
                  return <article className="assistant-message assistant-message-user" key={item.id}><div className="assistant-message-body"><p>{item.content}</p></div></article>;
                }
                if (item.kind === "memory_warning") {
                  return (
                    <article className="assistant-memory-warning" key={item.id}>
                      <AlertCircle size={14} />
                      <div>
                        <strong>章节交接记忆生成失败</strong>
                        <p>{item.content}</p>
                        <small>写下一章时会自动重试；也可让子 Agent 重新生成。</small>
                      </div>
                    </article>
                  );
                }
                if (item.kind === "thinking") {
                  return <AssistantThinkingPanel key={item.id} content={item.content} active={item.active} />;
                }
                if (item.kind === "tool") {
                  return <article className="assistant-tool-message" key={item.id}><div className="assistant-tool-message-mark">{item.status === "success" ? "✓" : item.status === "running" ? "•" : "!"}</div><div className="assistant-tool-message-body"><div className="assistant-message-meta"><strong>{assistantToolLabel(item.toolKey)}</strong><span>{item.status === "running" ? "执行中" : item.status === "success" ? "已完成" : "失败"}</span></div><p>{item.summary || (item.status === "running" ? "正在调用工具……" : "工具已返回结果")}</p></div></article>;
                }
                if (item.kind === "subagent") {
                  const childEvents = delegatedRunEvents[item.runId] ?? [];
                  const latest = childEvents[childEvents.length - 1];
                  return <button type="button" className="assistant-subagent-timeline-item" key={item.id} onClick={() => { const feed = assistantFeedRef.current; if (feed) mainFeedScrollTopRef.current = feed.scrollTop; assistantStickToBottomRef.current = true; setSelectedSubagentRunId(item.runId); }}><span className="assistant-subagent-mark"><Bot size={13} /></span><span className="assistant-subagent-timeline-copy"><strong>{item.title}</strong><small className="assistant-subagent-status">{latest?.status === "success" ? "已完成" : latest?.status === "failed" ? "失败" : "执行中"}</small></span><ChevronRight size={14} /></button>;
                }
                const outputText = compactAssistantOutput(item.content);
                const isLiveOutput = item.id === liveOutputItemId;
                return (
                  <article
                    className={`assistant-message assistant-message-agent${isLiveOutput ? " assistant-output-live" : ""}`}
                    key={item.id}
                  >
                    <div className="assistant-message-body">
                      {outputText.length > 240
                        ? <details className="assistant-output-details"><summary>查看全文（{outputText.length} 字）</summary><p>{outputText}</p></details>
                        : <p>{outputText}</p>}
                      {!isLiveOutput && item.id === lastOutputItemId && chapterFlow?.bodyArtifact && (
                        <button type="button" className="assistant-output-link" onClick={openCandidatePanel}>
                          {chapterFlow?.state === "adopted" ? "查看正文" : "打开候选稿"}
                          <ChevronRight size={12} />
                        </button>
                      )}
                    </div>
                  </article>
                );
              })}

              {selectedSubagentRunId != null && (
                <>
                  <nav className="assistant-breadcrumb" aria-label="子 Agent 会话位置">
                    <button type="button" className="assistant-breadcrumb-root" onClick={() => setSelectedSubagentRunId(null)}>
                      <ChevronLeft size={13} /> 主 Agent
                    </button>
                    <span className="assistant-breadcrumb-sep">/</span>
                    <strong className="assistant-breadcrumb-title">
                      {selectedSubagentEvents[0]?.task_title || selectedSubagentSummary?.run.task_title || "子 Agent 会话"}
                    </strong>
                  </nav>
                  {selectedSubagentSummary && (
                    <p className="assistant-subagent-meta">
                      运行 #{selectedSubagentSummary.run.id} · {runStatusLabel(selectedSubagentSummary.run.status)}
                      {selectedSubagentSummary.run.elapsed_ms > 0 ? ` · ${selectedSubagentSummary.run.elapsed_ms.toLocaleString()} ms` : ""}
                      {` · ${selectedSubagentSummary.tool_invocations.length} 次工具调用`}
                    </p>
                  )}
                  {selectedSubagentProcess.map((item) => {
                    if (item.kind === "thinking") {
                      return <AssistantThinkingPanel key={item.id} content={item.content} active={item.active} />;
                    }
                    if (item.kind === "tool") {
                      const invocation = item.invocationId != null
                        ? selectedSubagentSummary?.tool_invocations.find((candidate) => candidate.id === item.invocationId)
                        : undefined;
                      const statusLabel = item.status === "running" ? "执行中" : item.status === "success" ? "已完成" : item.status === "rejected" ? "已拒绝" : "失败";
                      return (
                        <article className="assistant-tool-message" key={item.id}>
                          <div className="assistant-tool-message-mark">{item.status === "success" ? "✓" : item.status === "running" ? "•" : "!"}</div>
                          {invocation ? (
                            <details className="assistant-tool-message-body assistant-tool-entry">
                              <summary className="assistant-message-meta">
                                <strong>{assistantToolLabel(item.toolKey)}</strong>
                                <span>{statusLabel}{invocation.elapsed_ms > 0 ? ` · ${invocation.elapsed_ms.toLocaleString()} ms` : ""}</span>
                              </summary>
                              <label>参数</label>
                              <pre>{JSON.stringify(invocation.arguments, null, 2)}</pre>
                              <label>{invocation.error ? "错误" : "结果"}</label>
                              <pre>{invocation.error ?? JSON.stringify(invocation.result, null, 2)}</pre>
                            </details>
                          ) : (
                            <div className="assistant-tool-message-body">
                              <div className="assistant-message-meta">
                                <strong>{assistantToolLabel(item.toolKey)}</strong>
                                <span>{statusLabel}</span>
                              </div>
                              <p>{item.summary || (item.status === "running" ? "正在调用工具……" : "工具已返回结果")}</p>
                            </div>
                          )}
                        </article>
                      );
                    }
                    if (item.kind !== "output") return null;
                    const output = item.content;
                    return output.trim().length > 240 || /[\[{]/.test(output.trim())
                      ? <details className="assistant-subagent-output-panel" key={item.id}><summary><span>查看输出</span><small>{output.trim().length} 字</small></summary><pre>{output}</pre></details>
                      : <p className="assistant-subagent-output" key={item.id}>{output}</p>;
                  })}
                </>
              )}

            </div>

            {(streamingRun || lastAgentRun) && !orchestratorParentRunId && (
              <details
                className={`assistant-workflow-panel${streamingRun ? " assistant-workflow-panel-live" : ""}`}
                open={!workflowStepsCollapsed}
                onToggle={(event) => setWorkflowStepsCollapsed(!event.currentTarget.open)}
              >
                <summary>
                  <span><Check size={13} /> 执行状态</span>
                </summary>
                <div className="assistant-workflow-todo">
                  <div className="assistant-workflow-todo-item done">
                    <span>✓</span><strong>准备</strong>
                  </div>
                  <div className={`assistant-workflow-todo-item${activeLiveTool ? " active" : completedLiveTool ? " done" : " pending"}`}>
                    <span>{completedLiveTool ? "✓" : activeLiveTool ? "•" : "·"}</span><strong>工具</strong>
                  </div>
                  <div className={`assistant-workflow-todo-item${waitingForModelOutput ? " active" : streamingRun ? " pending" : " done"}`}>
                    <span>{streamingRun ? "•" : "✓"}</span><strong>生成结果</strong>
                  </div>
                  <div className={`assistant-workflow-todo-item${streamingRun ? " pending" : " done"}`}>
                    <span>{streamingRun ? "·" : "✓"}</span><strong>完成</strong>
                  </div>
                </div>
              </details>
            )}

            <div className="assistant-composer-dock">
            <div className="assistant-composer">
              <textarea
                ref={assistantInputRef}
                className="assistant-chat-input"
                rows={3}
                value={instruction}
                placeholder={selectedBookArtifactCanIterate ? "告诉 Agent 只改哪里，其他内容保持不变…" : "今天帮你推进什么？"}
                onChange={(event) => setInstruction(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter") return;
                  // Shift/Alt+Enter 交给 textarea 插入换行；Enter 与 Ctrl/Cmd+Enter 发送。
                  if (event.shiftKey || event.altKey) return;
                  event.preventDefault();
                  submitAssistantPrompt();
                }}
              />
              <div className="assistant-composer-actions">
                <div className="assistant-composer-tools">
                  <button
                    type="button"
                    className="assistant-composer-tool-button"
                    onClick={() => setAssistantAdvancedOpen((open) => !open)}
                    aria-expanded={assistantAdvancedOpen}
                    aria-label={assistantAdvancedOpen ? "收起工作区工具" : "打开工作区工具"}
                    title={assistantAdvancedOpen ? "收起工作区工具" : "打开工作区工具"}
                  >
                    {assistantAdvancedOpen ? <X size={17} /> : <Plus size={17} />}
                    {pendingActionProposalCount > 0 && <span className="assistant-composer-tool-badge">{pendingActionProposalCount}</span>}
                  </button>
                  <span className="assistant-composer-context-label">
                    <Sparkles size={12} />
                    <span className="assistant-ellipsis-text">{agentCatalog.find((agent) => agent.stage === selectedStage)?.name ?? "主 Agent"}</span>
                  </span>
                  <span className="assistant-composer-model" title={settings.model || "默认模型"}>
                    {compactAssistantModelName(settings.model || "默认模型")}
                  </span>
                </div>
                <button
                  type="button"
                  className={`assistant-send-button${busy || orchestratorRunIsActive ? " is-busy" : ""}`}
                  onClick={submitAssistantPrompt}
                  disabled={!detail || !instruction.trim() || Boolean(busy) || orchestratorRunIsActive}
                  title={busy || orchestratorRunIsActive ? "执行中" : "发送"}
                  aria-label={busy || orchestratorRunIsActive ? "执行中" : "发送"}
                >
                  {busy || orchestratorRunIsActive
                    ? <Loader2 size={14} className="spin" />
                    : <><Send size={14} /> <span>发送</span></>}
                </button>
              </div>
            </div>

            <details
              className="assistant-advanced-controls"
              open={assistantAdvancedOpen}
              onToggle={(event) => setAssistantAdvancedOpen(event.currentTarget.open)}
            >
              <summary>
                <SlidersHorizontal size={14} />
                <strong>工作区工具</strong>
                <span>参考资料与质量检查{pendingActionProposalCount > 0 ? ` · ${pendingActionProposalCount} 条待确认` : ""}</span>
              </summary>
              <div className="assistant-advanced-content">
            <section className="panel next-action-panel">
              <input
                ref={referenceFileInputRef}
                className="visually-hidden"
                type="file"
                accept=".txt,text/plain"
                onChange={importReferenceFile}
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

            {isChapterWorkbench && chapterFlow && (
              <section className="panel next-action-panel chapter-candidate-panel">
                <div className="panel-title">
                  <Sparkles size={14} />
                  候选稿
                </div>
                <div className="chapter-candidate-card">
                  <div className="chapter-candidate-card-head">
                    <strong>
                      {chapterFlow.bodyArtifact
                        ? `${chapterFlow.bodyArtifact.stage === "revision" ? "修订稿" : "正文"} v${chapterFlow.bodyArtifact.version}`
                        : "尚无候选稿"}
                    </strong>
                    <span>{chapterFlow.label}</span>
                  </div>
                  {chapterFlow.reviewArtifact && (
                    <p>
                      试读报告：{chapterFlow.reviewIssueCount > 0 ? `发现 ${chapterFlow.reviewIssueCount} 个问题` : "未发现结构化问题"}
                    </p>
                  )}
                </div>
              </section>
            )}

            <details className="tools-group">
              <summary>质量与连续性</summary>
              <div className="tools-group-content">

            <section className="panel">
              <div className="panel-title">
                <BarChart3 size={14} />
                质量检查
              </div>
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

              </div>
            </details>
            </div>
          </aside>
          </>
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

    </main>
  );
}
