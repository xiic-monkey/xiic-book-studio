export type KnowledgeSection = {
  title: string;
  content: string[];
};

function cleanKnowledgeText(value: string) {
  return value
    .replace(/\*\*/g, "")
    .replace(/`/g, "")
    .trim();
}

export function parseKnowledgeSections(content: string): KnowledgeSection[] {
  const sections: KnowledgeSection[] = [];
  let current: KnowledgeSection | null = null;

  for (const rawLine of content.split("\n")) {
    const line = rawLine.trim();
    const heading = line.match(/^##+\s+(.+)$/);
    if (heading) {
      if (current) sections.push(current);
      current = { title: cleanKnowledgeText(heading[1]), content: [] };
      continue;
    }
    if (!current) continue;
    if (line && line !== "---") current.content.push(line);
  }

  if (current) sections.push(current);
  return sections.length > 0 ? sections : [{ title: "资料内容", content: content.split("\n").filter(Boolean) }];
}

export function KnowledgeSectionCard({ section }: { section: KnowledgeSection }) {
  const lines = section.content.filter((line) => !/^\|?[-:]+/.test(line.replaceAll("|", "")));
  return (
    <details className="knowledge-card" open={lines.length <= 4}>
      <summary>
        <strong>{section.title}</strong>
        <span>{lines.length} 条资料</span>
      </summary>
      <div className="knowledge-card-body">
        {lines.map((line, index) => {
          const detail = line.match(/^\*\*(.+?)\*\*[：:](.+)$/);
          if (detail) {
            return (
              <div className="knowledge-detail" key={`${detail[1]}-${index}`}>
                <strong>{cleanKnowledgeText(detail[1])}</strong>
                <span>{cleanKnowledgeText(detail[2])}</span>
              </div>
            );
          }
          if (line.startsWith("- ")) {
            return <p className="knowledge-bullet" key={`${line}-${index}`}>{cleanKnowledgeText(line.slice(2))}</p>;
          }
          if (line.startsWith("|")) {
            const cells = line.split("|").map(cleanKnowledgeText).filter(Boolean);
            return <p className="knowledge-table-row" key={`${line}-${index}`}>{cells.join(" · ")}</p>;
          }
          return <p className="knowledge-paragraph" key={`${line}-${index}`}>{cleanKnowledgeText(line)}</p>;
        })}
      </div>
    </details>
  );
}
