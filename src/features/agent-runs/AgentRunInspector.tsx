import { Check, ChevronDown, Clock3, ShieldCheck, Wrench, X } from "lucide-react";
import type { ActionProposal, AgentRunSummary, ToolInvocation } from "../../types";

interface AgentRunInspectorProps {
  run: AgentRunSummary | null;
  proposals: ActionProposal[];
  busy: boolean;
  mode?: "full" | "compact";
  onApplyProposal: (proposal: ActionProposal) => void;
  onRejectProposal: (proposal: ActionProposal) => void;
}

const toolLabels: Record<string, string> = {
  story_context_search: "检索故事资料",
  prepare_agent_context: "准备创作上下文",
  check_continuity: "检查连续性",
  create_artifact: "生成候选版本",
  get_current_artifact: "读取当前版本",
  get_story_bible: "读取创作基准",
  get_chapter_context: "读取当前章节",
  search_story: "检索故事内容",
};

function toolLabel(toolKey: string) {
  return toolLabels[toolKey] ?? toolKey.replace(/[_-]+/g, " ").replace(/(^|\s)\S/g, (letter) => letter.toUpperCase());
}

function invocationStatusLabel(invocation: ToolInvocation) {
  if (invocation.status === "success") return "成功";
  if (invocation.status === "rejected") return "已拒绝";
  if (["running", "started", "pending"].includes(invocation.status)) return "执行中";
  return invocation.error ? "失败" : invocation.status || "已完成";
}

function invocationStatusClass(invocation: ToolInvocation) {
  if (invocation.error || invocation.status === "failed") return "failed";
  if (["running", "started", "pending"].includes(invocation.status)) return "running";
  return "success";
}

function resultSummary(invocation: ToolInvocation) {
  if (invocation.error) return invocation.error;
  const result = invocation.result ?? {};
  const record = result as Record<string, unknown>;
  const summary = [record.summary, record.message, record.description].find(
    (value): value is string => typeof value === "string" && value.trim().length > 0,
  );
  if (summary) return summary;
  const count = [record.count, record.total, record.result_count, record.source_count].find(
    (value): value is number => typeof value === "number",
  );
  if (count !== undefined) return `返回 ${count.toLocaleString()} 条结果`;
  return "已返回结构化结果";
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

function CompactRunInspector({ run }: { run: AgentRunSummary }) {
  const invocations = run.tool_invocations ?? [];
  return (
    <section className="agent-run-inspector agent-run-inspector-compact">
      <header className="agent-run-inspector-head">
        <div>
          <strong>执行记录</strong>
          <span>{invocations.length > 0 ? `${invocations.length} 个工具调用` : "本次未调用额外工具"} · {run.run.elapsed_ms.toLocaleString()} ms</span>
        </div>
        <Wrench size={15} />
      </header>
      {invocations.length > 0 ? (
        <div className="agent-run-tool-list">
          {invocations.map((invocation) => (
            <details key={invocation.id} className={`agent-run-tool-item agent-run-tool-item-${invocationStatusClass(invocation)}`}>
              <summary>
                <span>
                  <ChevronDown size={13} />
                  <strong>{toolLabel(invocation.tool_key)}</strong>
                </span>
                <small><i /> {invocationStatusLabel(invocation)} · {invocation.elapsed_ms} ms</small>
              </summary>
              <div className="agent-run-tool-compact-result">
                <span>{resultSummary(invocation)}</span>
                <details>
                  <summary>查看详情</summary>
                  <div className="agent-run-tool-payload">
                    <label>工具</label>
                    <code>{invocation.tool_key}</code>
                    <label>参数</label>
                    <pre>{JSON.stringify(invocation.arguments, null, 2)}</pre>
                    <label>{invocation.error ? "错误" : "结果"}</label>
                    <pre>{invocation.error ?? JSON.stringify(invocation.result, null, 2)}</pre>
                  </div>
                </details>
              </div>
            </details>
          ))}
        </div>
      ) : (
        <p className="agent-run-empty">Agent 直接基于当前上下文完成了这次响应。</p>
      )}
    </section>
  );
}

export function AgentRunInspector({
  run,
  proposals,
  busy,
  mode = "full",
  onApplyProposal,
  onRejectProposal,
}: AgentRunInspectorProps) {
  const invocations = run?.tool_invocations ?? [];
  const pendingProposals = proposals.filter((proposal) => proposal.status === "pending");
  if (!run && pendingProposals.length === 0) return null;
  if (mode === "compact" && run) return <CompactRunInspector run={run} />;

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
            <details key={invocation.id} className={`agent-run-tool-item agent-run-tool-item-${invocationStatusClass(invocation)}`}>
              <summary>
                <span>
                  <ChevronDown size={13} />
                  <strong>{toolLabel(invocation.tool_key)}</strong>
                </span>
                <small>
                  {invocation.protocol} · {invocationStatusLabel(invocation)} · {invocation.elapsed_ms} ms
                </small>
              </summary>
              <div className="agent-run-tool-payload">
                <label>工具</label>
                <code>{invocation.tool_key}</code>
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
