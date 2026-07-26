import { describe, expect, it } from "vitest";
import { buildUnifiedDiff, selectDiffContext } from "./diff";

describe("buildUnifiedDiff", () => {
  it("preserves line numbers across an insertion and deletion", () => {
    expect(buildUnifiedDiff("alpha\nbeta\ngamma", "alpha\ndelta\ngamma")).toEqual([
      { kind: "same", text: "alpha", baseLine: 1, currentLine: 1 },
      { kind: "added", text: "delta", baseLine: null, currentLine: 2 },
      { kind: "removed", text: "beta", baseLine: 2, currentLine: null },
      { kind: "same", text: "gamma", baseLine: 3, currentLine: 3 },
    ]);
  });

  it("keeps surrounding lines for a focused diff", () => {
    const lines = buildUnifiedDiff("a\nb\nc\nd\ne", "a\nb\nx\nd\ne");
    expect(selectDiffContext(lines, 1).map((line) => line.text)).toEqual(["b", "x", "c", "d"]);
  });
});
