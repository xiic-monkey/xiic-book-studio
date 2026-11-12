import { describe, expect, it } from "vitest";
import {
  assistantToolGlyph,
  reasoningDurationSeconds,
  reasoningDurationText,
  reasoningStreamingSummary,
} from "./ConversationTimelineRows";

describe("reasoningStreamingSummary", () => {
  it("取最后一个非空行并去除首尾空白", () => {
    expect(reasoningStreamingSummary("第一行\n\n  最后一行思考  \n")).toBe("最后一行思考");
  });

  it("全空文本返回空串", () => {
    expect(reasoningStreamingSummary("  \n\n")).toBe("");
  });
});

describe("reasoningDurationSeconds", () => {
  it("向上取整且至少 1 秒", () => {
    expect(reasoningDurationSeconds(1_000, 5_000)).toBe(4);
    expect(reasoningDurationSeconds(1_000, 1_200)).toBe(1);
  });

  it("进行中以 now 计算耗时", () => {
    expect(reasoningDurationSeconds(1_000, null, 61_000)).toBe(60);
  });

  it("缺时间戳时返回 undefined", () => {
    expect(reasoningDurationSeconds(undefined, 5_000)).toBeUndefined();
    expect(reasoningDurationSeconds(1_000, null, undefined)).toBeUndefined();
  });
});

describe("reasoningDurationText", () => {
  it("有可靠时间戳时给出秒数", () => {
    expect(reasoningDurationText(1_000, 43_000)).toBe("持续了 42 秒");
  });

  it("缺时间戳时退化为模糊文案", () => {
    expect(reasoningDurationText(undefined, undefined)).toBe("持续了几秒");
  });
});

describe("assistantToolGlyph", () => {
  it("按关键词归类返回不同图标", () => {
    expect(assistantToolGlyph("search_story_facts")).toBeTruthy();
    expect(assistantToolGlyph("create_artifact")).toBeTruthy();
    expect(assistantToolGlyph("unknown_future_tool")).toBeTruthy();
  });
});
