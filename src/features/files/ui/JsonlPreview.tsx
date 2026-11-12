import { memo, useMemo, useState } from "react";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { formatInteger } from "../../../shared/lib/numbers";
import { ChevronRight } from "../../../shared/ui/icons";
import { HighlightedCodeBlock } from "../../sessions/ui/HighlightedCodeBlock";
import { FileTypeIcon } from "./FileTypeIcon";
import { parseJsonl, type JsonlRecord } from "../model/jsonl";

/**
 * JSON Lines as one collapsed row per record. A record is pretty-printed and
 * highlighted only once it is expanded, so a long log costs one summary line
 * per record until someone opens it.
 */
export function JsonlPreview({ text }: { text: string }) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const { records, total } = useMemo(() => parseJsonl(text), [text]);
  const invalid = records.filter((record) => "error" in record).length;

  return (
    <div
      ref={lockOverscroll}
      tabIndex={0}
      role="region"
      aria-label="JSON Lines preview"
      className="markdown-preview h-full overflow-y-auto overscroll-none [overflow-anchor:none]"
    >
      <p className="pt-3 pr-40 pb-2 pl-4 text-[12px] text-content/50">
        {total === records.length
          ? `${formatInteger(total)} ${total === 1 ? "record" : "records"}`
          : `Showing the first ${formatInteger(records.length)} of ${formatInteger(total)} records. Switch to Source to see the rest.`}
        {invalid > 0
          ? ` · ${formatInteger(invalid)} invalid ${invalid === 1 ? "line" : "lines"}`
          : null}
      </p>
      <div className="pb-6">
        {records.map((record) => (
          <JsonlRow key={record.line} record={record} />
        ))}
      </div>
    </div>
  );
}

const JsonlRow = memo(function JsonlRow({ record }: { record: JsonlRecord }) {
  const [expanded, setExpanded] = useState(false);
  const failed = "error" in record;

  return (
    <div className="border-b border-content/6">
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded(!expanded)}
        className="flex w-full min-w-0 items-baseline gap-2 px-2 py-1 text-left font-mono text-[12px] leading-5 hover:bg-content/5"
      >
        <ChevronRight
          aria-hidden="true"
          className={`size-3 shrink-0 self-center text-content/40 ${expanded ? "rotate-90" : ""}`}
        />
        <span className="w-10 shrink-0 text-right text-[11px] tabular-nums text-content/35">
          {record.line}
        </span>
        {failed ? (
          <span className="shrink-0 rounded bg-red-400/15 px-1 font-sans text-[11px] text-red-400">
            Invalid JSON
          </span>
        ) : null}
        <span className="min-w-0 flex-1 truncate text-content/80">
          {failed ? record.text : record.summary}
        </span>
      </button>
      {expanded ? (
        <div className="pr-4 pl-[4.25rem]">
          {failed ? (
            <div className="pb-2">
              <p className="mb-1 text-[12px] text-red-400">{record.error}</p>
              <pre className="whitespace-pre-wrap break-all font-mono text-[12px] leading-5 text-content/80">
                {record.text}
              </pre>
            </div>
          ) : (
            // Styled like a JSON fence in the Markdown preview.
            <div className="agent-markdown">
              <div className="markdown-code-shell" dir="ltr">
                <span className="markdown-code-icon" aria-hidden="true">
                  <FileTypeIcon name="record.json" isDir={false} />
                </span>
                <HighlightedCodeBlock
                  code={JSON.stringify(record.value, null, 2)}
                  language="json"
                  isIncomplete={false}
                  lineNumbers
                />
              </div>
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
});
