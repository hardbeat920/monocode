import { useMemo } from "react";
import { MarkdownPreview } from "./AgentMarkdown";

/** Keep the skill's YAML header readable without interpreting it as Markdown. */
export function SkillDocumentPreview({ text }: { text: string }) {
  const document = useMemo(() => {
    const opening = text.match(/^\uFEFF?---[ \t]*\r?\n/);
    if (!opening) return { metadata: null, body: text };
    const remaining = text.slice(opening[0].length);
    const closing = /^(?:---|\.\.\.)[ \t]*(?:\r?\n|$)/m.exec(remaining);
    // An unfinished header remains visible as ordinary document text.
    if (!closing) return { metadata: null, body: text };
    return {
      metadata: remaining.slice(0, closing.index).replace(/\r?\n$/, ""),
      body: remaining.slice(closing.index + closing[0].length),
    };
  }, [text]);

  return (
    <MarkdownPreview
      text={document.body}
      header={
        document.metadata !== null ? (
          <details className="mb-6 rounded-lg border border-content/10 bg-content/[0.03]">
            <summary className="cursor-pointer rounded-lg px-3 py-2 text-[12px] text-content/60 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent">
              Skill metadata
            </summary>
            <pre className="whitespace-pre-wrap break-words border-t border-content/10 px-3 py-2 font-mono text-[12px] leading-5 text-content/70">
              {document.metadata}
            </pre>
          </details>
        ) : null
      }
    />
  );
}
