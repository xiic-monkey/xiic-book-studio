import type { Artifact } from "../types";
import { buildUnifiedDiff, selectDiffContext } from "../utils/diff";

type ArtifactDiffPanelProps = {
  selectedArtifact: Artifact;
  compareArtifact: Artifact;
};

export function ArtifactDiffPanel({ selectedArtifact, compareArtifact }: ArtifactDiffPanelProps) {
  const diffLines = buildUnifiedDiff(compareArtifact.content, selectedArtifact.content);
  const visibleDiffLines = selectDiffContext(diffLines);

  if (diffLines.length === 0) return null;

  return (
    <details className="version-drawer diff-drawer">
      <summary>查看版本差异</summary>
      <section className="diff-board">
        <div className="diff-board-head">
          <strong>版本对比</strong>
          <span>当前 v{selectedArtifact.version} 对比 v{compareArtifact.version}</span>
        </div>
        <div className="diff-lines">
          {visibleDiffLines.slice(0, 160).map((line, index) => (
            <article className={`diff-line ${line.kind}`} key={`${line.baseLine ?? "-"}-${line.currentLine ?? "-"}-${index}`}>
              <span className="diff-marker" aria-hidden="true">
                {line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " "}
              </span>
              <span className="diff-line-number">
                {line.kind === "added" ? `当前 ${line.currentLine}` : line.kind === "removed" ? `旧版 ${line.baseLine}` : `${line.baseLine}`}
              </span>
              <pre>{line.text || "（空行）"}</pre>
            </article>
          ))}
          {diffLines.every((line) => line.kind === "same") && (
            <div className="empty-inline">两个版本内容一致</div>
          )}
          {visibleDiffLines.length > 160 && (
            <div className="empty-inline">差异已截断</div>
          )}
        </div>
      </section>
    </details>
  );
}
