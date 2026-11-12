import { describe, expect, it } from "vitest";
import type { ActionProposal, RunEvent, ToolInvocation } from "../../types";
import type { AssistantTimelineItem, AssistantToolItem } from "./timeline";
import {
  attachPendingProposals,
  backfillTimelineFromSummary,
  buildAssistantTimeline,
  buildChildTimeline,
  childRunState,
  compactAssistantOutput,
  findInvocation,
  groupReadToolItems,
  mergeRunEvents,
  parsePlanSnapshot,
  planProgressLabel,
  readToolKindLabel,
  toolGroupCountsLabel,
  toolMetaLabel,
} from "./timeline";

const BASE_TIME = Date.parse("2026-10-31T10:00:00.000Z");

function event(overrides: Partial<RunEvent> & { kind: RunEvent["kind"] }): RunEvent {
  return {
    run_id: 1,
    project_id: 7,
    chapter_id: 3,
    stage: "orchestrator",
    sequence: 1,
    delta: "",
    status: "running",
    error: null,
    tool_key: null,
    tool_invocation_id: null,
    elapsed_ms: null,
    created_at: new Date(BASE_TIME).toISOString(),
    parent_run_id: null,
    agent_key: null,
    agent_role: null,
    task_title: null,
    ...overrides,
  };
}

function planDelta(entries: Array<Partial<{ index: number; title: string; type: string; status: string; depends_on: number[] }>>) {
  return JSON.stringify({
    tasks: entries.map((entry, fallbackIndex) => ({
      index: entry.index ?? fallbackIndex,
      title: entry.title ?? `任务 ${fallbackIndex}`,
      task_type: entry.type ?? "draft",
      chapter_id: 3,
      depends_on: entry.depends_on ?? [],
      status: entry.status ?? "pending",
    })),
  });
}

describe("buildAssistantTimeline 的编排事件白名单", () => {
  it("把 plan_updated 收成一张计划卡，后续快照原地替换而不是新增行", () => {
    const items = buildAssistantTimeline([
      event({ kind: "started", sequence: 1 }),
      event({ kind: "plan_updated", sequence: 2, delta: planDelta([{ title: "写第三章" }, { title: "试读", type: "review", depends_on: [0] }]) }),
      event({ kind: "plan_updated", sequence: 5, delta: planDelta([{ title: "写第三章", status: "running" }, { title: "试读", type: "review", depends_on: [0] }]) }),
    ]);
    const plans = items.filter((item) => item.kind === "plan");
    expect(plans).toHaveLength(1);
    expect(plans[0]?.kind === "plan" && plans[0].tasks.map((task) => task.status)).toEqual(["running", "pending"]);
    expect(plans[0]?.kind === "plan" && plans[0].tasks[1].dependsOn).toEqual([0]);
  });

  it("过程播报入时间线，终态播报不入（终态文本已由消息通道呈现）", () => {
    const items = buildAssistantTimeline([
      event({ kind: "output_delta", sequence: 2, delta: "我已识别到需要执行的任务，正在委托专业 Agent…", status: "running" }),
      event({ kind: "output_delta", sequence: 3, delta: "无法启动“试读”：缺少来源产物", status: "running" }),
      event({ kind: "output_delta", sequence: 9, delta: "已完成 1 个子任务。", status: "success" }),
    ]);
    const narration = items.filter((item) => item.kind === "narration");
    expect(narration.map((item) => item.kind === "narration" && item.content)).toEqual([
      "我已识别到需要执行的任务，正在委托专业 Agent…",
      "无法启动“试读”：缺少来源产物",
    ]);
  });

  it("proposal_warning 渲染为警示播报", () => {
    const items = buildAssistantTimeline([
      event({ kind: "proposal_warning", sequence: 4, delta: "两条提议因资料缺失被跳过" }),
    ]);
    expect(items[0]).toMatchObject({ kind: "narration", tone: "warning" });
  });

  it("畸形计划载荷被丢弃，其余事件照常成行", () => {
    const items = buildAssistantTimeline([
      event({ kind: "plan_updated", sequence: 2, delta: "{不是 JSON" }),
      event({ kind: "output_delta", sequence: 3, delta: "委托播报", status: "running" }),
    ]);
    expect(items.map((item) => item.kind)).toEqual(["narration"]);
  });

  it("未知事件类型不影响时间线", () => {
    const items = buildAssistantTimeline([
      event({ kind: "usage_updated" as RunEvent["kind"], sequence: 2, delta: "{}" }),
      event({ kind: "output_delta", sequence: 3, delta: "委托播报", status: "running" }),
    ]);
    expect(items.map((item) => item.kind)).toEqual(["narration"]);
  });
});

describe("buildChildTimeline", () => {
  it("剔除子 run 自己的 started 事件（主时间线里已渲染成子任务行）", () => {
    const events = [
      event({ run_id: 9, parent_run_id: 1, kind: "started", sequence: 1, task_title: "重写结尾" }),
      event({ run_id: 9, parent_run_id: 1, kind: "tool_started", sequence: 2, tool_key: "replace_text" }),
    ];
    expect(buildAssistantTimeline(events).map((item) => item.kind)).toEqual(["subagent", "tool"]);
    expect(buildChildTimeline(events, 9).map((item) => item.kind)).toEqual(["tool"]);
  });
});

describe("buildAssistantTimeline 的过程行", () => {
  it("思考与工具按 run 分桶，主 Agent 与子 Agent 不串台", () => {
    const items = buildAssistantTimeline([
      event({ run_id: 1, stage: "orchestrator", kind: "thinking_start", sequence: 10 }),
      event({ run_id: 2, stage: "draft", parent_run_id: 1, kind: "thinking_start", sequence: 1 }),
      event({ run_id: 2, stage: "draft", parent_run_id: 1, kind: "thinking_delta", sequence: 2, delta: "子任务在想" }),
      event({ run_id: 1, stage: "orchestrator", kind: "thinking_delta", sequence: 11, delta: "主 Agent 在想" }),
    ]);
    const thinking = items.filter((item) => item.kind === "thinking");
    expect(thinking.find((item) => item.runId === 1)?.kind === "thinking" && thinking.find((item) => item.runId === 1)!.content).toBe("主 Agent 在想");
    expect(thinking.find((item) => item.runId === 2)?.kind === "thinking" && thinking.find((item) => item.runId === 2)!.content).toBe("子任务在想");
  });

  it("工具行带耗时与序号，终态事件补出缺失的 started", () => {
    const items = buildAssistantTimeline([
      event({ run_id: 4, stage: "draft", kind: "tool_started", sequence: 1, tool_key: "search_story" }),
      event({ run_id: 4, stage: "draft", kind: "tool_completed", sequence: 2, tool_key: "search_story", status: "success", delta: "命中 6 条", elapsed_ms: 812 }),
      event({ run_id: 4, stage: "draft", kind: "tool_completed", sequence: 3, tool_key: "continuity_check", status: "failed", error: "缺少前情", elapsed_ms: 44 }),
    ]);
    const tools = items.filter((item) => item.kind === "tool");
    expect(tools.map((item) => item.kind === "tool" && item.ordinal)).toEqual([1, 2]);
    expect(tools[0]).toMatchObject({ status: "success", elapsedMs: 812, summary: "命中 6 条" });
    expect(tools[1]).toMatchObject({ status: "failed", summary: "缺少前情", elapsedMs: 44 });
  });

  it("子 Agent 行保留任务标题与开始时间", () => {
    const items = buildAssistantTimeline([
      event({ run_id: 9, stage: "draft", parent_run_id: 1, kind: "started", sequence: 1, task_title: "重写第三章结尾", agent_key: "draft" }),
    ]);
    expect(items[0]).toMatchObject({ kind: "subagent", title: "重写第三章结尾", agentKey: "draft" });
    expect(items[0]?.kind === "subagent" && items[0].startedAtMs).toBe(BASE_TIME);
  });
});

describe("计划与工具的派生文案", () => {
  it("planProgressLabel 单独点出异常任务", () => {
    const tasks = parsePlanSnapshot(planDelta([
      { status: "success" },
      { status: "skipped" },
      { status: "running" },
    ]))!;
    expect(planProgressLabel(tasks)).toBe("1/3 完成 · 1 异常");
  });

  it("toolMetaLabel 拼接序号、耗时与协议，缺项不留空分隔", () => {
    const openai = { protocol: "openai_tools" } as unknown as ToolInvocation;
    expect(toolMetaLabel({ ordinal: 2, elapsedMs: 812 }, openai)).toBe("#2 · 812 ms · openai_tools");
    expect(toolMetaLabel({ ordinal: 1 })).toBe("#1");
  });

  it("findInvocation 优先主键，缺失时按同键第 n 次出现兜底", () => {
    const invocations = [
      { id: 11, run_id: 4, tool_key: "search_story" },
      { id: 12, run_id: 4, tool_key: "search_story" },
    ] as unknown as ToolInvocation[];
    expect(findInvocation({ runId: 4, toolKey: "search_story", ordinal: 2, invocationId: 12 }, invocations)?.id).toBe(12);
    expect(findInvocation({ runId: 4, toolKey: "search_story", ordinal: 2 }, invocations)?.id).toBe(12);
    expect(findInvocation({ runId: 4, toolKey: "search_story", ordinal: 1 }, invocations)?.id).toBe(11);
  });
});

describe("查阅类工具聚合", () => {
  const tool = (runId: number, id: string, ordinal: number) => ({
    kind: "tool" as const, id, runId, ordinal, toolKey: "search_story", status: "success" as const, sequence: 0, order: 0,
  });

  it("跨 run 的连续查阅工具不合并", () => {
    const grouped = groupReadToolItems([tool(4, "a", 1), tool(4, "b", 2), tool(5, "c", 1), tool(5, "d", 2)]);
    expect(grouped.map((item) => item.kind)).toEqual(["tool_group", "tool_group"]);
  });

  it("按工具类别归类在册工具，写入类不归类", () => {
    expect(readToolKindLabel("search_story_facts")).toBe("检索");
    expect(readToolKindLabel("history_context")).toBe("检索");
    expect(readToolKindLabel("web_search")).toBe("检索");
    expect(readToolKindLabel("get_chapter_context")).toBe("读取");
    expect(readToolKindLabel("check_continuity")).toBe("检查");
    expect(readToolKindLabel("quality_analysis")).toBe("检查");
    expect(readToolKindLabel("create_artifact")).toBeNull();
    expect(readToolKindLabel("update_canon_entry")).toBeNull();
  });

  it("连续成功查阅收成一行，失败/运行中/写入类打断聚合", () => {
    const item = (id: string, toolKey: string, status: AssistantToolItem["status"] = "success") => ({
      kind: "tool" as const, id, runId: 1, ordinal: 1, toolKey, status, sequence: 0, order: 0,
    });
    const merged = groupReadToolItems([
      item("t1", "search_story_facts"),
      item("t2", "search_story"),
      item("t3", "get_chapter_context"),
    ]);
    expect(merged).toHaveLength(1);
    if (merged[0]?.kind === "tool_group") {
      expect(merged[0].tools).toHaveLength(3);
      expect(toolGroupCountsLabel(merged[0].counts)).toBe("2 检索, 1 读取");
    }
    const broken = groupReadToolItems([
      item("t1", "search_story"),
      item("t2", "check_continuity", "failed"),
      item("t3", "search_story_facts"),
      item("t4", "search_story", "running"),
      item("t5", "create_artifact"),
    ]);
    expect(broken.filter((entry) => entry.kind === "tool_group")).toHaveLength(0);
    expect(broken).toHaveLength(5);
  });

  it("非工具行不受影响且保持顺序", () => {
    const grouped = groupReadToolItems([
      { kind: "tool", id: "t1", runId: 1, ordinal: 1, toolKey: "search_story", status: "success", sequence: 0, order: 0 },
      { kind: "tool", id: "t2", runId: 1, ordinal: 2, toolKey: "get_story_bible", status: "success", sequence: 0, order: 0 },
      { kind: "output", id: "o1", runId: 1, content: "正文", sequence: 0, order: 1 },
    ] as AssistantTimelineItem[]);
    expect(grouped.map((entry) => entry.kind)).toEqual(["tool_group", "output"]);
  });
});

describe("mergeRunEvents", () => {
  it("同 run 同序号的事件以后到者覆盖，并保持时间顺序", () => {
    const merged = mergeRunEvents(
      [event({ kind: "tool_started", sequence: 2, tool_key: "search_story" })],
      [event({ kind: "tool_started", sequence: 2, tool_key: "search_story", status: "running" }), event({ kind: "thinking_start", sequence: 1 })],
    );
    expect(merged.map((item) => item.sequence)).toEqual([1, 2]);
  });
});

describe("compactAssistantOutput", () => {
  it("folds a delegated task's raw JSON into a short status line", () => {
    const output = '已完成 1 个子任务。第 1 章试读检查：[ { "issue_type": "事实越界" } ]';

    expect(compactAssistantOutput(output)).toBe("已完成 1 个子任务 · 第 1 章试读检查");
  });

  it("keeps excerpt text out of the main conversation even without JSON", () => {
    const output = "已完成 1 个子任务。按试读反馈修订第 1 章候选稿：林默把最后一盘归档带推进修复机。";

    expect(compactAssistantOutput(output)).toBe("已完成 1 个子任务 · 按试读反馈修订第 1 章候选稿");
  });

  it("truncates an overlong task label", () => {
    const output = `已完成 2 个子任务。${"长".repeat(60)}：正文`;

    expect(compactAssistantOutput(output)).toBe(`已完成 2 个子任务 · ${"长".repeat(40)}…`);
  });

  it("keeps outputs without the completion preamble unchanged", () => {
    const output = "第 1 章草稿已生成，请查看候选稿。";

    expect(compactAssistantOutput(output)).toBe(output);
  });
});

describe("childRunState", () => {
  it("没有事件时按执行中处理，最后一条决定状态", () => {
    expect(childRunState([])).toEqual({ status: "running", running: true, statusLabel: "执行中" });
    expect(childRunState([event({ run_id: 8, kind: "started", status: "running" })]).running).toBe(true);
    expect(childRunState([
      event({ run_id: 8, kind: "started", status: "running" }),
      event({ run_id: 8, kind: "completed", status: "success" }),
    ])).toEqual({ status: "success", running: false, statusLabel: "已完成" });
  });
});

describe("backfillTimelineFromSummary", () => {
  const summary = {
    run: { id: 4, output: "章节草稿完成", created_at: "2026-10-31T10:00:05.000Z" },
    tool_invocations: [
      { id: 70, run_id: 4, tool_key: "search_story", status: "success", error: null, elapsed_ms: 20, created_at: "2026-10-31T10:00:01.000Z", arguments: {}, result: {} },
      { id: 71, run_id: 4, tool_key: "replace_text", status: "failed", error: "范围越界", elapsed_ms: 5, created_at: "2026-10-31T10:00:03.000Z", arguments: {}, result: {} },
    ],
  } as unknown as Parameters<typeof backfillTimelineFromSummary>[1];

  it("补齐缺失的工具行与正文，已代表的调用不重复", () => {
    const events = [
      event({ run_id: 4, kind: "tool_started", sequence: 1, tool_key: "search_story", tool_invocation_id: 70 }),
      event({ run_id: 4, kind: "tool_completed", sequence: 2, tool_key: "search_story", tool_invocation_id: 70 }),
    ];
    const items = backfillTimelineFromSummary(buildAssistantTimeline(events), summary);
    expect(items.map((item) => item.kind)).toEqual(["tool", "tool", "output"]);
    const replaced = items[1];
    expect(replaced?.kind === "tool" && replaced.toolKey).toBe("replace_text");
    expect(replaced?.kind === "tool" && replaced.status).toBe("failed");
    expect(replaced?.kind === "tool" && replaced.ordinal).toBe(2);
  });

  it("没有摘要时原样返回", () => {
    const items = buildAssistantTimeline([event({ run_id: 4, kind: "tool_started", sequence: 1, tool_key: "search_story" })]);
    expect(backfillTimelineFromSummary(items, undefined)).toBe(items);
  });
});

describe("attachPendingProposals", () => {
  const tool = (runId: number, sequence: number): AssistantTimelineItem => ({
    kind: "tool", id: `tool-${sequence}`, runId, toolKey: "replace_text",
    status: "success", ordinal: 1, sequence, order: sequence,
  });
  const proposal = (id: number, runId: number | null, status = "pending") => ({
    id, project_id: 7, chapter_id: 3, source_run_id: runId, proposal_type: "chapter_revision",
    summary: `替换第 ${id} 段`, payload: {}, expected_version: null, status, decision_note: "",
    created_at: "2026-10-31T10:00:00.000Z", decided_at: null,
  } as unknown as ActionProposal);

  it("提议紧跟宿主 run 的最后一行，且重排后仍贴附", () => {
    const items = [tool(4, 1), tool(4, 2), tool(5, 3)];
    const result = attachPendingProposals(items, [proposal(9, 4)]);
    expect(result.map((item) => item.kind)).toEqual(["tool", "tool", "proposal", "tool"]);
    expect(result[2]?.order).toBeGreaterThan(result[1]!.order);
    expect(result[2]?.order).toBeLessThan(result[3]!.order);
  });

  it("已决策或非本时间线的提议不入行", () => {
    expect(attachPendingProposals([tool(4, 1)], [proposal(9, 4, "applied")])).toHaveLength(1);
    expect(attachPendingProposals([tool(4, 1)], [proposal(9, null)])).toHaveLength(1);
  });
});

