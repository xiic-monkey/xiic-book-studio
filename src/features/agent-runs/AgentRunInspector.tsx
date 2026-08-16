import { Check, ChevronDown, Clock3, ShieldCheck, Wrench, X } from "lucide-react";
import type { ActionProposal, AgentRunSummary, ToolInvocation } from "../../types";

interface AgentRunInspectorProps {
  run: AgentRunSummary | null;
  proposals: ActionProposal[];
  busy: boolean;
  onApplyProposal: (proposal: ActionProposal) => void;
  onRejectProposal: (proposal: ActionProposal) => void;
}

function invocationStatusLabel(invocation: ToolInvocation) {
  if (invocation.status === "success") return "成功";
  if (invocation.status === "rejected") return "已拒绝";
  return invocation.error ? "失败" : invocation.status;
}

function proposalTypeLabel(proposalType: string) {
  const labels: Record<string, string> = {
    create_chapter: "创建章节",
    rename_chapter: "重命名章节",
    artifact_candidate: "资料候选版本",
    knowledge_card: "知识卡候选",
    foreshadowing: "伏笔候选",
  };
  return labels[proposalType] ?? proposalType;
}

export function AgentRunInspector({
  run,
  proposals,
  busy,
  onApplyProposal,
  onRejectProposal,
}: AgentRunInspectorProps) {
  const invocations = run?.tool_invocations ?? [];
  const pendingProposals = proposals.filter((proposal) => proposal.status === "pending");
  if (!run && pendingProposals.length === 0) return null;

  return (
    <section className="agent-run-inspector">
      <header className="agent-run-inspector-head">
        <div>
          <strong>本次运行</strong>
          <span>
            {run ? `运行 #${run.run.id} · ${run.run.status}` : "当前项目"}
            {pendingProposals.length > 0 ? ` · ${pendingProposals.length} 条待确认提案` : ""}
          </span>
        </div>
        <ShieldCheck size={17} />
      </header>

      {run && (
        <div className="agent-run-summary-row">
          <span><Clock3 size={13} /> {run.run.elapsed_ms.toLocaleString()} ms</span>
          <span><Wrench size={13} /> {invocations.length} 次工具调用</span>
          <span>{run.prepared_context_id ? `上下文 #${run.prepared_context_id}` : "未复用预览"}</span>
        </div>
      )}

      {invocations.length > 0 && (
        <div className="agent-run-tool-list">
          {invocations.map((invocation) => (
            <details key={invocation.id} className="agent-run-tool-item">
              <summary>
                <span>
                  <ChevronDown size={13} />
                  <strong>{invocation.tool_key}</strong>
                </span>
                <small>
                  {invocation.protocol} · {invocationStatusLabel(invocation)} · {invocation.elapsed_ms} ms
                </small>
              </summary>
              <div className="agent-run-tool-payload">
                <label>参数</label>
                <pre>{JSON.stringify(invocation.arguments, null, 2)}</pre>
                <label>{invocation.error ? "错误" : "结果"}</label>
                <pre>{invocation.error ?? JSON.stringify(invocation.result, null, 2)}</pre>
              </div>
            </details>
          ))}
        </div>
      )}

      {pendingProposals.length > 0 && (
        <div className="action-proposal-list">
          {pendingProposals.map((proposal) => (
            <article key={proposal.id} className="action-proposal-card">
              <div>
                <small>{proposalTypeLabel(proposal.proposal_type)} · 提案 #{proposal.id}</small>
                <strong>{proposal.summary}</strong>
                {proposal.expected_version && <span>目标版本：{proposal.expected_version}</span>}
              </div>
              <details>
                <summary>查看结构化内容</summary>
                <pre>{JSON.stringify(proposal.payload, null, 2)}</pre>
              </details>
              <div className="button-row">
                <button
                  type="button"
                  className="btn-primary"
                  disabled={busy}
                  onClick={() => onApplyProposal(proposal)}
                >
                  <Check size={13} /> 人工确认并应用
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => onRejectProposal(proposal)}
                >
                  <X size={13} /> 拒绝
                </button>
              </div>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
