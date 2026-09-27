import { describe, expect, it } from "vitest";
import {
  buildChapterAgentPrompt,
  compactAssistantModelName,
  compactAssistantOutput,
  currentPlanStatus,
  formatReviewInstructions,
  isTopLevelStreamingRun,
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
