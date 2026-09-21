import type { ReactNode } from "react";

type Props = {
  text: string;
  positions: number[];
  active: boolean;
};

/** Fuzzy-match text with the matched characters accented. */
export function MatchText({ text, positions, active }: Props): ReactNode {
  if (!active || positions.length === 0) return text;
  const marked = new Set(positions);
  const runs: { text: string; match: boolean }[] = [];
  for (let i = 0; i < text.length; i++) {
    const match = marked.has(i);
    const last = runs[runs.length - 1];
    if (last && last.match === match) last.text += text[i];
    else runs.push({ text: text[i], match });
  }
  return runs.map((run, index) =>
    run.match ? (
      <span key={index} className="text-accent">
        {run.text}
      </span>
    ) : (
      <span key={index}>{run.text}</span>
    ),
  );
}
