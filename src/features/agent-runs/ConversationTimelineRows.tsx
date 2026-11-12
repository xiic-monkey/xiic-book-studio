import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import {
  AlertTriangle,
  Bot,
  Brain,
  Check,
  ChevronRight,
  ExternalLink,
  FileSignature,
  FileText,
  Layers,
  ListChecks,
  Loader2,
  Megaphone,
  PenLine,
  Search,
  ShieldCheck,
  Square,
  Wrench,
  X,
} from "lucide-react";

/*
 * 对话时间线行组件。视觉与交互对齐 ZCode 的 conversation timeline：
 * 思考行（流式扫光 + 折叠耗时）、工具行（紧凑单行 + 展开载荷）、
 * 子 Agent 任务行、底部 spinner。样式类见 styles.css 的 assistant-tl-* 段。
 */

export type AssistantToolStatus = "running" | "success" | "failed" | "rejected";

export type AssistantToolDetailSection = { label: string; text: string };

/** 取思考流式文本的最后一个非空行，作为收起态的单行摘要。 */
export function reasoningStreamingSummary(text: string): string {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index]?.trim() ?? "";
    if (line.length > 0) return line;
  }
  return "";
}

/** 思考耗时秒数：向上取整且至少 1 秒；缺时间戳时返回 undefined。 */
export function reasoningDurationSeconds(
  startedAtMs?: number,
  endedAtMs?: number | null,
  nowMs?: number,
): number | undefined {
  if (startedAtMs == null || !Number.isFinite(startedAtMs)) return undefined;
  const end = endedAtMs ?? nowMs;
  if (end == null || !Number.isFinite(end)) return undefined;
  return Math.max(1, Math.ceil((end - startedAtMs) / 1000));
}

/** "持续了 N 秒"；没有可靠时间戳时退化为 "持续了几秒"。 */
export function reasoningDurationText(
  startedAtMs?: number,
  endedAtMs?: number | null,
  nowMs?: number,
): string | undefined {
  const seconds = reasoningDurationSeconds(startedAtMs, endedAtMs, nowMs);
  if (seconds === undefined) return "持续了几秒";
  return `持续了 ${seconds} 秒`;
}

const TOOL_ICON_RULES: [RegExp, ReactNode][] = [
  [/search|query/i, <Search size={14} />],
  [/prepare|context/i, <Layers size={14} />],
  [/create|write/i, <PenLine size={14} />],
  [/continuity|check/i, <ShieldCheck size={14} />],
  [/get_|read|reference|memory|bible/i, <FileText size={14} />],
];

/** 工具行的类别图标：按 tool_key 关键词归类，未识别的用通用扳手。 */
export function assistantToolGlyph(toolKey: string): ReactNode {
  for (const [pattern, icon] of TOOL_ICON_RULES) {
    if (pattern.test(toolKey)) return icon;
  }
  return <Wrench size={14} />;
}

function toggleOnActivation(key: string, event: KeyboardEvent<HTMLElement>, toggle: () => void) {
  if (key !== "Enter" && key !== " ") return;
  event.preventDefault();
  toggle();
}

type ReasoningRowProps = {
  content: string;
  streaming: boolean;
  startedAtMs?: number;
  endedAtMs?: number | null;
  defaultOpen?: boolean;
};

/** 思考行：流式时“正在思考 · 末行摘要”，结束后折叠为“思考 · 持续了 N 秒”。 */
export function AssistantReasoningRow({ content, streaming, startedAtMs, endedAtMs, defaultOpen = false }: ReasoningRowProps) {
  const [open, setOpen] = useState(defaultOpen);
  const userInteractedRef = useRef(false);
  const prevStreamingRef = useRef(streaming);
  const [, setTick] = useState(0);
  const summaryViewportRef = useRef<HTMLSpanElement | null>(null);
  const summaryTextRef = useRef<HTMLSpanElement | null>(null);
  const [summaryOverflowing, setSummaryOverflowing] = useState(false);
  const contentScrollRef = useRef<HTMLDivElement | null>(null);
  const contentFollowBottomRef = useRef(true);

  // 结束时自动收起，但用户手动展开/收起后自动规则不再覆盖其选择。
  useEffect(() => {
    if (prevStreamingRef.current && !streaming && !userInteractedRef.current) {
      setOpen(false);
    }
    prevStreamingRef.current = streaming;
  }, [streaming]);

  // 展开且流式时按秒重算耗时；收起态不为隐藏的耗时触发重渲染。
  useEffect(() => {
    if (!streaming || !open) return;
    const timer = window.setInterval(() => setTick((tick) => tick + 1), 1000);
    return () => window.clearInterval(timer);
  }, [streaming, open]);

  const streamingSummary = streaming && !open ? reasoningStreamingSummary(content) : "";

  // 单行摘要持续增长时把视口推到末尾，让最新内容可见、旧内容向左移出。
  useEffect(() => {
    const viewport = summaryViewportRef.current;
    if (!viewport || !streamingSummary) return;
    setSummaryOverflowing(viewport.scrollWidth > viewport.clientWidth + 1);
    viewport.scrollLeft = viewport.scrollWidth;
  }, [streamingSummary]);

  // 展开态流式追加时默认吸底跟随；用户上翻后暂停，滚回底部再恢复。
  useEffect(() => {
    const node = contentScrollRef.current;
    if (!node || !open) return;
    if (contentFollowBottomRef.current) node.scrollTop = node.scrollHeight;
  }, [content, open]);

  if (streaming && content.trim().length === 0) return null;

  const toggle = () => {
    userInteractedRef.current = true;
    setOpen((current) => !current);
  };

  const durationText = streaming
    ? undefined
    : reasoningDurationText(startedAtMs, endedAtMs);
  const runningDurationText = streaming && open
    ? reasoningDurationText(startedAtMs, null, Date.now())
    : undefined;

  const handleContentScroll = () => {
    const node = contentScrollRef.current;
    if (!node) return;
    contentFollowBottomRef.current =
      node.scrollHeight - node.clientHeight - node.scrollTop <= 2;
  };

  return (
    <div className="assistant-tl-item">
      <div
        role="button"
        tabIndex={0}
        aria-expanded={open}
        aria-label={streaming ? "正在思考" : "思考过程"}
        className={`assistant-tl-row${open ? " is-open" : ""}`}
        onClick={toggle}
        onKeyDown={(event) => toggleOnActivation(event.key, event, toggle)}
      >
        <span className="assistant-tl-icon"><Brain size={14} /></span>
        <span className="assistant-tl-label">
          {streaming
            ? <span className="animated-gradient-text assistant-tl-thinking-live">正在思考</span>
            : "思考"}
        </span>
        {(durationText ?? runningDurationText) && (
          <>
            <span className="assistant-tl-sep">·</span>
            <span className="assistant-tl-status">{durationText ?? runningDurationText}</span>
          </>
        )}
        {streamingSummary ? (
          <>
            <span className="assistant-tl-sep">·</span>
            <span
              ref={summaryViewportRef}
              className={`assistant-tl-stream-preview${summaryOverflowing ? " is-overflowing" : ""}`}
            >
              <span ref={summaryTextRef}>{streamingSummary}</span>
            </span>
          </>
        ) : null}
        <ChevronRight size={14} className="assistant-tl-chevron" />
      </div>
      {content.trim().length > 0 && (
        <div className={`assistant-tl-collapse${open ? " is-open" : ""}`}>
          <div className="assistant-tl-collapse-inner">
            <div
              ref={contentScrollRef}
              className="assistant-tl-thinking-content"
              onScroll={handleContentScroll}
            >
              {content}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

type ToolRowProps = {
  icon?: ReactNode;
  label: string;
  status: AssistantToolStatus;
  summary?: string;
  meta?: string;
  sections?: AssistantToolDetailSection[];
  /** 首次展开时触发：真实载荷不在事件流里，需要按 run 懒取。 */
  onExpand?: () => void;
  defaultOpen?: boolean;
};

const TOOL_STATUS_TEXT: Record<AssistantToolStatus, string> = {
  running: "执行中",
  success: "",
  failed: "失败",
  rejected: "已拒绝",
};

// 历史数据里存在"已返回（结构化）结果"这类零信息填充摘要，不上折叠行预览。
const GENERIC_TOOL_SUMMARY = /^(工具)?已返回(结构化)?结果\s*[。.!！]?$/;

function meaningfulToolSummary(summary?: string) {
  const trimmed = summary?.trim();
  if (!trimmed || GENERIC_TOOL_SUMMARY.test(trimmed)) return "";
  return trimmed;
}

/** 工具行：图标 + 名称 + 状态 + 单行摘要，可展开查看序号/耗时与参数、结果载荷。 */
export function AssistantToolRow({ icon, label, status, summary, meta, sections, onExpand, defaultOpen = false }: ToolRowProps) {
  const [open, setOpen] = useState(defaultOpen);
  const detailSections = (sections ?? []).filter((section) => section.text.trim().length > 0);
  const previewText = meaningfulToolSummary(summary);
  const noteText = !detailSections.length ? previewText : "";
  const canToggle = detailSections.length > 0 || noteText.length > 0 || Boolean(onExpand);
  const statusText = TOOL_STATUS_TEXT[status];

  const toggle = () => {
    if (!canToggle) return;
    const next = !open;
    if (next) onExpand?.();
    setOpen(next);
  };

  return (
    <div className="assistant-tl-item">
      <div
        role={canToggle ? "button" : undefined}
        tabIndex={canToggle ? 0 : undefined}
        aria-expanded={canToggle ? open : undefined}
        className={`assistant-tl-row${open ? " is-open" : ""}${canToggle ? "" : " is-static"}`}
        onClick={toggle}
        onKeyDown={
          canToggle
            ? (event) => toggleOnActivation(event.key, event, toggle)
            : undefined
        }
      >
        <span className="assistant-tl-icon">{icon ?? <Wrench size={14} />}</span>
        <span className="assistant-tl-label">{label}</span>
        {statusText ? (
          <>
            <span className="assistant-tl-sep">·</span>
            <span
              className={`assistant-tl-status${status === "running" ? " is-running" : ""}${status === "failed" ? " is-failed" : ""}`}
            >
              {status === "running"
                ? <span className="animated-gradient-text">{statusText}</span>
                : statusText}
            </span>
          </>
        ) : null}
        {previewText && status !== "running" ? (
          <>
            <span className="assistant-tl-sep">·</span>
            <span className="assistant-tl-preview">{previewText}</span>
          </>
        ) : null}
        {canToggle ? <ChevronRight size={14} className="assistant-tl-chevron" /> : null}
      </div>
      {canToggle && (
        <div className={`assistant-tl-collapse${open ? " is-open" : ""}`}>
          <div className="assistant-tl-collapse-inner">
            {detailSections.length > 0 ? (
              <div className="assistant-tl-tool-detail">
                {meta ? <div className="assistant-tl-tool-meta">{meta}</div> : null}
                {detailSections.map((section) => (
                  <div className="assistant-tl-tool-detail-section" key={section.label}>
                    <label>{section.label}</label>
                    <pre>{section.text}</pre>
                  </div>
                ))}
              </div>
            ) : (
              <div className="assistant-tl-tool-detail">
                {meta ? <div className="assistant-tl-tool-meta">{meta}</div> : null}
                {noteText ? <p className="assistant-tl-tool-note">{noteText}</p> : <p className="assistant-tl-tool-note">载荷载入中…</p>}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

type SubagentRowProps = {
  title: string;
  statusLabel: string;
  running: boolean;
  onOpen: () => void;
  expanded?: boolean;
  onToggle?: () => void;
  children?: ReactNode;
  onCancel?: () => void;
  cancelling?: boolean;
  busy?: boolean;
};

/**
 * 子 Agent 任务行：默认就地展开子时间线（原生 harness 的内联嵌套），
 * 「单独查看」仍进入面包屑全屏视图；运行中可就地停止该子任务。
 */
export function AssistantSubagentRow({
  title,
  statusLabel,
  running,
  onOpen,
  expanded = false,
  onToggle,
  children,
  onCancel,
  cancelling = false,
  busy = false,
}: SubagentRowProps) {
  const hasChildren = Array.isArray(children) ? children.length > 0 : Boolean(children);
  const toggle = onToggle ? () => onToggle() : undefined;

  return (
    <div className="assistant-tl-item assistant-tl-subagent-item">
      <div
        role={toggle ? "button" : undefined}
        tabIndex={toggle ? 0 : undefined}
        aria-expanded={toggle ? expanded : undefined}
        className={`assistant-tl-row assistant-tl-subagent${expanded ? " is-open" : ""}${toggle ? "" : " is-static"}`}
        onClick={toggle}
        onKeyDown={toggle ? (event) => toggleOnActivation(event.key, event, toggle) : undefined}
      >
        <span className="assistant-tl-icon"><Bot size={14} /></span>
        <span className="assistant-tl-label assistant-tl-label-fluid">{title}</span>
        <span className="assistant-tl-sep">·</span>
        <span className={`assistant-tl-status${running ? " is-running" : ""}`}>
          {running ? <span className="animated-gradient-text">{statusLabel}</span> : statusLabel}
        </span>
        <span className="assistant-tl-row-actions">
          <button
            type="button"
            className="assistant-tl-secondary-action"
            onClick={(event) => {
              event.stopPropagation();
              onOpen();
            }}
            title="单独查看子会话"
            aria-label="单独查看子会话"
          >
            <ExternalLink size={13} />
          </button>
          {onCancel && (
            <button
              type="button"
              className="assistant-tl-stop-action"
              onClick={(event) => {
                event.stopPropagation();
                onCancel();
              }}
              disabled={cancelling || busy}
              title={cancelling ? "正在停止子任务" : "停止该子任务"}
              aria-label={cancelling ? "正在停止子任务" : "停止该子任务"}
            >
              {cancelling ? <Loader2 size={13} className="spin" /> : <Square size={12} />}
            </button>
          )}
          {toggle && <ChevronRight size={14} className="assistant-tl-chevron" />}
        </span>
      </div>
      {hasChildren && (
        <div className={`assistant-tl-collapse${expanded ? " is-open" : ""}`}>
          <div className="assistant-tl-collapse-inner">
            <div className="assistant-tl-group-children">{children}</div>
          </div>
        </div>
      )}
    </div>
  );
}

type ToolGroupRowProps = {
  countsLabel: string;
  children: ReactNode;
  defaultOpen?: boolean;
};

/** 查阅聚合行：连续的检索/读取类工具收成一行，展开后带左导线显示子行。 */
export function AssistantToolGroupRow({ countsLabel, children, defaultOpen = false }: ToolGroupRowProps) {
  const [open, setOpen] = useState(defaultOpen);
  const hasChildren = Array.isArray(children) ? children.length > 0 : Boolean(children);

  const toggle = () => {
    if (!hasChildren) return;
    setOpen((current) => !current);
  };

  return (
    <div className="assistant-tl-item">
      <div
        role="button"
        tabIndex={0}
        aria-expanded={open}
        aria-label={`查阅 ${countsLabel}`}
        className={`assistant-tl-row${open ? " is-open" : ""}`}
        onClick={toggle}
        onKeyDown={(event) => toggleOnActivation(event.key, event, toggle)}
      >
        <span className="assistant-tl-icon"><Search size={14} /></span>
        <span className="assistant-tl-label">查阅</span>
        <span className="assistant-tl-sep">·</span>
        <span className="assistant-tl-status">{countsLabel}</span>
        <ChevronRight size={14} className="assistant-tl-chevron" />
      </div>
      {hasChildren && (
        <div className={`assistant-tl-collapse${open ? " is-open" : ""}`}>
          <div className="assistant-tl-collapse-inner">
            <div className="assistant-tl-group-children">{children}</div>
          </div>
        </div>
      )}
    </div>
  );
}

/** 会话底部忙碌指示：与 ZCode 一致的小号 Loader，占一行。 */
export function AssistantChatLoading({ active }: { active: boolean }) {
  if (!active) return null;
  return (
    <div className="assistant-tl-row assistant-tl-loading" role="status" aria-label="执行中">
      <Loader2 size={14} className="spin" />
    </div>
  );
}

type PlanTask = {
  index: number;
  title: string;
  taskType: string;
  status: string;
};

const PLAN_TASK_TYPE_LABEL: Record<string, string> = {
  story_architect: "架构",
  draft: "草稿",
  review: "审校",
  revision: "修订",
};

const PLAN_STATUS_GLYPH: Record<string, string> = {
  pending: "·",
  running: "•",
  success: "✓",
  failed: "!",
  skipped: "—",
  cancelled: "×",
};

/** 计划卡：主 Agent 的任务清单与实时进度，执行阶段一眼可见（对齐 harness 的 todo 列表）。 */
export function AssistantPlanRow({ tasks, progressLabel }: { tasks: PlanTask[]; progressLabel: string }) {
  return (
    <div className="assistant-tl-item assistant-plan">
      <div className="assistant-tl-row is-static">
        <span className="assistant-tl-icon"><ListChecks size={14} /></span>
        <span className="assistant-tl-label">任务计划</span>
        <span className="assistant-tl-sep">·</span>
        <span className="assistant-tl-status">{progressLabel}</span>
      </div>
      <ol className="assistant-plan-list">
        {tasks.map((task) => (
          <li key={task.index} className={`assistant-plan-task is-${task.status}`}>
            <span className="assistant-plan-glyph" aria-hidden>
              {task.status === "running"
                ? <Loader2 size={12} className="spin" />
                : PLAN_STATUS_GLYPH[task.status] ?? "·"}
            </span>
            <span className="assistant-plan-title">{task.title}</span>
            <small className="assistant-plan-type">{PLAN_TASK_TYPE_LABEL[task.taskType] ?? task.taskType}</small>
          </li>
        ))}
      </ol>
    </div>
  );
}

/** 委托播报行：主 Agent 的过程性说明；警示态用强调色，其余为次要文本。 */
export function AssistantNarrationRow({ content, tone }: { content: string; tone: "info" | "warning" }) {
  if (!content.trim()) return null;
  return (
    <div className={`assistant-tl-narration${tone === "warning" ? " is-warning" : ""}`}>
      {tone === "warning" ? <AlertTriangle size={13} /> : <Megaphone size={13} />}
      <span>{content}</span>
    </div>
  );
}

type ProposalRowProps = {
  summary: string;
  typeLabel: string;
  busy: boolean;
  onApply: () => void;
  onReject: () => void;
};

/** 待确认提议：紧跟产生它的运行行，就地决定采用或放弃。 */
export function AssistantProposalRow({ summary, typeLabel, busy, onApply, onReject }: ProposalRowProps) {
  return (
    <div className="assistant-tl-item assistant-proposal">
      <div className="assistant-proposal-head">
        <span className="assistant-tl-icon"><FileSignature size={14} /></span>
        <span className="assistant-tl-label">{typeLabel}</span>
      </div>
      <p className="assistant-proposal-summary">{summary}</p>
      <div className="assistant-proposal-actions">
        <button type="button" className="assistant-proposal-apply" onClick={onApply} disabled={busy}>
          <Check size={12} /> 确认采用
        </button>
        <button type="button" className="assistant-proposal-reject" onClick={onReject} disabled={busy}>
          <X size={12} /> 放弃
        </button>
      </div>
    </div>
  );
}
