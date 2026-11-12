import type { ActionProposal, RunEvent, ToolInvocation } from "../../types";

/*
 * Agent 面板时间线的纯逻辑：RunEvent 流 -> 可视化条目。
 * 无 React、无 IO：面板状态机的一半在这里，可以脱离渲染被直接测试。
 * 条目只描述"发生过什么"，载荷（参数/结果）由渲染层按 run 懒取。
 */

export type AssistantToolStatus = "running" | "success" | "failed" | "rejected";
export type AssistantToolGroupCounts = { label: string; count: number };
export type AssistantPlanStatus =
  | "pending"
  | "running"
  | "success"
  | "failed"
  | "skipped"
  | "cancelled";

export type AssistantPlanTask = {
  index: number;
  title: string;
  taskType: string;
  chapterId: number | null;
  dependsOn: number[];
  status: AssistantPlanStatus;
};

export type AssistantToolItem = {
  kind: "tool";
  id: string;
  runId: number;
  toolKey: string;
  status: AssistantToolStatus;
  summary?: string;
  invocationId?: number | null;
  elapsedMs?: number | null;
  ordinal: number;
  sequence: number;
  order: number;
};

export type AssistantTimelineItem =
  | { kind: "thinking"; id: string; runId: number; content: string; active: boolean; startedAtMs?: number; endedAtMs?: number | null; sequence: number; order: number }
  | AssistantToolItem
  | { kind: "tool_group"; id: string; runId: number; counts: AssistantToolGroupCounts[]; tools: AssistantToolItem[]; sequence: number; order: number }
  | { kind: "output"; id: string; runId: number; content: string; sequence: number; order: number }
  | { kind: "narration"; id: string; runId: number; content: string; tone: "info" | "warning"; sequence: number; order: number }
  | { kind: "plan"; id: string; runId: number; tasks: AssistantPlanTask[]; sequence: number; order: number }
  | { kind: "subagent"; id: string; runId: number; title: string; agentKey?: string | null; status: string; startedAtMs?: number; sequence: number; order: number }
  | { kind: "proposal"; id: string; runId: number; proposal: ActionProposal; sequence: number; order: number };

/** 已落库的会话消息：与运行事件共用 order 排序，但不是 RunEvent 派生的。 */
export type AssistantChatItem =
  | { kind: "user"; id: string; content: string; order: number }
  | { kind: "output"; id: string; content: string; order: number }
  | { kind: "memory_warning"; id: string; content: string; order: number };

export type AssistantFeedItem = AssistantTimelineItem | AssistantChatItem;

const TERMINAL_KINDS = ["completed", "failed", "cancelled"];
/** 落库的终态播报（父 run 的最终答复）状态：这些文本同时已作为 assistant 消息呈现，不再入时间线。 */
const TERMINAL_OUTPUT_STATUSES = ["success", "completed", "failed", "cancelled"];

/**
 * 编排事件按 kind 白名单入时间线（过去整条被丢弃，用户只能看到"卡住"）：
 * - 终态 output_delta 不渲染：该文本已由 insert_message 落库为 assistant 消息，渲染会重复。
 * - 过程 output_delta（status=running）渲染为次要播报行。
 */
function handleOrchestratorEvent(
  event: RunEvent,
  push: (item: AssistantTimelineItem) => void,
) {
  if (event.kind === "plan_updated") {
    const tasks = parsePlanSnapshot(event.delta);
    if (tasks) push(planItem(event, tasks));
    else console.warn("plan_updated 载荷无法解析，已丢弃该快照", event.delta);
    return true;
  }
  if (event.kind === "proposal_warning") {
    push({ kind: "narration", id: `narration-${event.run_id}-${event.sequence}`, runId: event.run_id, content: event.delta, tone: "warning", sequence: event.sequence, order: assistantEventOrder(event) });
    return true;
  }
  if (event.kind === "output_delta") {
    if (TERMINAL_OUTPUT_STATUSES.includes(event.status) || !event.delta.trim()) return true;
    push({ kind: "narration", id: `narration-${event.run_id}-${event.sequence}`, runId: event.run_id, content: event.delta.trim(), tone: "info", sequence: event.sequence, order: assistantEventOrder(event) });
    return true;
  }
  if (event.kind === "started" || TERMINAL_KINDS.includes(event.kind)) return true;
  return false;
}

function planItem(event: RunEvent, tasks: AssistantPlanTask[]): AssistantTimelineItem {
  return {
    kind: "plan",
    id: `plan-${event.run_id}`,
    runId: event.run_id,
    tasks,
    sequence: event.sequence,
    order: assistantEventOrder(event),
  };
}

/** `plan_updated` 载荷解析；畸形载荷返回 null，由调用方丢弃该事件而不是清空计划卡。 */
export function parsePlanSnapshot(delta: string): AssistantPlanTask[] | null {
  const text = delta.trim();
  if (!text.startsWith("{")) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  const rawTasks = (parsed as { tasks?: unknown }).tasks;
  if (!Array.isArray(rawTasks)) return null;
  const tasks = rawTasks.flatMap((entry): AssistantPlanTask[] => {
    if (typeof entry !== "object" || entry === null) return [];
    const task = entry as Record<string, unknown>;
    if (typeof task.index !== "number" || typeof task.title !== "string") return [];
    return [{
      index: task.index,
      title: task.title,
      taskType: typeof task.task_type === "string" ? task.task_type : "task",
      chapterId: typeof task.chapter_id === "number" ? task.chapter_id : null,
      dependsOn: Array.isArray(task.depends_on) ? task.depends_on.filter((id): id is number => typeof id === "number") : [],
      status: isPlanStatus(task.status) ? task.status : "pending",
    }];
  });
  return tasks.length > 0 ? tasks.sort((left, right) => left.index - right.index) : null;
}

function isPlanStatus(value: unknown): value is AssistantPlanStatus {
  return value === "pending" || value === "running" || value === "success"
    || value === "failed" || value === "skipped" || value === "cancelled";
}

/** 计划进度文案："2/3 完成"，失败与跳过单独计数以便一眼看出问题。 */
export function planProgressLabel(tasks: AssistantPlanTask[]): string {
  const done = tasks.filter((task) => task.status === "success").length;
  const broken = tasks.filter((task) => task.status === "failed" || task.status === "skipped").length;
  const base = `${done}/${tasks.length} 完成`;
  return broken > 0 ? `${base} · ${broken} 异常` : base;
}

/** 运行状态标签：面板头部与子任务行共用。 */
export function runStatusLabel(status: string) {
  if (status === "success") return "已完成";
  if (status === "failed") return "失败";
  if (status === "cancelled") return "已停止";
  if (["running", "streaming", "cancellation_requested"].includes(status)) return "执行中";
  return status;
}

/** 子 run 的执行状态：事件流最后一条即最新状态（没有事件时视为刚开始）。 */
export function childRunState(events: RunEvent[]): { status: string; running: boolean; statusLabel: string } {
  const status = events[events.length - 1]?.status ?? "running";
  const running = !["success", "failed", "cancelled"].includes(status);
  return { status, running, statusLabel: runStatusLabel(status) };
}

/** 委托运行的收尾语折叠成一行短状态，正文留在子任务详情里。 */
export function compactAssistantOutput(content: string) {
  const trimmed = content.trim();
  const completion = trimmed.match(/^(已完成\s+\d+\s+个子任务)[。.!！]?/);
  if (!completion) return content;

  const remainder = trimmed.slice(completion[0].length).trim();
  let label = remainder.split(/[:：]/, 1)[0]?.trim() ?? "";
  if (label.length > 40) label = `${label.slice(0, 40)}…`;
  return label ? `${completion[1]} · ${label}` : completion[1];
}

const assistantEventOrderCache = new WeakMap<RunEvent, number>();

export function timestampMs(event: RunEvent) {
  const timestamp = Date.parse(event.created_at);
  return Number.isFinite(timestamp) ? timestamp : undefined;
}

export function assistantEventOrder(event: RunEvent) {
  // 排序比较器会反复调用：Date.parse 每个事件只算一次。
  const cached = assistantEventOrderCache.get(event);
  if (cached !== undefined) return cached;
  const timestamp = Date.parse(event.created_at);
  const order = (Number.isFinite(timestamp) ? timestamp : 0) * 1000 + event.sequence;
  assistantEventOrderCache.set(event, order);
  return order;
}

function toolStatusOf(event: RunEvent): AssistantToolStatus {
  if (event.status === "success") return "success";
  if (event.status === "rejected") return "rejected";
  if (["running", "started", "pending"].includes(event.status)) return "running";
  return "failed";
}

/**
 * 事件流 -> 时间线条目。思考栈、工具栈与输出尾指针都按 run 分桶：
 * 主 Agent 与子 Agent 的事件混在一个列表里到达，全局栈会把两个 run 的过程串在一起。
 */
export function buildAssistantTimeline(events: RunEvent[]): AssistantTimelineItem[] {
  const sorted = [...events].sort((a, b) => a.run_id - b.run_id || a.sequence - b.sequence);
  const items: AssistantTimelineItem[] = [];
  const openThinking = new Map<number, Extract<AssistantTimelineItem, { kind: "thinking" }>[]>();
  const openTools = new Map<number, Map<string, AssistantToolItem>>();
  const toolOrdinals = new Map<number, number>();
  const lastOutput = new Map<number, Extract<AssistantTimelineItem, { kind: "output" }>>();
  const planItems = new Map<number, Extract<AssistantTimelineItem, { kind: "plan" }>>();

  for (const event of sorted) {
    if (event.parent_run_id && event.kind === "started") {
      items.push({
        kind: "subagent",
        id: `subagent-${event.run_id}`,
        runId: event.run_id,
        title: event.task_title || event.agent_role || event.agent_key || "子 Agent",
        agentKey: event.agent_key,
        status: event.status,
        startedAtMs: timestampMs(event),
        sequence: event.sequence,
        order: assistantEventOrder(event),
      });
      continue;
    }

    if (event.stage === "orchestrator" && handleOrchestratorEvent(event, (item) => {
      if (item.kind === "plan") {
        // 计划是整体快照：原地替换最早那一条，卡片不会随每次跃迁在时间线里重复出现。
        const existing = planItems.get(event.run_id);
        if (existing) {
          existing.tasks = item.tasks;
          return;
        }
        planItems.set(event.run_id, item);
      }
      items.push(item);
    })) {
      continue;
    }

    if (event.kind === "thinking_start") {
      const item: Extract<AssistantTimelineItem, { kind: "thinking" }> = {
        kind: "thinking",
        id: `thinking-${event.sequence}`,
        runId: event.run_id,
        content: "",
        active: true,
        startedAtMs: timestampMs(event),
        sequence: event.sequence,
        order: assistantEventOrder(event),
      };
      items.push(item);
      openThinking.set(event.run_id, [...(openThinking.get(event.run_id) ?? []), item]);
      continue;
    }
    if (event.kind === "thinking_delta") {
      const rounds = openThinking.get(event.run_id) ?? [];
      const current = rounds[rounds.length - 1];
      if (current && current.active) {
        current.content += event.delta;
        continue;
      }
      // start 缺失时兜底，避免逐 token 内容被丢弃。
      const item: Extract<AssistantTimelineItem, { kind: "thinking" }> = {
        kind: "thinking",
        id: `thinking-${event.sequence}`,
        runId: event.run_id,
        content: event.delta,
        active: true,
        startedAtMs: timestampMs(event),
        sequence: event.sequence,
        order: assistantEventOrder(event),
      };
      items.push(item);
      openThinking.set(event.run_id, [...rounds, item]);
      continue;
    }
    if (event.kind === "thinking_end") {
      const rounds = openThinking.get(event.run_id) ?? [];
      const current = rounds.pop();
      openThinking.set(event.run_id, rounds);
      if (current) {
        current.active = false;
        current.endedAtMs = timestampMs(event);
      }
      continue;
    }
    if (event.kind === "tool_started" && event.tool_key) {
      const ordinal = (toolOrdinals.get(event.run_id) ?? 0) + 1;
      toolOrdinals.set(event.run_id, ordinal);
      const item: AssistantToolItem = {
        kind: "tool",
        id: `tool-${event.sequence}`,
        runId: event.run_id,
        toolKey: event.tool_key,
        status: "running",
        invocationId: event.tool_invocation_id,
        ordinal,
        sequence: event.sequence,
        order: assistantEventOrder(event),
      };
      items.push(item);
      const byKey = openTools.get(event.run_id) ?? new Map<string, AssistantToolItem>();
      byKey.set(event.tool_key, item);
      openTools.set(event.run_id, byKey);
      continue;
    }
    if (event.kind === "tool_completed" && event.tool_key) {
      const byKey = openTools.get(event.run_id);
      const current = byKey?.get(event.tool_key);
      if (current && byKey) {
        current.status = toolStatusOf(event);
        current.summary = event.delta || event.error || undefined;
        current.elapsedMs = event.elapsed_ms;
        if (event.tool_invocation_id != null) current.invocationId = event.tool_invocation_id;
        byKey.delete(event.tool_key);
      } else {
        // 只有完成事件的历史运行（started 缺失）也要成行，否则工具调用总数与详情对不上。
        const ordinal = (toolOrdinals.get(event.run_id) ?? 0) + 1;
        toolOrdinals.set(event.run_id, ordinal);
        items.push({
          kind: "tool",
          id: `tool-${event.sequence}`,
          runId: event.run_id,
          toolKey: event.tool_key,
          status: toolStatusOf(event),
          summary: event.delta || event.error || undefined,
          invocationId: event.tool_invocation_id,
          elapsedMs: event.elapsed_ms,
          ordinal,
          sequence: event.sequence,
          order: assistantEventOrder(event),
        });
      }
      continue;
    }
    if (event.kind === "output_delta" && event.delta) {
      const previous = lastOutput.get(event.run_id);
      if (previous && items[items.length - 1] === previous) {
        previous.content += event.delta;
        previous.sequence = event.sequence;
        previous.order = assistantEventOrder(event);
      } else {
        const item: Extract<AssistantTimelineItem, { kind: "output" }> = {
          kind: "output",
          id: `output-${event.run_id}-${event.sequence}`,
          runId: event.run_id,
          content: event.delta,
          sequence: event.sequence,
          order: assistantEventOrder(event),
        };
        items.push(item);
        lastOutput.set(event.run_id, item);
      }
      continue;
    }
    // 未知 kind 与 output_reset 一律忽略：时间线不能因为后端加了新事件类型而崩或清空。
  }
  return items.filter(
    (item) => item.kind !== "thinking" || item.active || item.content.trim().length > 0,
  );
}

/**
 * 子 run 的过程条目：剔除它自己的 started 事件。
 * 那条事件在主时间线里已经渲染成子任务行，在子会话里再出现一次就是重复。
 */
export function buildChildTimeline(events: RunEvent[], runId: number): AssistantTimelineItem[] {
  return buildAssistantTimeline(
    events.filter((event) => !(event.kind === "started" && event.run_id === runId)),
  );
}

// 查阅类（检索/读取/检查）工具归入聚合行；写入类保持独立以保证可见性。
const READ_TOOL_KIND_ORDER = ["检索", "读取", "检查"];

export function readToolKindLabel(toolKey: string): string | null {
  if (/search|history_context/.test(toolKey)) return "检索";
  if (/^(get_|prepare)|read|reference|memory|bible/.test(toolKey)) return "读取";
  if (/check|continuity|quality|analysis/.test(toolKey)) return "检查";
  return null;
}

export function toolGroupCountsLabel(counts: AssistantToolGroupCounts[]) {
  return counts.map((entry) => `${entry.count} ${entry.label}`).join(", ");
}

/** 连续的成功查阅类工具收成一行"查阅 · N 检索, M 读取"；跨 run 不合并（载荷按 run 懒取）。 */
export function groupReadToolItems(items: AssistantTimelineItem[]): AssistantTimelineItem[] {
  const result: AssistantTimelineItem[] = [];
  let run: AssistantToolItem[] = [];
  const flush = () => {
    if (run.length >= 2) {
      const counts = new Map<string, number>();
      for (const tool of run) {
        const label = readToolKindLabel(tool.toolKey) ?? "读取";
        counts.set(label, (counts.get(label) ?? 0) + 1);
      }
      result.push({
        kind: "tool_group",
        id: `tool-group-${run[0].id}`,
        runId: run[0].runId,
        counts: READ_TOOL_KIND_ORDER
          .filter((label) => counts.has(label))
          .map((label) => ({ label, count: counts.get(label) as number })),
        tools: run,
        sequence: run[0].sequence,
        order: run[0].order,
      });
    } else if (run.length === 1) {
      result.push(run[0]);
    }
    run = [];
  };
  for (const item of items) {
    if (item.kind === "tool" && item.status === "success" && readToolKindLabel(item.toolKey) != null) {
      if (run.length > 0 && run[0].runId !== item.runId) flush();
      run.push(item);
      continue;
    }
    flush();
    result.push(item);
  }
  flush();
  return result;
}

/**
 * 待确认提议紧跟在产生它的那一行之后：写入类工具的结果是"等你拍板"，
 * 摆在时间线里才谈得上执行闭环；抽屉里的完整列表仍然保留。
 * 未挂在任何可见行上的提议不在这里出现（由抽屉与待确认计数兜底）。
 */
export function attachPendingProposals(
  items: AssistantTimelineItem[],
  proposals: ActionProposal[],
): AssistantTimelineItem[] {
  const byRun = new Map<number, ActionProposal[]>();
  for (const proposal of proposals) {
    if (proposal.status !== "pending" || proposal.source_run_id == null) continue;
    byRun.set(proposal.source_run_id, [...(byRun.get(proposal.source_run_id) ?? []), proposal]);
  }
  if (byRun.size === 0) return items;
  const lastIndexOfRun = new Map<number, number>();
  items.forEach((item, index) => lastIndexOfRun.set(item.runId, index));
  const result: AssistantTimelineItem[] = [];
  items.forEach((item, index) => {
    result.push(item);
    if (lastIndexOfRun.get(item.runId) !== index) return;
    for (const proposal of byRun.get(item.runId) ?? []) {
      result.push({
        kind: "proposal",
        id: `proposal-${proposal.id}`,
        runId: item.runId,
        proposal,
        sequence: item.sequence,
        // +0.5 让提议在整表按 order 重排后仍紧贴宿主行。
        order: item.order + 0.5,
      });
    }
  });
  return result;
}

/**
 * 工具条目与落库调用的对应关系：优先主键，其次按 (run, tool_key, 该键内的第 n 次出现) 兜底
 * ——历史事件里 tool_invocation_id 可能为空。
 */
export function findInvocation(
  tool: { runId: number; toolKey: string; ordinal: number; invocationId?: number | null },
  invocations: ToolInvocation[],
): ToolInvocation | undefined {
  if (tool.invocationId != null) {
    const byId = invocations.find((invocation) => invocation.id === tool.invocationId);
    if (byId) return byId;
  }
  const sameKey = invocations.filter(
    (invocation) => invocation.tool_key === tool.toolKey
      && (invocation.run_id ?? 0) === tool.runId,
  );
  return sameKey[tool.ordinal - 1] ?? sameKey[sameKey.length - 1];
}

/**
 * 用运行摘要补齐事件流缺失的过程行：历史运行的 tool_invocations 是权威数据，
 * 事件里可能没有 started/completed 对；正文也需在无 output_delta 时补一行。
 */
export function backfillTimelineFromSummary(
  items: AssistantTimelineItem[],
  summary: { run: { id: number; output: string; created_at: string }; tool_invocations: ToolInvocation[] } | undefined,
): AssistantTimelineItem[] {
  if (!summary) return items;
  const next = [...items];
  const represented = new Set(
    next
      .filter((item): item is AssistantToolItem => item.kind === "tool" && item.invocationId != null)
      .map((item) => item.invocationId as number),
  );
  let ordinal = next.filter((item) => item.kind === "tool").length;
  for (const invocation of summary.tool_invocations) {
    if (represented.has(invocation.id)) continue;
    ordinal += 1;
    next.push({
      kind: "tool",
      id: `invocation-${invocation.id}`,
      runId: invocation.run_id ?? summary.run.id,
      ordinal,
      toolKey: invocation.tool_key,
      status: invocation.error ? "failed"
        : invocation.status === "success" ? "success"
          : invocation.status === "rejected" ? "rejected"
            : ["running", "started", "pending"].includes(invocation.status) ? "running" : "failed",
      invocationId: invocation.id,
      elapsedMs: invocation.elapsed_ms,
      sequence: 0,
      order: orderOf(invocation.created_at),
    });
  }
  const runId = summary.run.id;
  if (!next.some((item) => item.kind === "output" && item.runId === runId) && summary.run.output.trim()) {
    next.push({
      kind: "output",
      id: `run-output-${runId}`,
      runId,
      content: summary.run.output,
      sequence: 0,
      order: orderOf(summary.run.created_at) + 1,
    });
  }
  return next.sort((left, right) => left.order - right.order || left.sequence - right.sequence);
}

function orderOf(timestamp: string) {
  const parsed = Date.parse(timestamp);
  return (Number.isFinite(parsed) ? parsed : 0) * 1000;
}

/** 展开区元信息：#序号 · 耗时 · 协议。缺项自动省略，不留空分隔符。 */
export function toolMetaLabel(tool: { ordinal: number; elapsedMs?: number | null }, invocation?: ToolInvocation) {
  const parts = [`#${tool.ordinal}`];
  const elapsed = tool.elapsedMs ?? invocation?.elapsed_ms;
  if (elapsed != null && elapsed >= 0) parts.push(`${elapsed.toLocaleString()} ms`);
  if (invocation?.protocol) parts.push(invocation.protocol);
  return parts.join(" · ");
}

export function runEventKey(event: RunEvent) {
  return `${event.run_id}:${event.sequence}`;
}

export function mergeRunEvents(current: RunEvent[], incoming: RunEvent[]) {
  const merged = new Map(current.map((event) => [runEventKey(event), event]));
  incoming.forEach((event) => merged.set(runEventKey(event), event));
  return [...merged.values()].sort((left, right) => {
    const orderDelta = assistantEventOrder(left) - assistantEventOrder(right);
    return orderDelta || left.run_id - right.run_id || left.sequence - right.sequence;
  });
}
