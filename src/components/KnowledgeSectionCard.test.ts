import { describe, expect, it } from "vitest";
import { parseKnowledgeSections } from "./KnowledgeSectionCard";

describe("parseKnowledgeSections", () => {
  it("keeps content under markdown headings and removes dividers", () => {
    expect(parseKnowledgeSections("## 世界\n- 规则\n---\n## 角色\n**名字**：林舟")).toEqual([
      { title: "世界", content: ["- 规则"] },
      { title: "角色", content: ["**名字**：林舟"] },
    ]);
  });

  it("uses a fallback section for unstructured content", () => {
    expect(parseKnowledgeSections("第一条\n第二条")).toEqual([
      { title: "资料内容", content: ["第一条", "第二条"] },
    ]);
  });
});
