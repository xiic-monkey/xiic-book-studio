import { describe, expect, it, vi } from "vitest";
import {
  buildChapterAgentPrompt,
  compactAssistantModelName,
  currentPlanStatus,
  formatReviewInstructions,
  foundationStageReadiness,
  isFeedScrolledToBottom,
  isTopLevelStreamingRun,
  partialConfirmationNotice,
  scrollFeedToBottom,
  splitThinkingContent,
} from "./BookStudioWorkspace";

function workspace(overrides: Record<string, unknown> = {}) {
  return {
    story_bible: null,
    story_bible_review: null,
    canonical_fingerprint: "current",
    canon_entries: [],
    ...overrides,
  } as Parameters<typeof currentPlanStatus>[0];
}

describe("isFeedScrolledToBottom", () => {
  it("treats a feed within the threshold as following the bottom", () => {
    expect(isFeedScrolledToBottom({ scrollHeight: 1000, clientHeight: 400, scrollTop: 600 })).toBe(true);
    expect(isFeedScrolledToBottom({ scrollHeight: 1000, clientHeight: 400, scrollTop: 598 })).toBe(true);
  });

  it("stops following once the reader scrolls away from the bottom", () => {
    expect(isFeedScrolledToBottom({ scrollHeight: 1000, clientHeight: 400, scrollTop: 100 })).toBe(false);
  });

  it("treats content that does not overflow as at the bottom", () => {
    expect(isFeedScrolledToBottom({ scrollHeight: 400, clientHeight: 400, scrollTop: 0 })).toBe(true);
  });
});

describe("scrollFeedToBottom", () => {
  it("scrolls the feed to its full height with a smooth animation", () => {
    const scrollTo = vi.fn();
    scrollFeedToBottom({ scrollHeight: 1800, scrollTo });
    expect(scrollTo).toHaveBeenCalledTimes(1);
    expect(scrollTo).toHaveBeenCalledWith({ top: 1800, behavior: "smooth" });
  });

  it("targets the current height rather than a captured one", () => {
    const scrollTo = vi.fn();
    const feed = { scrollHeight: 500, scrollTo };
    scrollFeedToBottom(feed);
    feed.scrollHeight = 1200;
    scrollFeedToBottom(feed);
    expect(scrollTo).toHaveBeenNthCalledWith(1, { top: 500, behavior: "smooth" });
    expect(scrollTo).toHaveBeenNthCalledWith(2, { top: 1200, behavior: "smooth" });
  });
});

describe("currentPlanStatus", () => {
  it("maps an empty plan to the drafting state", () => {
    expect(currentPlanStatus(workspace())).toEqual({ label: "待完善计划", tone: "draft" });
  });

  it("counts only book-level foundation cards as pending plan material", () => {
    expect(currentPlanStatus(workspace({
      canon_entries: [
        { category: "world", status: "pending_human_approval", source_chapter_id: null },
        { category: "character", status: "pending_human_approval", source_chapter_id: 7 },
      ],
    }))).toEqual({ label: "待确认资料", tone: "draft" });
    expect(currentPlanStatus(workspace({
      canon_entries: [
        { category: "character", status: "pending_human_approval", source_chapter_id: 7 },
      ],
    }))).toEqual({ label: "待完善计划", tone: "draft" });
  });

  it("stops asking for confirmation once every card is adopted", () => {
    expect(currentPlanStatus(workspace({
      canon_entries: [
        { category: "world", status: "approved", source_chapter_id: null },
      ],
    }))).toEqual({ label: "待完善计划", tone: "draft" });
  });

  it("distinguishes review, blocking, and confirmed states", () => {
    const review = {
      canon_fingerprint: "current",
      status: "pending_human_confirmation",
      issues: [],
    };
    expect(currentPlanStatus(workspace({ story_bible: { status: "needs_review" }, story_bible_review: review }))).toEqual({
      label: "待确认审校",
      tone: "awaiting",
    });
    expect(currentPlanStatus(workspace({
      story_bible: { status: "needs_review" },
      story_bible_review: { ...review, issues: [{ severity: "major" }] },
    }))).toEqual({ label: "存在阻断问题", tone: "blocked" });
    expect(currentPlanStatus(workspace({
      story_bible: { status: "confirmed" },
      story_bible_review: { ...review, status: "confirmed" },
    }))).toEqual({ label: "已采用", tone: "confirmed" });
  });
});

describe("foundationStageReadiness", () => {
  it("marks a stage as ready once it has book-level material", () => {
    const stages = foundationStageReadiness({
      canon_entries: [
        { category: "world", status: "pending_human_approval", source_chapter_id: null, content: "火脉残缺者只能做杂役。" },
      ],
      chapter_plans: [],
    });

    expect(stages.map((stage) => [stage.stage, stage.ready, stage.pendingCount])).toEqual([
      ["setting", true, 1],
      ["outline", false, 0],
      ["characters", false, 0],
    ]);
  });

  it("treats chapter plans and outline cards as outline material", () => {
    const stages = foundationStageReadiness({
      canon_entries: [
        { category: "outline", status: "approved", source_chapter_id: null, content: "第一阶段目标。" },
      ],
      chapter_plans: [{ status: "pending_human_approval", content: "第一章计划。" }],
    });

    expect(stages.find((stage) => stage.stage === "outline")?.ready).toBe(true);
  });

  it("ignores chapter-derived cards and archived material", () => {
    const stages = foundationStageReadiness({
      canon_entries: [
        { category: "character", status: "approved", source_chapter_id: 7, content: "正文抽出的角色。" },
        { category: "world", status: "archived", source_chapter_id: null, content: "已归档。" },
      ],
      chapter_plans: [{ status: "archived", content: "已归档计划。" }],
    });

    expect(stages.every((stage) => !stage.ready)).toBe(true);
  });
});

describe("partialConfirmationNotice", () => {
  it("reports the adopted cards and the stages that are still missing", () => {
    const notice = partialConfirmationNotice({
      status: "partially_confirmed",
      approved_card_count: 12,
      approved_plan_count: 0,
      story_bible: null,
      review: null,
      blockers: ["缺少大纲基础资料", "缺少角色基础资料"],
      missing_stages: ["outline", "characters"],
    });

    expect(notice).toContain("已采纳 12 张资料卡");
    expect(notice).toContain("大纲");
    expect(notice).toContain("角色");
    expect(notice).not.toContain("世界观");
  });

  it("says the available material is already adopted when nothing new was pending", () => {
    const notice = partialConfirmationNotice({
      status: "partially_confirmed",
      approved_card_count: 0,
      approved_plan_count: 0,
      story_bible: null,
      review: null,
      blockers: ["缺少大纲基础资料", "缺少角色基础资料"],
      missing_stages: ["outline", "characters"],
    });

    expect(notice).toContain("现有阶段资料已全部采纳");
    expect(notice).toContain("大纲、角色");
  });
});

describe("formatReviewInstructions", () => {
  it("turns trial-reading issues into an agent revision prompt", () => {
    const prompt = formatReviewInstructions([{
      issue_type: "节奏拖沓",
      severity: "major",
      location: "第 2 段",
      reason: "冲突推进停滞",
      suggestion: "压缩解释并提前落下动作",
      evidence_quote: "他想了很久",
    }]);

    expect(prompt).toContain("请根据当前候选稿的试读建议进行修订");
    expect(prompt).toContain("[major] 节奏拖沓");
    expect(prompt).toContain("修订建议：压缩解释并提前落下动作");
    expect(prompt).toContain("依据：他想了很久");
  });
});

describe("buildChapterAgentPrompt", () => {
  it("routes a new chapter version through the main Agent conversation", () => {
    expect(buildChapterAgentPrompt("潮痕档案", "第 1 章《雨夜入城》", "draft")).toBe(
      "请由主 Agent 处理《潮痕档案》的第 1 章《雨夜入城》：生成一版新的章节候选版本。请先确认任务类型，再委托对应的专业 Agent 执行。结果必须作为候选版本放入工作区供我确认采用，不要直接替换正式正文。",
    );
  });

  it("keeps manual instructions in the same main-Agent request", () => {
    expect(buildChapterAgentPrompt("潮痕档案", "第 1 章", "draft", "把结尾停在门打开之前")).toContain(
      "补充要求：把结尾停在门打开之前",
    );
  });
});

describe("splitThinkingContent", () => {
  it("splits the first sentence into the title and the rest into the body", () => {
    const { title, rest } = splitThinkingContent("正在核对角色卡。发现两处时间线问题需要修正。", false);

    expect(title).toBe("正在核对角色卡");
    expect(rest).toBe("发现两处时间线问题需要修正。");
  });

  it("keeps a single-sentence thought as title only", () => {
    const { title, rest } = splitThinkingContent("正在结合工具结果继续分析……", true);

    expect(title).toBe("正在结合工具结果继续分析……");
    expect(rest).toBe("");
  });

  it("falls back to a placeholder title for empty content", () => {
    expect(splitThinkingContent("  ", false).title).toBe("思考摘要");
    expect(splitThinkingContent("  ", true).title).toBe("思考中");
  });
});

describe("compactAssistantModelName", () => {
  it("removes a redundant provider prefix from the composer label", () => {
    expect(compactAssistantModelName("deepseek-v4-pro")).toBe("v4 pro");
  });

  it("keeps custom model names and handles an empty value", () => {
    expect(compactAssistantModelName("local-model_alpha")).toBe("local model alpha");
    expect(compactAssistantModelName("   ")).toBe("");
  });
});

describe("isTopLevelStreamingRun", () => {
  const run = {
    id: 1,
    project_id: 1,
    chapter_id: 2,
    stage: "draft",
    output: "",
    status: "running",
    error: null,
    elapsed_ms: 0,
    created_at: "2026-09-23T00:00:00Z",
    parent_run_id: null,
    run_kind: "legacy",
    task_title: null,
  };

  it("keeps only root non-orchestrator runs in the central artifact stream", () => {
    expect(isTopLevelStreamingRun(run)).toBe(true);
    expect(isTopLevelStreamingRun({ ...run, parent_run_id: 99, run_kind: "subagent" })).toBe(false);
    expect(isTopLevelStreamingRun({ ...run, run_kind: "orchestrator" })).toBe(false);
  });
});
