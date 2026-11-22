import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ProjectGenreSkillStatus } from "./SettingsView";

describe("项目题材 Skill", () => {
  const selection = { skill_key: "mystery", name: "悬疑", enabled: true };

  it("没有项目时不显示", () => {
    expect(renderToStaticMarkup(createElement(ProjectGenreSkillStatus, { skills: [] }))).toBe("");
  });

  it("优先显示库里刚保存的名称和开关", () => {
    const skills = [{ ...selection, name: "推理", enabled: false }];
    const markup = renderToStaticMarkup(createElement(ProjectGenreSkillStatus, { selection, skills }));
    expect(markup).toContain("推理 · 未启用");
    expect(markup).not.toContain("悬疑");
  });

  it("缺少库记录时仍显示工作区的选中项", () => {
    const markup = renderToStaticMarkup(createElement(ProjectGenreSkillStatus, { selection, skills: [] }));
    expect(markup).toContain("悬疑 · 已启用");
  });
});
