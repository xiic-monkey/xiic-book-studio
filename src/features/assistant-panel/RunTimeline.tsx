import { useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import type { ActionProposal, AgentRunSummary, RunEvent, ToolInvocation } from "../../types";
import {
  AssistantNarrationRow,
  AssistantPlanRow,
  AssistantProposalRow,
  AssistantReasoningRow,
  AssistantSubagentRow,
  AssistantToolGroupRow,
  AssistantToolRow,
  assistantToolGlyph,
} from "../agent-runs/ConversationTimelineRows";
import { proposalTypeLabel, toolLabel } from "../agent-runs/toolLabels";
import type { AssistantFeedItem, AssistantToolItem } from "./timeline";
import {
  childRunState,
  compactAssistantOutput,
  findInvocation,
  planProgressLabel,
  toolGroupCountsLabel,
  toolMetaLabel,
} from "./timeline";

/*
 * 运行控制台时间线：把 RunEvent 派生的条目渲染成原生 harness 的执行流。
 * 工具载荷（参数/结果）不在事件里，展开某一行时才按 run 懒取，所以这里只依赖 summaries 缓存。
 * 子任务默认就地展开，"单独查看"仍可进入全屏子会话视图。
 */

type RunTimelineProps = {
  items: AssistantFeedItem[];
  summaries: Record<number, AgentRunSummary>;
  onRequestPayloads: (runId: number) => void;
  /** 子会话内联视图：只传该 run 自己的条目，长输出折叠、正文不再当主回答。 */
  variant?: "main" | "child";
  childItems?: Record<number, AssistantFeedItem[]>;
  childEvents?: Record<number, RunEvent[]>;
  onOpenChild?: (runId: number) => void;
  onCancelChild?: (runId: number) => void;
  cancellingChildRunIds?: number[];
  onApplyProposal?: (proposal: ActionProposal) => void;
  onRejectProposal?: (proposal: ActionProposal) => void;
  actionBusy?: boolean;
  lastOutputItemId?: string | null;
  liveOutputItemId?: string | null;
  onOpenCandidate?: () => void;
  candidateLabel?: string;
};

function toolSections(invocation: ToolInvocation | undefined) {
  if (!invocation) return undefined;
  return [
    { label: "参数", text: JSON.stringify(invocation.arguments, null, 2) },
    {
      label: invocation.error ? "错误" : "结果",
      text: invocation.error ?? JSON.stringify(invocation.result, null, 2),
    },
  ];
}

type Props = Pick<RunTimelineProps, "summaries" | "onRequestPayloads" | "childItems" | "childEvents"
  | "onOpenChild" | "onCancelChild" | "cancellingChildRunIds" | "onApplyProposal" | "onRejectProposal" | "actionBusy">;

function ToolRow({
  tool,
  summaries,
  onRequestPayloads,
}: {
  tool: AssistantToolItem;
  summaries: RunTimelineProps["summaries"];
  onRequestPayloads: (runId: number) => void;
}) {
  const invocation = summaries[tool.runId] ? findInvocation(tool, summaries[tool.runId].tool_invocations) : undefined;
  return (
    <AssistantToolRow
      icon={assistantToolGlyph(tool.toolKey)}
      label={toolLabel(tool.toolKey)}
      status={tool.status}
      summary={tool.summary}
      meta={toolMetaLabel(tool, invocation)}
      sections={toolSections(invocation)}
      onExpand={() => onRequestPayloads(tool.runId)}
    />
  );
}

function ChildOutputBlock({ content }: { content: string }) {
  if (content.trim().length > 240 || /[\[{]/.test(content.trim())) {
    return (
      <details className="assistant-subagent-output-panel">
        <summary><span>查看输出</span><small>{content.trim().length} 字</small></summary>
        <pre>{content}</pre>
      </details>
    );
  }
  return <p className="assistant-subagent-output">{content}</p>;
}

function TimelineRows({ items, isMainRun, ...props }: {
  items: AssistantFeedItem[];
  isMainRun: boolean;
} & Props & Pick<RunTimelineProps, "lastOutputItemId" | "liveOutputItemId" | "onOpenCandidate" | "candidateLabel">) {
  const [childOverrides, setChildOverrides] = useState<Record<number, boolean>>({});
  const { summaries, onRequestPayloads } = props;

  const renderItem = (item: AssistantFeedItem): ReactNode => {
    switch (item.kind) {
      case "user":
        return (
          <article className="assistant-message assistant-message-user" key={item.id}>
            <div className="assistant-message-body"><p>{item.content}</p></div>
          </article>
        );
      case "memory_warning":
        return (
          <article className="assistant-memory-warning" key={item.id}>
            <div>
              <strong>章节交接记忆生成失败</strong>
              <p>{item.content}</p>
              <small>写下一章时会自动重试；也可让子 Agent 重新生成。</small>
            </div>
          </article>
        );
      case "narration":
        return <AssistantNarrationRow key={item.id} content={item.content} tone={item.tone} />;
      case "plan":
        return <AssistantPlanRow key={item.id} tasks={item.tasks} progressLabel={planProgressLabel(item.tasks)} />;
      case "thinking":
        return (
          <AssistantReasoningRow
            key={item.id}
            content={item.content}
            streaming={item.active}
            startedAtMs={item.startedAtMs}
            endedAtMs={item.endedAtMs}
          />
        );
      case "tool":
        return <ToolRow key={item.id} tool={item} summaries={summaries} onRequestPayloads={onRequestPayloads} />;
      case "tool_group":
        return (
          <AssistantToolGroupRow key={item.id} countsLabel={toolGroupCountsLabel(item.counts)}>
            {item.tools.map((tool) => (
              <ToolRow key={tool.id} tool={tool} summaries={summaries} onRequestPayloads={onRequestPayloads} />
            ))}
          </AssistantToolGroupRow>
        );
      case "proposal":
        return (
          <AssistantProposalRow
            key={item.id}
            summary={item.proposal.summary}
            typeLabel={proposalTypeLabel(item.proposal.proposal_type)}
            busy={props.actionBusy ?? false}
            onApply={() => props.onApplyProposal?.(item.proposal)}
            onReject={() => props.onRejectProposal?.(item.proposal)}
          />
        );
      case "subagent": {
        const state = childRunState(props.childEvents?.[item.runId] ?? []);
        const children = props.childItems?.[item.runId] ?? [];
        const expanded = childOverrides[item.runId] ?? state.running;
        return (
          <AssistantSubagentRow
            key={item.id}
            title={item.title}
            statusLabel={state.statusLabel}
            running={state.running}
            expanded={expanded}
            onToggle={() => setChildOverrides((current) => ({ ...current, [item.runId]: !expanded }))}
            onOpen={() => props.onOpenChild?.(item.runId)}
            onCancel={state.running ? () => props.onCancelChild?.(item.runId) : undefined}
            cancelling={(props.cancellingChildRunIds ?? []).includes(item.runId)}
            busy={props.actionBusy ?? false}
          >
            {expanded && children.length > 0 && (
              <TimelineRows
                items={children}
                isMainRun={false}
                summaries={summaries}
                onRequestPayloads={onRequestPayloads}
                childItems={props.childItems}
                childEvents={props.childEvents}
                onOpenChild={props.onOpenChild}
                onCancelChild={props.onCancelChild}
                cancellingChildRunIds={props.cancellingChildRunIds}
                onApplyProposal={props.onApplyProposal}
                onRejectProposal={props.onRejectProposal}
                actionBusy={props.actionBusy}
              />
            )}
          </AssistantSubagentRow>
        );
      }
      case "output": {
        const text = compactAssistantOutput(item.content);
        if (!isMainRun) return <ChildOutputBlock key={item.id} content={text} />;
        const isLiveOutput = item.id === props.liveOutputItemId;
        return (
          <article
            className={`assistant-message assistant-message-agent${isLiveOutput ? " assistant-output-live" : ""}`}
            key={item.id}
          >
            <div className="assistant-message-body">
              {/* 对齐原生 harness：正文全文直出，不做"查看全文"折叠。 */}
              <p>{text}</p>
              {!isLiveOutput && item.id === props.lastOutputItemId && props.onOpenCandidate && (
                <button type="button" className="assistant-output-link" onClick={props.onOpenCandidate}>
                  {props.candidateLabel ?? "打开候选稿"}
                  <ChevronRight size={12} />
                </button>
              )}
            </div>
          </article>
        );
      }
      default:
        return null;
    }
  };

  return <>{items.map(renderItem)}</>;
}

export function RunTimeline({ items, variant = "main", ...props }: RunTimelineProps) {
  return <TimelineRows items={items} isMainRun={variant === "main"} {...props} />;
}
