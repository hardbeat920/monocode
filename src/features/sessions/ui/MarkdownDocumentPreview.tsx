import { memo, useMemo } from "react";
import { splitMarkdownFrontmatter } from "../../../shared/lib/markdownFrontmatter";
import { ChevronDown, ChevronRight } from "../../../shared/ui/icons";
import type { OpenFileFn } from "../../search/model/search";
import { MarkdownPreview } from "./AgentMarkdown";

export const MarkdownDocumentPreview = memo(function MarkdownDocumentPreview({
  text,
  metadataLabel,
  cwd,
  onOpenFile,
}: {
  text: string;
  metadataLabel: string;
  cwd?: string;
  onOpenFile?: OpenFileFn;
}) {
  const document = useMemo(() => splitMarkdownFrontmatter(text), [text]);

  return (
    <MarkdownPreview
      text={document.body}
      cwd={cwd}
      onOpenFile={onOpenFile}
      header={
        document.metadata !== null ? (
          <details className="group/metadata mb-6 rounded-lg border border-content/10 bg-content/[0.03]">
            <summary className="flex cursor-pointer list-none items-center gap-1.5 rounded-lg px-3 py-2 text-[12px] text-content/60 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent [&::-webkit-details-marker]:hidden">
              <ChevronRight
                aria-hidden="true"
                className="size-3.5 shrink-0 text-content/50 group-open/metadata:hidden"
                strokeWidth={1.75}
              />
              <ChevronDown
                aria-hidden="true"
                className="hidden size-3.5 shrink-0 text-content/50 group-open/metadata:block"
                strokeWidth={1.75}
              />
              {metadataLabel}
            </summary>
            <pre className="whitespace-pre-wrap break-words border-t border-stroke px-3 py-2 font-mono text-[12px] leading-5 text-content/70">
              {document.metadata}
            </pre>
          </details>
        ) : null
      }
    />
  );
});
