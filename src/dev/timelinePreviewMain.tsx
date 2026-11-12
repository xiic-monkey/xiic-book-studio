import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import {
  AssistantChatLoading,
  AssistantNarrationRow,
  AssistantPlanRow,
  AssistantProposalRow,
  AssistantReasoningRow,
  AssistantSubagentRow,
  AssistantToolGroupRow,
  AssistantToolRow,
  assistantToolGlyph,
} from "../features/agent-runs/ConversationTimelineRows";

/*
 * 时间线组件的假数据预览页（timeline-preview.html）。
 * 单栏 360px，与真实 Agent 面板同宽；样式类与主应用共用 styles.css。
 * 组件的真实接线在 BookStudioWorkspace.tsx 的 assistant-chat-feed。
 */

const longThinking = [
  "先确认子任务行缺失的根因：数据库里 started 事件带 parent_run_id，说明后端写入没问题。",
  "接着排查前端订阅回调的分流逻辑——parent_run_id 存在时事件进了 delegatedRunEvents 桶。",
  "主时间线只接收顶层事件，因此子任务行需要在 started 事件时单独插入占位行。",
  "最后核对渲染层：子任务行组件的状态标签应该随子运行终态刷新，而不是停留在 started。",
].join("\n");

const planTasks = [
  { index: 1, title: "核对第三章时间线冲突", taskType: "review", status: "success" },
  { index: 2, title: "重写第三章结尾过渡段", taskType: "revision", status: "running" },
  { index: 3, title: "补充角色对话", taskType: "draft", status: "pending" },
];

function App() {
  return (
    <section
      className="assistant-panel assistant-panel-v2"
      style={{ position: "static", height: "100dvh", width: 360 }}
    >
      <div className="assistant-chat-feed">
        <article className="assistant-message assistant-message-user">
          <div className="assistant-message-body"><p>帮我核对第三章的时间线。</p></div>
        </article>

        <AssistantReasoningRow content={longThinking} streaming startedAtMs={Date.now() - 12_000} />

        <AssistantPlanRow tasks={planTasks} progressLabel="1/3 完成" />

        <AssistantNarrationRow content="正在派发重写任务给修订 Agent。" tone="info" />
        <AssistantNarrationRow content="第 2 章夜戏与第 3 章清晨描写冲突，需要人工确认取哪一版。" tone="warning" />

        <AssistantToolRow
          icon={assistantToolGlyph("search_story_facts")}
          label="检索故事事实"
          status="success"
          summary="命中 6 条与时间线相关的事实条目"
          meta="#1 · 812 ms · openai_tools"
        />
        <AssistantToolGroupRow countsLabel="2 检索, 1 读取">
          <AssistantToolRow
            icon={assistantToolGlyph("search_story_facts")}
            label="检索故事事实"
            status="success"
            summary="命中 4 条时间线冲突记录"
          />
          <AssistantToolRow
            icon={assistantToolGlyph("search_story")}
            label="检索故事内容"
            status="success"
            summary="定位第 2 章结尾夜戏段落"
          />
          <AssistantToolRow
            icon={assistantToolGlyph("get_chapter_context")}
            label="读取当前章节"
            status="success"
          />
        </AssistantToolGroupRow>
        <AssistantToolRow
          icon={assistantToolGlyph("get_chapter_context")}
          label="读取当前章节"
          status="running"
        />
        <AssistantToolRow
          icon={assistantToolGlyph("check_continuity")}
          label="检查连续性"
          status="failed"
          summary="第 2 章夜戏与第 3 章清晨描写冲突"
        />
        <AssistantToolRow
          icon={assistantToolGlyph("create_artifact")}
          label="生成候选版本"
          status="success"
          summary="候选稿 v3 已生成"
          meta="#4 · 2,304 ms · openai_tools"
          sections={[
            { label: "参数", text: JSON.stringify({ chapter_id: 3, instruction: "只调整夜戏到清晨的过渡", keep_rest: true }, null, 2) },
            { label: "结果", text: JSON.stringify({ artifact_id: 42, words: 3184 }, null, 2) },
          ]}
        />

        <AssistantSubagentRow
          title="重写第三章结尾"
          statusLabel="已完成"
          running={false}
          expanded
          onOpen={() => {}}
        >
          <AssistantToolRow
            icon={assistantToolGlyph("replace_text")}
            label="替换正文段落"
            status="success"
            summary="已替换第 3 章结尾段落"
          />
        </AssistantSubagentRow>
        <AssistantSubagentRow title="补写角色对话" statusLabel="执行中" running expanded onOpen={() => {}}>
          <AssistantToolRow
            icon={assistantToolGlyph("read_reference")}
            label="读取参考资料"
            status="running"
          />
        </AssistantSubagentRow>

        <AssistantProposalRow
          summary="把第 3 章结尾过渡段改写为清晨版本，并保留其余正文不动。"
          typeLabel="章节修订"
          busy={false}
          onApply={() => {}}
          onReject={() => {}}
        />

        <article className="assistant-message assistant-message-agent">
          <div className="assistant-message-body">
            <p>已定位冲突：第 2 章结尾是深夜，第 3 章开头却写阳光正烈。我让子 Agent 重写了过渡段，见候选稿 v3。</p>
          </div>
        </article>

        <AssistantChatLoading active />
      </div>
    </section>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
