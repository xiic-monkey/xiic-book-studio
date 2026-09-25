# Agent 侧栏对话面板 UI 修复计划（问题 1–8）

拟定日期：2026-09-25
基线：git `a562fb3`（工作区含未提交改动）
依据：2026-09-25 右侧栏智能体对话面板 UI 走查（`dev_api` + vite 实测，面板 300px 最小宽 / 360px 默认宽均验证，含 getBoundingClientRect 测量数据）
涉及文件：`src/features/workbench/BookStudioWorkspace.tsx`、`src/styles.css`。纯前端改动，不触碰 Rust 侧与生成契约。

---

## 一、问题清单与证据

| # | 问题 | 证据 | 严重度 |
|---|---|---|---|
| 1 | Composer 底部行溢出：300px 宽度下发送按钮右缘 1615px > 面板右缘 1600px，被裁 15px，"执行中"折成两行（按钮高 40px） | 实测 `sendButton.right=1615, panelRight=1600, h=40` | 高 |
| 2 | 模型名硬裁切无省略号：`scrollWidth 84 > clientWidth 77`，"mimo v2.5 pro" 显示为 "mimo v2.5 pr"，任意宽度都裁；上下文 chips 同病 | 实测 `modelClipped=true`；`styles.css:6495` 对 flex 容器用 `text-overflow`（无效） | 高 |
| 3 | Shift+Enter / Alt+Enter 被吞掉直接发送，无法键盘换行 | `BookStudioWorkspace.tsx:4648-4653` 只排除 meta/ctrl | 高 |
| 4 | 流式期间无条件拽底：往上翻历史被不停拉回 | `BookStudioWorkspace.tsx:1526-1530` 每次 feed 变化都 `scrollTo(bottom)` | 中 |
| 5 | 思考面板：①"已完成/实时更新"被压成竖排单字；②标题与正文整句重复 | 实测截图（子会话视图）；`summary small` 无 `flex-shrink:0`；`:496` 标题取整句后 `<p>` 再渲染同一句 | 中 |
| 6 | Agent 最终输出不整洁：内部前言（"已完成 1 个子任务。"+任务名）+ 长摘录在半句处截断，聊天内无"查看候选稿"入口 | `:4585` 原样渲染；`compactAssistantOutput`（`:502`）只处理带 JSON 的输出；`WorkflowRunSummary` 无 artifact 字段（`v2-contracts.ts:22`，已核实） | 中 |
| 7 | "流式中"与"已完成"无视觉区分：所有 agent 输出永久挂 `.assistant-streaming-output`（蓝色左边线） | `:4585` 对全部 output 统一加类 | 低 |
| 8 | 执行中状态冗余 4 处（header 停止钮 / 状态 chip / 发送钮 spinner+文字 / feed 内"处理中…"），且"执行中"三字加剧问题 1 | `:4508-4519`、`:4546-4549`、`:4676-4679`、`:4588-4593` | 低 |

---

## 二、修复批次

### 批次 1：Composer 布局（问题 1 + 2，同一段选择器，必须一起修）

**根因**（两层叠加）：
1. `.assistant-composer-actions` 是 `.assistant-composer`（grid）的 item，无 `min-width: 0`，grid 轨道被行的 min-content（约 278px）撑破 244px 可用宽 → 整行顶出卡片。
2. `.assistant-composer-context-label` / `.assistant-composer-model` / `.assistant-context-chip` 是 flex 容器（`inline-flex`），`text-overflow: ellipsis` 对 flex 容器无效 → 收缩时文字硬裁。

**改动**

CSS（`styles.css`，只动本次触碰的选择器，把旧层里已被覆盖的死声明一并删掉，新规则统一放在 dock 层 `6587` 之后）：

```css
.assistant-composer { grid-template-columns: minmax(0, 1fr); }
.assistant-composer-actions { min-width: 0; }
.assistant-composer-tools { flex: 1 1 auto; }

/* 文本-only 的标签改为 block，ellipsis 才能生效 */
.assistant-composer-model { display: block; }
.assistant-context-chip { display: block; }
```

同时清理旧层冲突：删除 `:5454-5463` 旧发送按钮层的 `width: auto !important; min-width: 66px`（与 `:6522` 的 `min-width: 68px` 重复打架），发送按钮规格只保留 dock 层一份。

JSX（`BookStudioWorkspace.tsx:4668-4674`）：`.assistant-composer-context-label` 内含图标 + 裸文本节点，文本需包一层 span 承接 ellipsis：

```tsx
<span className="assistant-composer-context-label">
  <Sparkles size={12} />
  <span className="assistant-ellipsis-text">{agentCatalog.find(...)?.name ?? "主 Agent"}</span>
</span>
```

```css
.assistant-ellipsis-text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
```

**验收**（沿用走查用的测量脚本，300px 与 360px 各测一遍）：
- `sendButton.right <= panelRight`，按钮高度恒为 30px（单行）；
- `model.scrollWidth <= model.clientWidth + 1`，即出现 "…" 而非硬裁；
- 360px 下视觉与现状无回归（按钮、"修订 Agent"、模型名、徽标齐全）。

**风险**：低。纯 CSS + 一处 JSX 包裹；注意删除旧层声明时只删本次触碰的选择器，不做全局合并（那是 9 号之外的事，本计划不含）。

---

### 批次 2：输入框键盘行为（问题 3）

**改动**（`BookStudioWorkspace.tsx:4648-4653`）：

```tsx
onKeyDown={(event) => {
  if (event.key !== "Enter") return;
  if (event.shiftKey || event.altKey) return; // 交给 textarea 插入换行
  event.preventDefault();
  submitAssistantPrompt();                     // Enter 与 Ctrl/Cmd+Enter 发送
}}
```

语义变化：Shift/Alt+Enter 从"发送"改为"换行"；Ctrl/Cmd+Enter 从"换行（原 bug 行为）"改为"发送"（业界惯例）。

可选增强（同批可做）：textarea 随内容自增高，上限约 180px：

```tsx
onChange={(e) => {
  setInstruction(e.target.value);
  const el = e.target;
  el.style.height = "auto";
  el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
}}
```

**验收**：手测四条路径——Enter 发送；Shift+Enter 出现换行；Cmd/Ctrl+Enter 发送；粘贴多行文本显示完整。busy 时 Enter 不发送（既有守卫 `:2698-2709` 保持）。

**风险**：低。注意别把 `event.metaKey` 早退分支删成"meta+Enter 也插换行"，语义要按上面写全。

---

### 批次 3：滚动跟随（问题 4）

**改动**（`BookStudioWorkspace.tsx:1526-1530` 及 feed 容器 `:4554`）：

```tsx
const assistantStickToBottomRef = useRef(true);

// feed 容器加 onScroll：
onScroll={(e) => {
  const el = e.currentTarget;
  assistantStickToBottomRef.current =
    el.scrollHeight - el.scrollTop - el.clientHeight < 80;
}}

useEffect(() => {
  const feed = assistantFeedRef.current;
  if (!feed || !assistantStickToBottomRef.current) return;
  feed.scrollTo({ top: feed.scrollHeight, behavior: "smooth" });
}, [assistantFeedItems, selectedSubagentRunId]);
```

并在三个主动作处重置为跟随：`submitAssistantMessage` 发送成功时、点子任务行下钻时、新建会话时。

可选增强：`!stick && orchestratorRunIsActive` 时在 feed 底部浮一个"↓ 回到底部"小按钮（复用 `.assistant-live-status` 的定位方式）。

**验收**：Agent 执行中往上滚动 → 视口不再被拽回；滚回底部附近（<80px）→ 恢复自动跟随；发送新消息后强制回底。

**风险**：低。注意 smooth 滚动期间 onScroll 事件会把 stick 置 false 造成"跟一半断掉"，距底阈值取 80px 即可覆盖 smooth 过程中的中间态。

---

### 批次 4：思考面板（问题 5）

**改动**

CSS（`styles.css`，在 `:5328` 或最终层）：

```css
.assistant-thinking-panel > summary small { flex: 0 0 auto; }
```

JSX：把 `thinkingSummaryTitle`（`:496`）升级为"标题 + 剩余正文"二分，两个渲染点（`:4575` 主 feed、`:4604` 子会话）共用：

```tsx
function splitThinkingContent(content: string, active: boolean) {
  const trimmed = content.trim();
  const first = trimmed.split(/\n|。|！|？/)[0]?.trim() ?? "";
  const title = first.length > 52 ? `${first.slice(0, 52)}…` : first;
  const rest = trimmed.length > first.length
    ? trimmed.slice(first.length).replace(/^[。！？\n]+/, "")
    : "";
  return { title: title || (active ? "思考中" : "思考摘要"), rest };
}
// <summary><span>…{title}…</span></summary>
// {rest && <p>{rest}</p>}   ← 正文只渲染第一句之外的内容，不再重复
```

**验收**：子会话视图截图核对——"已完成/实时更新"横向单行；单句思考只出现一次（作标题）；多句思考标题首句 + 正文剩余句。

**风险**：低。注意保留 `assistant-thinking-panel-current` 的 open 属性逻辑不变。

---

### 批次 5：输出呈现与状态收敛（问题 6 + 7 + 8，同一段渲染区，一起做）

**改动 6a（输出收敛）**：`compactAssistantOutput`（`:502`）扩展为两段式——识别 `^已完成 N 个子任务。`前言时，无论 remainder 是否 JSON，都折叠为短状态行（"已完成 1 个子任务 · {label}"），摘录不再直接进聊天。

**改动 6b（长文折叠 + 入口）**：agent 输出渲染点（`:4585`）改为：

- 正文 > 240 字时用 `<details>` 折叠，summary 显示"查看全文（N 字）"；
- 运行产生的候选稿不依赖 run summary（`WorkflowRunSummary` 无 artifact 字段，已核实），入口指向工作区既有候选稿面板：输出尾部追加"打开候选稿"按钮，行为 = `switchContentSurface("workbench")` + 滚动定位到 `.chapter-candidate-panel`（`:4836`）。候选稿面板本身已展示最新候选与"已采用"状态，无需新增数据链路。

**改动 7（流式语义修正）**：渲染点（`:4585`）只在 `streamingRun` 存在且该项是当前最后一个输出时挂 `assistant-streaming-output`，已完成的输出挂中性类（无左边线或灰色边线），流式中保留蓝色边线 + 既有脉冲样式。

**改动 8（状态收敛）**：

- 发送钮 busy 态只留 spinner，去掉"执行中"文字（`:4676-4679`），`title`/`aria-label` 保留"执行中"；固定宽度（约 40px），顺带消除问题 1 的最坏形态；
- 删除 feed 内"处理中…"行（`:4588-4593`）——思考面板与工具行本身已有动效，header 状态 chip + 停止钮 + 发送钮 spinner 足够。

**验收**：
- 模拟输出（含前言 + >240 字正文）→ 聊天内是短状态行 + 折叠块 + "打开候选稿"按钮，点击后面板滚动到候选稿；
- 流式进行中最后一项带左边线，运行结束后该边线消失；
- busy 时面板右下角无文字溢出，全面板"执行中"字样只出现在 header chip 与按钮 tooltip。

**风险**：中低。6b 的滚动定位依赖候选稿面板在当前章节已渲染（无候选稿时按钮不渲染或置灰，需一并处理）；7 需要在 map 中拿"最后一个输出项"的索引，注意 `assistantFeedItems` 与 `delegatedRunEvents` 两个渲染分支都要覆盖。

---

## 三、执行顺序

```
批次 1（Composer 布局）── 最先做：纯 CSS+微 JSX，消除最显眼的硬伤，当天可验
批次 2（键盘行为）───── 独立，随批次 1 同一提交亦可
批次 3（滚动跟随）───── 独立
批次 4（思考面板）───── 独立
批次 5（输出呈现）───── 最后做：涉及渲染结构改动，建议单独提交
```

**若只做一件事**：批次 1。300px 是面板官方支持的最小宽度，当前状态下按钮被裁是用户第一眼就能看到的 bug。
**建议节奏**：1+2 一个提交，3、4 各自小提交，5 单独一个提交，便于回滚。

---

## 四、统一验收

1. `npx tsc --noEmit`、`npx vitest run` 全绿（现有 `BookStudioWorkspace.test.ts` 需随批次 2/5 的行为变化同步断言）；
2. 浏览器实测（dev_api + vite），走查脚本复测三项硬指标：300px 与 360px 下 `sendButton.right <= panelRight`、`model.scrollWidth <= clientWidth + 1`、按钮高度 30px；
3. 手测清单：Shift+Enter 换行 / Cmd+Enter 发送 / 执行中上翻不拽底 / 子会话思考面板单行状态 / 长输出折叠与"打开候选稿"定位。

---

## 五、明确不做（本次范围外）

- **不做 styles.css 全量分层合并**：4 轮覆写层（`5033` v2 / `6241` reference-style / `6527` drawer / `6587` dock）与死 CSS（`.assistant-run-card*`、`.assistant-message-avatar`、`.assistant-agent-select`、`.assistant-run-steps*`）是独立维护任务，待本计划落地后单独排期；
- **不做用户消息折叠限高**（走查问题 9 之外的第 9 条呈现类建议，涉及产品取舍：长指令是否默认收起，需先定预期）；
- **不做暗色模式、虚拟滚动、历史会话持久化**——当前会话重建链路（从 run events 回放）工作正常，未见瓶颈；
- **不动后端与生成契约**：本计划全部改动可在前端内闭环，`WorkflowRunSummary` 缺 artifact 字段用既有候选稿面板绕开，不为此扩契约。
