import { RefreshCcw } from "lucide-react";
import { useMemo } from "react";
import type { ReactNode } from "react";
import type {
  Chapter,
  ChapterMemoryEntry,
  ChapterMemoryView,
  ProjectWorkspace,
  StoryEntity,
  StoryEvent,
  StoryEventParticipant,
  StoryFact,
  StoryIndexSource,
} from "../types";
import { Select } from "./Select";

export type StoryIndexStatus = {
  approved: number;
  succeeded: number;
  pending: number;
  running: number;
  failed: Array<{ chapter: Chapter; source?: StoryIndexSource }>;
};

export type EntityTimelineEntry =
  | { type: "event"; id: number; chapterId: number | null; event: StoryEvent }
  | { type: "fact"; id: number; chapterId: number | null; fact: StoryFact };

const continuityViews = ["chapter-memory", "events", "character-timeline", "items"] as const;
export type ContinuityLibraryFocus = (typeof continuityViews)[number];
type EntityFocus = Exclude<ContinuityLibraryFocus, "events" | "chapter-memory">;

type ContinuityLibraryPanelProps = {
  focus: ContinuityLibraryFocus;
  detail: Pick<ProjectWorkspace, "chapters" | "index_jobs" | "story_entities" | "story_events" | "chapter_memories"> | null;
  busy: boolean;
  status: StoryIndexStatus;
  participantsByEvent: Map<number, StoryEventParticipant[]>;
  entities: StoryEntity[];
  selectedEntity: StoryEntity | null;
  currentFacts: StoryFact[];
  timeline: EntityTimelineEntry[];
  onRebuild: () => void;
  onSelectFocus: (focus: ContinuityLibraryFocus) => void;
  onSelectEntity: (entityId: number) => void;
  onOpenEntity: (focus: EntityFocus, entityId: number) => void;
  onOpenChapter: (chapterId: number) => void;
};

const continuityViewMeta = {
  "chapter-memory": {
    title: "章节交接记忆",
    selectionLabel: "",
    empty: "暂无章节记忆",
  },
  "character-timeline": {
    title: "角色时间线",
    selectionLabel: "选择角色",
    empty: "暂无角色记录",
  },
  items: {
    title: "物品状态",
    selectionLabel: "选择物品或资源",
    empty: "暂无物品或资源记录",
  },
  events: {
    title: "事件时间线",
    selectionLabel: "",
    empty: "暂无事件",
  },
} as const;

const memorySectionMeta: Array<{
  key: keyof Pick<
    ChapterMemoryView,
    "summary" | "final_scene" | "last_action" | "state_changes" | "knowledge_changes" | "commitments" | "open_loops" | "immediate_next_intent" | "plan_reconciliation"
  >;
  label: string;
  list?: boolean;
}> = [
  { key: "summary", label: "章节摘要" },
  { key: "final_scene", label: "结尾画面" },
  { key: "last_action", label: "最后行动" },
  { key: "state_changes", label: "状态变化", list: true },
  { key: "knowledge_changes", label: "认知变化", list: true },
  { key: "commitments", label: "承诺与打算", list: true },
  { key: "open_loops", label: "未闭合线索", list: true },
  { key: "immediate_next_intent", label: "下一步意图" },
  { key: "plan_reconciliation", label: "计划对齐", list: true },
];

function memoryEntries(
  view: ChapterMemoryView,
  key: (typeof memorySectionMeta)[number]["key"],
  list: boolean,
): ChapterMemoryEntry[] {
  const value = view[key];
  if (!value) return [];
  if (list) return value as ChapterMemoryEntry[];
  return [value as ChapterMemoryEntry];
}

function chapterMemoryTitle(memory: ChapterMemoryView): string {
  const title = memory.chapter_title.trim();
  const prefix = `第 ${memory.chapter_no} 章`;
  if (!title || title.startsWith(prefix)) return title || prefix;
  return `${prefix} · ${title}`;
}

function MemoryCard({ title, copy, quote, chapter, onOpenChapter, children }: {
  title: string;
  copy: string;
  quote: string;
  chapter?: Chapter;
  onOpenChapter: (chapterId: number) => void;
  children?: ReactNode;
}) {
  return (
    <article className="entity-timeline-entry">
      <header className="memory-card-heading">
        <h4>{title}</h4>
        <span className="library-status">{chapter ? `第 ${chapter.chapter_no} 章` : "章节未定"}</span>
      </header>
      <p>{copy}</p>
      {children}
      {quote && <blockquote>{quote}</blockquote>}
      {chapter?.current_artifact_id != null && (
        <button type="button" className="timeline-source" onClick={() => onOpenChapter(chapter.id)}>查看正文</button>
      )}
    </article>
  );
}

export function ContinuityLibraryPanel({
  focus,
  detail,
  busy,
  status,
  participantsByEvent,
  entities,
  selectedEntity,
  currentFacts,
  timeline,
  onRebuild,
  onSelectFocus,
  onSelectEntity,
  onOpenEntity,
  onOpenChapter,
}: ContinuityLibraryPanelProps) {
  const viewMeta = continuityViewMeta[focus];
  const chaptersById = useMemo(() => new Map(detail?.chapters.map((chapter) => [chapter.id, chapter])), [detail?.chapters]);
  const entityKinds = useMemo(() => new Map(detail?.story_entities?.map((entity) => [entity.id, entity.kind])), [detail?.story_entities]);
  const events = useMemo(() => [...(detail?.story_events ?? [])].sort((left, right) => {
    const leftChapter = chaptersById.get(left.narrative_chapter_id ?? 0)?.chapter_no ?? Infinity;
    const rightChapter = chaptersById.get(right.narrative_chapter_id ?? 0)?.chapter_no ?? Infinity;
    return (leftChapter - rightChapter) || left.id - right.id;
  }), [chaptersById, detail?.story_events]);
  const showingEvents = focus === "events";
  const showingMemories = focus === "chapter-memory";
  const entityFocused = focus === "character-timeline" || focus === "items";
  const memories = useMemo(
    () => [...(detail?.chapter_memories ?? [])].sort((left, right) => left.chapter_no - right.chapter_no),
    [detail?.chapter_memories],
  );
  const count = showingEvents ? events.length : showingMemories ? memories.length : timeline.length;
  const indexJobs = detail?.index_jobs ?? [];
  const pendingJobs = indexJobs.filter((job) => job.status === "pending");
  const runningJobs = indexJobs.filter((job) => job.status === "running");
  const failedJobs = indexJobs.filter((job) => job.status === "failed");
  const jobTypeLabel = (jobType: string) => {
    if (jobType === "story_chapter") return "资料";
    if (jobType === "search_chapter") return "章节检索";
    if (jobType === "search_project") return "整书检索";
    return jobType;
  };

  const indexChip = status.approved > 0 ? (
    <span
      className="library-status-chip"
      title={status.failed
        .map(({ chapter, source }) => `第 ${chapter.chapter_no} 章：${source?.error ?? "更新失败"}`)
        .join("\n") || undefined}
    >
      <span className="library-status-chip-key">资料索引</span>
      <span className="library-status-chip-value">已覆盖 {status.succeeded}/{status.approved} 章</span>
      {status.pending > 0 && <span className="library-status-chip-warn">{status.pending} 章待更新</span>}
      {status.running > 0 && <span className="library-status-chip-warn">{status.running} 章更新中</span>}
      {status.failed.length > 0 && (
        <span className="library-status-chip-error">{status.failed.length} 章更新失败</span>
      )}
    </span>
  ) : null;

  const jobChip =
    pendingJobs.length > 0 || runningJobs.length > 0 || failedJobs.length > 0 ? (
      <span
        className="library-status-chip"
        title={failedJobs
          .map((job) => `${jobTypeLabel(job.job_type)}：${job.last_error ?? "更新失败"}`)
          .join("\n") || undefined}
      >
        <span className="library-status-chip-key">后台任务</span>
        {pendingJobs.length > 0 && <span>{pendingJobs.length} 个待处理</span>}
        {runningJobs.length > 0 && <span>{runningJobs.length} 个进行中</span>}
        {failedJobs.length > 0 && (
          <span className="library-status-chip-error">{failedJobs.length} 个失败</span>
        )}
      </span>
    ) : null;

  const memoryChip = status.approved > 0 ? (
    <span className="library-status-chip">
      <span className="library-status-chip-key">章节记忆</span>
      <span className="library-status-chip-value">已生成 {memories.length}/{status.approved} 章</span>
      {memories.length < status.approved && (
        <span className="library-status-chip-warn">{status.approved - memories.length} 章待生成</span>
      )}
    </span>
  ) : null;

  return (
    <section className="library-workspace continuity-workspace" aria-labelledby="chapter-memory-title">
      <header className="library-header continuity-header">
        <div>
          <span className="library-context-label">资料库</span>
          <h2 id="chapter-memory-title">章节记忆</h2>
          <p>已采用正文 · 只读</p>
        </div>
        <div className="continuity-header-side">
          <div className="library-status-chips">
            {indexChip}
            {memoryChip}
            {jobChip}
          </div>
          <button type="button" onClick={onRebuild} disabled={!detail || busy}>
            <RefreshCcw size={14} /> 更新资料索引
          </button>
        </div>
      </header>

      <section className="continuity-library">
        <nav className="surface-switch memory-tabs" aria-label="章节记忆分类">
          {continuityViews.map((view) => (
            <button
              key={view}
              type="button"
              className={view === focus ? "active" : ""}
              aria-pressed={view === focus}
              onClick={() => onSelectFocus(view)}
            >
              {continuityViewMeta[view].title}
            </button>
          ))}
        </nav>
        <div className="memory-content" key={focus}>
          <div className="memory-section-heading">
            <h3>{viewMeta.title}</h3>
            {entityFocused && entities.length > 0 && (
              <Select
                className="memory-entity-select"
                aria-label={viewMeta.selectionLabel}
                value={String(selectedEntity?.id ?? "")}
                options={entities.map((entity) => ({
                  value: String(entity.id),
                  label: entity.kind === "resource" ? `${entity.name} · 资源` : entity.name,
                }))}
                onChange={(value) => onSelectEntity(Number(value))}
              />
            )}
            <small>{showingMemories ? `${count} 章记忆` : `${count} 条记录`}</small>
          </div>
          {entityFocused && currentFacts.length > 0 && (
            <dl className="entity-current-state" aria-label="当前状态">
              {currentFacts.map((fact) => (
                <div key={fact.dimension}>
                  <dt>{fact.dimension}</dt>
                  <dd>{fact.value}</dd>
                </div>
              ))}
            </dl>
          )}
          {count === 0 ? (
            <div className="empty-state compact">
              {showingMemories ? "暂无章节记忆，开始写下一章时会自动补全上一章的交接记忆" : selectedEntity ? "暂无状态变化记录" : viewMeta.empty}
            </div>
          ) : showingMemories ? (
            <div className="chapter-memory-list">
              {memories.map((memory) => {
                const chapter = chaptersById.get(memory.chapter_id);
                return (
                  <article className="chapter-memory-card" key={memory.chapter_id}>
                    <header className="memory-card-heading">
                      <h4>{chapterMemoryTitle(memory)}</h4>
                      <span className="library-status">已采用正文</span>
                    </header>
                    <dl className="chapter-memory-sections">
                      {memorySectionMeta.map((section) => {
                        const entries = memoryEntries(memory, section.key, Boolean(section.list));
                        if (entries.length === 0) return null;
                        return (
                          <div className="chapter-memory-section" key={section.key}>
                            <dt>{section.label}</dt>
                            <dd>
                              {entries.map((entry, index) => (
                                <p key={`${section.key}-${index}`}>
                                  <span>{entry.text}</span>
                                  {entry.evidence_quote && entry.evidence_quote !== entry.text && (
                                    <blockquote>{entry.evidence_quote}</blockquote>
                                  )}
                                </p>
                              ))}
                            </dd>
                          </div>
                        );
                      })}
                    </dl>
                    {chapter?.current_artifact_id != null && (
                      <button type="button" className="timeline-source" onClick={() => onOpenChapter(chapter.id)}>查看正文</button>
                    )}
                  </article>
                );
              })}
            </div>
          ) : (
            <div className="memory-card-grid">
              {showingEvents ? events.map((event) => {
                const participants = participantsByEvent.get(event.id) ?? [];
                return (
                  <MemoryCard
                    key={event.id}
                    title={event.title}
                    copy={event.summary}
                    quote={event.source_quote}
                    chapter={chaptersById.get(event.narrative_chapter_id ?? 0)}
                    onOpenChapter={onOpenChapter}
                  >
                    {event.story_time && <small className="memory-story-time">{event.story_time}</small>}
                    {participants.length > 0 && (
                      <div className="timeline-participants">
                        {participants.map((participant) => {
                          const kind = entityKinds.get(participant.entity_id);
                          const target = kind === "character" ? "character-timeline"
                            : kind === "item" || kind === "resource" ? "items" : null;
                          const key = `${participant.entity_id}-${participant.role}`;
                          const label = <>{participant.entity_name}<span>{participant.role}</span></>;
                          return target ? (
                            <button type="button" key={key} onClick={() => onOpenEntity(target, participant.entity_id)}>{label}</button>
                          ) : (
                            <span className="memory-participant" key={key}>{label}</span>
                          );
                        })}
                      </div>
                    )}
                  </MemoryCard>
                );
              }) : timeline.map((entry) => (
                <MemoryCard
                  key={`${entry.type}-${entry.id}`}
                  title={entry.type === "event" ? entry.event.title : entry.fact.dimension}
                  copy={entry.type === "event" ? entry.event.summary : entry.fact.value}
                  quote={entry.type === "event" ? entry.event.source_quote : entry.fact.source_quote}
                  chapter={chaptersById.get(entry.chapterId ?? 0)}
                  onOpenChapter={onOpenChapter}
                />
              ))}
            </div>
          )}
        </div>
      </section>
    </section>
  );
}
