import { Check, Edit3, Loader2, RefreshCcw, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import type { AdoptionProposal, Chapter, Foreshadowing, KnowledgeCard } from "../types";

type Props = {
  open: boolean;
  proposals: AdoptionProposal[];
  chapters: Chapter[];
  knowledgeCards: KnowledgeCard[];
  foreshadowings: Foreshadowing[];
  busy: boolean;
  onClose: () => void;
  onExtract: () => void;
  onSave: (proposalId: number, data: Record<string, unknown>) => Promise<void>;
  onApply: (proposalIds: number[], note: string) => Promise<void>;
  onReject: (proposalIds: number[], note: string) => Promise<void>;
};

const categories = [
  ["world", "世界观"],
  ["cultivation", "修行体系"],
  ["map", "地图与地点"],
  ["faction", "势力与组织"],
  ["taboo", "禁忌与边界"],
  ["item", "重要物件"],
  ["outline", "大纲"],
  ["character", "角色"],
  ["other", "其他"],
] as const;

function text(data: Record<string, unknown>, key: string) {
  return typeof data[key] === "string" ? data[key] as string : "";
}

function optionalId(data: Record<string, unknown>, key: string) {
  return typeof data[key] === "number" ? data[key] as number : "";
}

function targetLabel(proposal: AdoptionProposal) {
  if (proposal.target_kind === "foreshadowing") return "伏笔";
  const category = text(proposal.data, "category");
  return categories.find(([value]) => value === category)?.[1] ?? "资料卡";
}

export function AdoptionDrawer({
  open,
  proposals,
  chapters,
  knowledgeCards,
  foreshadowings,
  busy,
  onClose,
  onExtract,
  onSave,
  onApply,
  onReject,
}: Props) {
  const [drafts, setDrafts] = useState<Record<number, Record<string, unknown>>>({});
  const [selectedIds, setSelectedIds] = useState<number[]>([]);
  const [note, setNote] = useState("");

  useEffect(() => {
    const nextDrafts = Object.fromEntries(proposals.map((proposal) => [proposal.id, { ...proposal.data }]));
    setDrafts(nextDrafts);
    setSelectedIds(
      proposals
        .filter((proposal) => proposal.status === "pending" && !proposal.validation_error)
        .map((proposal) => proposal.id)
    );
  }, [proposals]);

  const grouped = useMemo(() => {
    const pending = proposals.filter((proposal) => proposal.status === "pending");
    const stale = proposals.filter((proposal) => proposal.status === "stale");
    const decided = proposals.filter((proposal) => proposal.status === "applied" || proposal.status === "rejected");
    return { pending, stale, decided };
  }, [proposals]);

  if (!open) return null;

  function setField(id: number, key: string, value: unknown) {
    setDrafts((current) => ({ ...current, [id]: { ...(current[id] ?? {}), [key]: value } }));
    setSelectedIds((current) => current.filter((valueId) => valueId !== id));
  }

  function toggle(id: number) {
    setSelectedIds((current) => current.includes(id) ? current.filter((value) => value !== id) : [...current, id]);
  }

  function existingSummary(proposal: AdoptionProposal) {
    if (!proposal.target_id) return null;
    if (proposal.target_kind === "knowledge_card") {
      const item = knowledgeCards.find((card) => card.id === proposal.target_id);
      return item ? `${item.title}\n${item.content}` : null;
    }
    const item = foreshadowings.find((thread) => thread.id === proposal.target_id);
    return item ? `${item.title}\n${item.content}` : null;
  }

  return createPortal(
    <div className="adoption-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <aside className="adoption-drawer" role="dialog" aria-modal="true" aria-label="待采纳资料变更">
        <header className="adoption-drawer-head">
          <div>
            <h2>资料变更</h2>
            <p>只有勾选并人工采纳的内容会进入后续写作依据。</p>
          </div>
          <div className="button-row">
            <button onClick={onExtract} disabled={busy}>
              {busy ? <Loader2 className="spin" size={14} /> : <RefreshCcw size={14} />} 重新整理
            </button>
            <button className="icon-btn" onClick={onClose} title="关闭" aria-label="关闭资料变更">
              <X size={16} />
            </button>
          </div>
        </header>

        <div className="adoption-drawer-body">
          {grouped.pending.length === 0 && (
            <div className="adoption-empty">当前产物没有待采纳资料。可以重新整理，或直接关闭。</div>
          )}
          {grouped.pending.map((proposal) => {
            const data = drafts[proposal.id] ?? proposal.data;
            const existing = existingSummary(proposal);
            return (
              <section className={`adoption-item ${proposal.validation_error ? "invalid" : ""}`} key={proposal.id}>
                <div className="adoption-item-head">
                  <label>
                    <input
                      type="checkbox"
                      checked={selectedIds.includes(proposal.id)}
                      onChange={() => toggle(proposal.id)}
                      disabled={Boolean(proposal.validation_error)}
                    />
                    <strong>{targetLabel({ ...proposal, data })}</strong>
                  </label>
                  <span>{proposal.operation === "update" ? "更新现有资料" : "新增资料"}</span>
                </div>

                {proposal.target_kind === "knowledge_card" && (
                  <select value={text(data, "category")} onChange={(event) => setField(proposal.id, "category", event.target.value)}>
                    {categories.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                  </select>
                )}
                <input value={text(data, "title")} onChange={(event) => setField(proposal.id, "title", event.target.value)} placeholder="标题" />
                <textarea rows={5} value={text(data, "content")} onChange={(event) => setField(proposal.id, "content", event.target.value)} placeholder="确认后的事实内容" />

                {proposal.target_kind === "knowledge_card" ? (
                  <select value={optionalId(data, "source_chapter_id")} onChange={(event) => setField(proposal.id, "source_chapter_id", Number(event.target.value) || null)}>
                    <option value="">不绑定章节</option>
                    {chapters.map((chapter) => <option value={chapter.id} key={chapter.id}>{chapter.title}</option>)}
                  </select>
                ) : (
                  <div className="adoption-fields-row">
                    <select value={optionalId(data, "planted_chapter_id")} onChange={(event) => setField(proposal.id, "planted_chapter_id", Number(event.target.value) || null)}>
                      <option value="">埋设章节未指定</option>
                      {chapters.map((chapter) => <option value={chapter.id} key={chapter.id}>{chapter.title}</option>)}
                    </select>
                    <select value={optionalId(data, "planned_payoff_chapter_id")} onChange={(event) => setField(proposal.id, "planned_payoff_chapter_id", Number(event.target.value) || null)}>
                      <option value="">回收章节未指定</option>
                      {chapters.map((chapter) => <option value={chapter.id} key={chapter.id}>{chapter.title}</option>)}
                    </select>
                    <input value={text(data, "planned_payoff_note")} onChange={(event) => setField(proposal.id, "planned_payoff_note", event.target.value)} placeholder="回收里程碑" />
                  </div>
                )}

                {existing && (
                  <details className="adoption-existing">
                    <summary>查看当前资料</summary>
                    <pre>{existing}</pre>
                  </details>
                )}
                <blockquote>{proposal.evidence_quote}</blockquote>
                {proposal.validation_error && <p className="adoption-error">{proposal.validation_error}</p>}
                <button className="adoption-save" onClick={() => onSave(proposal.id, data)} disabled={busy}>
                  <Edit3 size={14} /> 保存修改并重新校验
                </button>
              </section>
            );
          })}

          {(grouped.stale.length > 0 || grouped.decided.length > 0) && (
            <details className="adoption-history">
              <summary>已处理与失效记录 ({grouped.stale.length + grouped.decided.length})</summary>
              {[...grouped.stale, ...grouped.decided].map((proposal) => (
                <div key={proposal.id}>
                  <strong>{text(proposal.data, "title") || `候选 #${proposal.id}`}</strong>
                  <span>{proposal.status === "applied" ? "已采纳" : proposal.status === "rejected" ? "已拒绝" : "已失效"}</span>
                </div>
              ))}
            </details>
          )}
        </div>

        <footer className="adoption-drawer-foot">
          <input value={note} onChange={(event) => setNote(event.target.value)} placeholder="人工确认备注，可为空" />
          <div className="button-row">
            <button onClick={() => onReject(selectedIds, note)} disabled={selectedIds.length === 0 || busy}>
              <Trash2 size={14} /> 拒绝所选
            </button>
            <button className="btn-primary" onClick={() => onApply(selectedIds, note)} disabled={selectedIds.length === 0 || busy}>
              <Check size={14} /> 采纳所选 ({selectedIds.length})
            </button>
          </div>
        </footer>
      </aside>
    </div>,
    document.body
  );
}
