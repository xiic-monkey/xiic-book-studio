import { RefreshCcw } from "lucide-react";
import type {
  Chapter,
  ProjectWorkspace,
  StoryEntity,
  StoryEvent,
  StoryEventParticipant,
  StoryFact,
  StoryIndexSource,
} from "../types";

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

type ContinuityLibraryPanelProps = {
  focus: "characters" | "items" | "events";
  readOnly?: boolean;
  detail: ProjectWorkspace | null;
  busy: boolean;
  status: StoryIndexStatus;
  participantsByEvent: Map<number, StoryEventParticipant[]>;
  entities: StoryEntity[];
  selectedEntity: StoryEntity | null;
  currentFacts: StoryFact[];
  timeline: EntityTimelineEntry[];
  onRebuild: () => void;
  onSelectEntity: (entityId: number) => void;
  onOpenEntity: (entityId: number, kind: string) => void;
  onOpenChapter: (chapterId?: number | null) => void;
};

export function ContinuityLibraryPanel({
  focus,
  readOnly = true,
  detail,
  busy,
  status,
  participantsByEvent,
  entities,
  selectedEntity,
  currentFacts,
  timeline,
  onRebuild,
  onSelectEntity,
  onOpenEntity,
  onOpenChapter,
}: ContinuityLibraryPanelProps) {
  const title = focus === "characters" ? "角色" : focus === "items" ? "物品与资源" : "事件";
  const isCharacterView = focus === "characters";
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

  return (
    <section className="library-workspace continuity-workspace">
      <header className="library-header">
        <div>
          <h2>{title}</h2>
          {status.approved > 0 && (
            <div className={status.failed.length > 0 ? "library-index-status has-error" : "library-index-status"}>
              <span>资料索引</span>
              <strong>已覆盖 {status.succeeded}/{status.approved} 章</strong>
              {status.pending > 0 && <small>{status.pending} 章待更新</small>}
              {status.running > 0 && <small>{status.running} 章正在更新</small>}
              {status.failed.length > 0 && (
                <small title={status.failed.map(({ chapter, source }) => `第 ${chapter.chapter_no} 章：${source?.error ?? "更新失败"}`).join("\n")}>
                  {status.failed.length} 章更新失败
                </small>
              )}
            </div>
          )}
          {(pendingJobs.length > 0 || runningJobs.length > 0 || failedJobs.length > 0) && (
            <div className={failedJobs.length > 0 ? "library-index-status has-error" : "library-index-status"}>
              <span>后台任务</span>
              {pendingJobs.length > 0 && <small>{pendingJobs.length} 个待处理</small>}
              {runningJobs.length > 0 && <small>{runningJobs.length} 个进行中</small>}
              {failedJobs.length > 0 && (
                <small
                  title={failedJobs
                    .map((job) => `${jobTypeLabel(job.job_type)}：${job.last_error ?? "更新失败"}`)
                    .join("\n")}
                >
                  {failedJobs.length} 个失败
                </small>
              )}
            </div>
          )}
        </div>
        <button onClick={onRebuild} disabled={!detail || busy}>
          <RefreshCcw size={14} /> 更新资料索引
        </button>
      </header>

      <section className="continuity-library">
        {focus === "events" ? (
          <div className="event-timeline">
            {(detail?.story_events ?? []).map((event) => {
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
                          <button
                            key={`${participant.event_id}-${participant.entity_id}-${participant.role}`}
                            onClick={() => onOpenEntity(
                              participant.entity_id,
                              detail?.story_entities.find((entity) => entity.id === participant.entity_id)?.kind ?? "character"
                            )}
                          >
                            {participant.entity_name}<span>{participant.role}</span>
                          </button>
                        ))}
                      </div>
                    )}
                    <blockquote>{event.source_quote}</blockquote>
                    {chapter && <button className="timeline-source" onClick={() => onOpenChapter(chapter.id)}>查看正式正文</button>}
                  </div>
                </article>
              );
            })}
            {(detail?.story_events?.length ?? 0) === 0 && <div className="empty-state compact">暂无事件</div>}
          </div>
        ) : (
          <div className="entity-timeline-layout">
            <nav className="entity-list" aria-label={isCharacterView ? "角色列表" : "物品与资源列表"}>
              {entities.map((entity) => (
                <button
                  key={entity.id}
                  className={entity.id === selectedEntity?.id ? "active" : ""}
                  onClick={() => onSelectEntity(entity.id)}
                >
                  <strong>{entity.name}</strong>
                  <span>{entity.kind === "character" ? "角色" : entity.kind === "resource" ? "资源" : "物品"}</span>
                </button>
              ))}
              {entities.length === 0 && <div className="empty-inline">暂无可用索引</div>}
            </nav>

            <section className="entity-detail">
              {selectedEntity ? (
                <>
                  <header className="entity-detail-head">
                    <div>
                      <span>{selectedEntity.kind === "character" ? "角色" : selectedEntity.kind === "resource" ? "资源" : "物品"}</span>
                      <h4>{selectedEntity.name}</h4>
                    </div>
                    <small>已索引</small>
                  </header>
                  {currentFacts.length > 0 && (
                    <div className="entity-current-state">
                      {currentFacts.map((fact) => (
                        <div key={fact.dimension}>
                          <span>{fact.dimension}</span>
                          <strong>{fact.value}</strong>
                        </div>
                      ))}
                    </div>
                  )}
                  <div className="entity-timeline">
                    {timeline.map((entry) => {
                      const chapter = detail?.chapters.find((item) => item.id === entry.chapterId);
                      if (entry.type === "event") {
                        return (
                          <article className="entity-timeline-entry event" key={`event-${entry.id}`}>
                            <span className="entity-timeline-chapter">{chapter ? `第 ${chapter.chapter_no} 章` : "章节未定"}</span>
                            <strong>{entry.event.title}</strong>
                            <p>{entry.event.summary}</p>
                            <blockquote>{entry.event.source_quote}</blockquote>
                            {chapter && <button className="timeline-source" onClick={() => onOpenChapter(chapter.id)}>查看正式正文</button>}
                          </article>
                        );
                      }
                      return (
                        <article className="entity-timeline-entry" key={`fact-${entry.id}`}>
                          <span className="entity-timeline-chapter">{chapter ? `第 ${chapter.chapter_no} 章` : "章节未定"}</span>
                          <strong>{entry.fact.dimension}</strong>
                          <p>{entry.fact.value}</p>
                          <blockquote>{entry.fact.source_quote}</blockquote>
                          {chapter && <button className="timeline-source" onClick={() => onOpenChapter(chapter.id)}>查看正式正文</button>}
                        </article>
                      );
                    })}
                    {timeline.length === 0 && <div className="empty-inline">尚无该实体的状态变化记录</div>}
                  </div>
                </>
              ) : <div className="empty-state compact">选择{isCharacterView ? "角色" : "物品"}</div>}
            </section>
          </div>
        )}
      </section>
    </section>
  );
}
