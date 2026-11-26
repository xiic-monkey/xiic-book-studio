import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { ContinuityLibraryPanel } from "./ContinuityLibraryPanel";
import type { Chapter, ProjectWorkspace, StoryEvent } from "../types";

function renderLibrary(detail: ProjectWorkspace | null) {
  return renderToStaticMarkup(createElement(ContinuityLibraryPanel, {
    focus: "events",
    detail,
    busy: false,
    status: { approved: 0, succeeded: 0, pending: 0, running: 0, failed: [] },
    participantsByEvent: new Map(),
    entities: [],
    selectedEntity: null,
    currentFacts: [],
    timeline: [],
    onRebuild: vi.fn(),
    onSelectFocus: vi.fn(),
    onSelectEntity: vi.fn(),
    onOpenEntity: vi.fn(),
    onOpenChapter: vi.fn(),
  }));
}

describe("章节记忆", () => {
  it("为空库保留三个分类和当前分类的空状态", () => {
    const markup = renderLibrary(null);
    expect(markup).toContain("章节记忆");
    expect(markup).toMatch(/aria-pressed="true"[^>]*>事件时间线/);
    expect(markup).toMatch(/aria-pressed="false"[^>]*>角色时间线/);
    expect(markup).toMatch(/aria-pressed="false"[^>]*>物品状态/);
    expect(markup).toContain("暂无事件");
  });

  it("按章序而非数据库 ID 展示事件，并保留原文与出处", () => {
    const first = { id: 20, chapter_no: 1, current_artifact_id: 101 } as Chapter;
    const second = { id: 12, chapter_no: 2, current_artifact_id: 102 } as Chapter;
    const events = [
      { id: 1, title: "发现登记簿", narrative_chapter_id: second.id, source_quote: "第二章引文" },
      { id: 2, title: "广播中断", narrative_chapter_id: first.id, source_quote: "第一章引文" },
    ] as StoryEvent[];
    const markup = renderLibrary({ chapters: [first, second], story_events: events } as ProjectWorkspace);
    expect(markup.indexOf("广播中断")).toBeLessThan(markup.indexOf("发现登记簿"));
    expect(markup).toContain("第一章引文");
    expect(markup.match(/查看正文/g)).toHaveLength(2);
  });
});
