export type UnifiedDiffLine = {
  kind: "same" | "added" | "removed";
  text: string;
  baseLine: number | null;
  currentLine: number | null;
};

export function buildUnifiedDiff(baseContent: string, currentContent: string): UnifiedDiffLine[] {
  const base = baseContent.split("\n");
  const current = currentContent.split("\n");

  // A chapter normally has far fewer than 1,200 paragraph lines. Beyond that,
  // preserve responsiveness and show the changed middle as a replacement block.
  if (base.length > 1200 || current.length > 1200) {
    return [
      ...base.map((text, index) => ({ kind: "removed" as const, text, baseLine: index + 1, currentLine: null })),
      ...current.map((text, index) => ({ kind: "added" as const, text, baseLine: null, currentLine: index + 1 })),
    ];
  }

  const width = current.length + 1;
  const table = new Uint16Array((base.length + 1) * width);
  for (let baseIndex = base.length - 1; baseIndex >= 0; baseIndex -= 1) {
    for (let currentIndex = current.length - 1; currentIndex >= 0; currentIndex -= 1) {
      const cell = baseIndex * width + currentIndex;
      table[cell] = base[baseIndex] === current[currentIndex]
        ? table[(baseIndex + 1) * width + currentIndex + 1] + 1
        : Math.max(table[(baseIndex + 1) * width + currentIndex], table[baseIndex * width + currentIndex + 1]);
    }
  }

  const lines: UnifiedDiffLine[] = [];
  let baseIndex = 0;
  let currentIndex = 0;
  while (baseIndex < base.length || currentIndex < current.length) {
    if (baseIndex < base.length && currentIndex < current.length && base[baseIndex] === current[currentIndex]) {
      lines.push({ kind: "same", text: base[baseIndex], baseLine: baseIndex + 1, currentLine: currentIndex + 1 });
      baseIndex += 1;
      currentIndex += 1;
    } else if (
      currentIndex < current.length
      && (baseIndex === base.length || table[baseIndex * width + currentIndex + 1] >= table[(baseIndex + 1) * width + currentIndex])
    ) {
      lines.push({ kind: "added", text: current[currentIndex], baseLine: null, currentLine: currentIndex + 1 });
      currentIndex += 1;
    } else {
      lines.push({ kind: "removed", text: base[baseIndex], baseLine: baseIndex + 1, currentLine: null });
      baseIndex += 1;
    }
  }

  return lines;
}

export function selectDiffContext(lines: UnifiedDiffLine[], context = 2) {
  const changedIndexes = lines
    .map((line, index) => (line.kind === "same" ? -1 : index))
    .filter((index) => index >= 0);
  if (changedIndexes.length === 0) return [];

  return lines.filter((line, index) => changedIndexes.some((changedIndex) => Math.abs(changedIndex - index) <= context));
}
