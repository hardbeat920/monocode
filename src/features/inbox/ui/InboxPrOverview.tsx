import { useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  LoaderCircle,
} from "../../../shared/ui/icons";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import { formatInteger } from "../../../shared/lib/numbers";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
import type { GithubPrDiff } from "../model/githubTasks";

const EXCERPT_LINES = 4;
const EXCERPT_CHARS = 360;
const GLANCE_FILES = 6;

type DescriptionExcerpt = {
  text: string;
  images: number;
  /** True when the excerpt drops anything the full render would show. */
  truncated: boolean;
};

/**
 * Plain-text lead of a markdown body. Rendering this instead of the full
 * markdown keeps images, embeds and code blocks unloaded until asked for.
 */
export function descriptionExcerpt(body: string): DescriptionExcerpt {
  const images =
    (body.match(/!\[[^\]]*\]\([^)]*\)/g)?.length ?? 0) +
    (body.match(/<img\b/gi)?.length ?? 0) +
    (body.match(/<video\b/gi)?.length ?? 0);
  const hasBlocks =
    /^\s*(```|~~~|\|.*\|)/m.test(body) || /<details\b/i.test(body);
  const lines = body
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/(```|~~~)[\s\S]*?(\1|$)/g, "")
    .replace(/<details\b[\s\S]*?(<\/details>|$)/gi, "")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/<[^>]+>/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .split("\n")
    .map((line) =>
      line
        .replace(
          /^\s{0,3}(#{1,6}\s+|>\s?|[-*+]\s+\[[ xX]\]\s+|[-*+]\s+|\d+[.)]\s+)/,
          "",
        )
        .replace(/(\*\*|__|~~|`)/g, "")
        .replace(/^\s*([-*_]\s*){3,}$/, "")
        .replace(/^\s*\|.*\|\s*$/, "")
        .trim(),
    )
    .filter(Boolean);
  let text = lines.slice(0, EXCERPT_LINES).join("\n");
  const clipped = text.length > EXCERPT_CHARS;
  if (clipped) text = `${text.slice(0, EXCERPT_CHARS).trimEnd()}…`;
  return {
    text,
    images,
    truncated:
      clipped || lines.length > EXCERPT_LINES || images > 0 || hasBlocks,
  };
}

export function InboxDescriptionSummary({
  body,
  cwd,
}: {
  body: string;
  cwd: string;
}) {
  const excerpt = useMemo(() => descriptionExcerpt(body), [body]);
  const [expanded, setExpanded] = useState(false);

  if (!body.trim()) {
    return <p className="text-[13px] text-content/45">No description</p>;
  }
  if (!excerpt.truncated) {
    return <AgentMarkdown text={body} cwd={cwd} allowRemoteMedia />;
  }
  return (
    <section
      data-inbox-description
      className="flex flex-col gap-2 rounded-lg border border-stroke bg-content/[0.02] px-3.5 py-3"
    >
      {expanded ? (
        <AgentMarkdown text={body} cwd={cwd} allowRemoteMedia />
      ) : excerpt.text ? (
        <p className="line-clamp-4 whitespace-pre-line text-[13px] leading-relaxed text-content/75">
          {excerpt.text}
        </p>
      ) : (
        <p className="text-[13px] text-content/45">Description is media only</p>
      )}
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((current) => !current)}
        className="inline-flex items-center gap-1 self-start text-[12px] text-content/50 hover:text-content"
      >
        <ChevronDown
          className={`size-3.5 transition-transform ${
            expanded ? "rotate-180" : ""
          }`}
          strokeWidth={1.75}
        />
        {expanded ? "Show less" : "Show full description"}
        {!expanded && excerpt.images > 0 ? (
          <span className="text-content/35">
            · {excerpt.images} {excerpt.images === 1 ? "image" : "images"}
          </span>
        ) : null}
      </button>
    </section>
  );
}

export function InboxPrChangesGlance({
  diff,
  loading,
  error,
  onOpenFile,
}: {
  diff: GithubPrDiff | null;
  loading: boolean;
  error: string | null;
  onOpenFile: (path?: string) => void;
}) {
  const files = diff?.files ?? [];
  const maxChurn = Math.max(
    1,
    ...files.map((file) => file.additions + file.deletions),
  );
  const shown = files.slice(0, GLANCE_FILES);

  return (
    <section data-inbox-pr-glance className="flex flex-col gap-2">
      <div className="flex items-center gap-2 text-[12px] text-content/50">
        <h2 className="text-content/70">Changed files</h2>
        {diff ? (
          <>
            <span className="tabular-nums">
              {files.length} {files.length === 1 ? "file" : "files"}
            </span>
            <span className="flex items-center gap-1.5 text-[11px] font-semibold tabular-nums">
              <span className="text-diff-add-fg">
                +{formatInteger(diff.additions)}
              </span>
              <span className="text-diff-del-fg">
                -{formatInteger(diff.deletions)}
              </span>
            </span>
          </>
        ) : null}
        {loading ? (
          <LoaderCircle
            className="size-3 animate-spin text-content/35"
            strokeWidth={1.75}
          />
        ) : null}
        {files.length > 0 ? (
          <button
            type="button"
            onClick={() => onOpenFile()}
            className="ml-auto inline-flex items-center gap-0.5 hover:text-content"
          >
            {files.length > shown.length
              ? `View all ${files.length}`
              : "View diff"}
            <ChevronRight className="size-3.5" strokeWidth={1.75} />
          </button>
        ) : null}
      </div>
      {error && !diff ? (
        <p className="text-[12px] text-content/45">{error}</p>
      ) : diff && files.length === 0 ? (
        <p className="text-[12px] text-content/45">No file changes</p>
      ) : shown.length > 0 ? (
        <ul className="flex flex-col overflow-hidden rounded-lg border border-stroke">
          {shown.map((file) => {
            const slash = file.path.lastIndexOf("/");
            const dir = slash >= 0 ? file.path.slice(0, slash + 1) : "";
            const name = file.path.slice(slash + 1);
            const churn = file.additions + file.deletions;
            const width = Math.max(8, Math.round((churn / maxChurn) * 48));
            const added = churn > 0 ? (file.additions / churn) * width : 0;
            return (
              <li
                key={file.path}
                className="border-b border-stroke last:border-b-0"
              >
                <button
                  type="button"
                  title={file.path}
                  onClick={() => onOpenFile(file.path)}
                  className="flex w-full min-w-0 items-center gap-2 bg-content/2 px-3 py-1.5 text-left hover:bg-content/5"
                >
                  <FileTypeIcon name={name} isDir={false} size={16} />
                  <span className="flex min-w-0 flex-1 font-mono text-[12px]">
                    <span className="min-w-0 truncate text-content/40">
                      {dir}
                    </span>
                    <span className="shrink-0 text-content/85">{name}</span>
                  </span>
                  <span
                    aria-hidden
                    className="flex h-1.5 shrink-0 overflow-hidden rounded-full @max-[420px]/linked:hidden"
                    style={{ width }}
                  >
                    <span
                      className="shrink-0 bg-diff-add/80"
                      style={{ width: added }}
                    />
                    <span className="flex-1 bg-diff-del/80" />
                  </span>
                  <span className="flex w-20 shrink-0 items-center justify-end gap-1.5 text-[11px] font-semibold tabular-nums">
                    {file.additions > 0 ? (
                      <span className="text-diff-add-fg">
                        +{formatInteger(file.additions)}
                      </span>
                    ) : null}
                    {file.deletions > 0 ? (
                      <span className="text-diff-del-fg">
                        -{formatInteger(file.deletions)}
                      </span>
                    ) : null}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
