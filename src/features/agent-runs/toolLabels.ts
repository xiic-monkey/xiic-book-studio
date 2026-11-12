/**
 * 工具键 → 中文显示名。会话时间线与运行详情共用，确保每个被调用的工具都能一眼看出用途。
 * 覆盖三类键：
 * 1. 现行注册工具（与 src-tauri/src/agent_tools.rs 的 definitions 对齐）；
 * 2. 知识卡更名 canon entry 前的旧工具键（历史运行记录里存的仍是旧键；后端会把新调用路由到现行实现）；
 * 3. 更早版本的遗留键（仅存在于历史记录）。
 * 未识别的键兜底展示为去掉下划线的原文。
 */
const TOOL_LABELS: Record<string, string> = {
  // —— 现行注册工具（agent_tools.rs）——
  search_story: "故事原文检索",
  search_story_facts: "故事事实检索",
  history_context: "历史上下文检索",
  reference_materials: "参考资料",
  chapter_memory: "章节记忆",
  request_chapter_memory: "委托生成章节记忆",
  continuity_check: "连续性检查",
  quality_analysis: "质量分析",
  chapter_split: "拆章规划",
  web_search: "网络搜索",
  replace_text: "替换文本",
  insert_after: "插入文本",
  delete_range: "删除范围",
  apply_patch: "应用多处修订",
  propose_create_chapter: "提议创建章节",
  propose_rename_chapter: "提议重命名章节",
  propose_artifact_candidate: "提议资料候选版本",
  create_canon_entry: "写入资料",
  update_canon_entry: "更新资料",
  create_chapter_plan: "创建章节计划",
  update_chapter_plan: "更新章节计划",
  propose_canon_entry: "提议新增资料",
  propose_update_canon_entry: "提议更新资料",
  propose_delete_canon_entry: "提议删除资料",
  propose_foreshadowing: "提议伏笔",

  // —— 旧工具键（知识卡更名 canon entry 之前）——
  create_knowledge_card: "写入知识卡",
  update_knowledge_card: "更新知识卡",
  propose_knowledge_card: "提议新增知识卡",
  propose_update_knowledge_card: "提议更新知识卡",
  propose_delete_knowledge_card: "提议删除知识卡",

  // —— 更早版本的遗留键（仅历史记录中出现）——
  story_context_search: "检索故事资料",
  prepare_agent_context: "准备创作上下文",
  check_continuity: "检查连续性",
  create_artifact: "生成候选版本",
  get_current_artifact: "读取当前版本",
  get_story_bible: "读取创作基准",
  get_chapter_context: "读取当前章节",
};

export function toolLabel(toolKey: string): string {
  return TOOL_LABELS[toolKey] ?? toolKey.replace(/[_-]+/g, " ");
}

/**
 * 提议类型 → 中文显示名。键集与 src-tauri/src/tool_runtime.rs 的 create_proposal 调用点对齐；
 * 未识别的类型兜底展示原文，新增类型时不会静默变空。
 */
const PROPOSAL_TYPE_LABELS: Record<string, string> = {
  artifact_candidate: "资料候选版本",
  canon_entry: "资料候选",
  canon_entry_update: "资料更新候选",
  canon_entry_delete: "资料归档候选",
  chapter_plan: "章节计划候选",
  chapter_plan_update: "章节计划更新候选",
  chapter_revision: "章节修订",
  create_chapter: "创建章节",
  rename_chapter: "重命名章节",
  foreshadowing: "伏笔候选",
};

export function proposalTypeLabel(proposalType: string): string {
  return PROPOSAL_TYPE_LABELS[proposalType] ?? proposalType;
}
