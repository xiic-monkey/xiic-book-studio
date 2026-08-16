# xiic-book-studio 项目记忆

## 定位
桌面端 AI 长篇小说创作工作台（Xiic Book Studio）。核心模型：AI 生成"候选产物"，人工确认后才成为正式章节正文；设定/大纲/角色/物品/事件/伏笔/正文均沉淀为带版本的项目资料，支持回溯。

## 技术栈
- 前端：React 19 + TypeScript + Vite 7 + @tanstack/react-query + lucide-react
- 后端：Rust + Tauri 2 + SQLite（sqlite-vec 向量检索 + FTS5 trigram）
- 本地检索：bge-small-zh-v1.5 嵌入 + SQLite 向量 + FTS5
- 规模：前端 ~8k 行 TS/TSX，Rust ~31k 行

## 架构要点
- 前后端契约代码生成：`src-tauri/src/bin/generate_contracts.rs` → `src/generated/v2-contracts.ts`；`npm run contracts:check` 强制一致
- 后端分层：commands.rs（Tauri 入口）→ application.rs/use_cases.rs → db.rs/v2_storage.rs
- 领域模块：workflow（创作流+阶段前置门禁）、story_architecture、quality、gate、continuity_ledger、adoption、story_search/context_search/story_index
- 前端入口：App → BookStudioWorkspace（features/workbench、agent-runs、proposals；components；utils/diff）

## 当前状态（2026-08-09）
- 有 5 个未提交改动（agent_run_service、commands、v2_storage、workflow、BookStudioWorkspace），正在进行"工作台架构迁移 + 统一 Agent 设置 UI"的 refactor 途中
- 包管理器混用：根目录同时存在 package-lock.json（npm）与 pnpm-lock.yaml（无 packageManager 字段）；scripts 用 `npm run`，但根有 pnpm-workspace.yaml —— 建议统一
- 测试薄弱：前端仅 2 个 .test.ts（diff、KnowledgeSectionCard），后端未见测试

## 数据完整性约定
- **保留 SQLite 外键**（`db.rs:100` 开启 `PRAGMA foreign_keys = ON`，全库 82 处 FK）。用户曾质疑是否应移除，结论：本地单文件 SQLite 无 scaling 成本，FK 免费且能白送 `ON DELETE CASCADE` 级联清理，保留。
- **FK 报错的处理原则**：根因通常是"往 FK 列写入了指向已删/不存在行的陈旧引用"，而非 FK 本身。修复方式是在写入前用 `existing_chapter_id` / `existing_artifact_id` 等辅助函数做 NULL 回退，而非关掉/删除 FK（关掉只会把崩溃变静默脏数据）。
- 见 `v2_storage.rs` / `story_search.rs` 中新增的 `existing_*_id` 回退辅助函数（2026-08-12 加）。

## 注意
- API Key 存本地 SQLite（非仅 Keychain）；当前导出仅 Markdown
- USER.md 中"当前工作区：TVApp"与该工作区不符，勿混淆
