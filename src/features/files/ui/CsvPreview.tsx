import { useMemo } from "react";
import { formatInteger } from "../../../shared/lib/numbers";
import { isNumericColumn, parseDelimited } from "../model/csv";

// A plain table keeps rendering and Find simple; the full file stays in Source.
const ROW_LIMIT = 1000;

// Opaque, so rows scrolling under the pinned header stay hidden.
const HEADER =
  "bg-background-base bg-linear-to-b from-content/[0.06] to-content/[0.06]";

/** Header pinned, row numbers, numeric columns right-aligned. */
export function CsvPreview({
  text,
  delimiter,
}: {
  text: string;
  delimiter: string;
}) {
  const { header, body, numeric, truncated } = useMemo(() => {
    // One extra row for the header.
    const table = parseDelimited(text, delimiter, ROW_LIMIT + 1);
    const [header, ...body] = table.rows;
    return {
      header,
      body,
      numeric: (header ?? []).map((_, column) => isNumericColumn(body, column)),
      truncated: table.truncated,
    };
  }, [text, delimiter]);
  const align = (column: number) =>
    numeric[column] ? "text-right tabular-nums" : "text-left";

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Leaves room for the Preview / Source toggle at the top right. */}
      <div className="flex h-10 shrink-0 items-center px-4 text-[11.5px] text-content/45">
        {header
          ? `${truncated ? "First " : ""}${count(body.length, "row")} × ${count(header.length, "column")}`
          : "No rows"}
      </div>
      {header ? (
        <div
          tabIndex={0}
          role="region"
          aria-label="CSV preview"
          className="csv-preview min-h-0 flex-1 overflow-auto px-4 pb-4 select-text"
        >
          <div className="w-max min-w-full overflow-clip rounded-[10px] border border-content/10">
            <table className="w-full border-separate border-spacing-0 text-[12.5px] text-content/85">
              <thead>
                <tr>
                  <th
                    className={`sticky top-0 z-10 w-px border-b border-content/10 ${HEADER}`}
                  />
                  {header.map((cell, column) => (
                    <th
                      key={column}
                      className={`sticky top-0 z-10 max-w-[28rem] border-b border-l border-content/10 px-3 py-2 align-bottom text-[12px] font-medium whitespace-pre-wrap break-words text-content/70 ${HEADER} ${align(column)}`}
                    >
                      {cell}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {body.map((row, index) => (
                  <tr
                    key={index}
                    className="hover:bg-content/[0.04] [&:last-child>td]:border-b-0"
                  >
                    {/* Hidden from Find and copy, like a spreadsheet's row headers. */}
                    <td
                      aria-hidden="true"
                      className="border-b border-content/5 px-3 py-1.5 text-right align-top text-[11px] text-content/30 tabular-nums select-none"
                    >
                      {index + 1}
                    </td>
                    {row.map((cell, column) => (
                      <td
                        key={column}
                        className={`max-w-[28rem] border-b border-l border-content/5 px-3 py-1.5 align-top whitespace-pre-wrap break-words ${align(column)}`}
                      >
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {truncated ? (
            <p className="pt-3 text-center text-[11.5px] text-content/45">
              Showing the first {formatInteger(ROW_LIMIT)} rows. Switch to
              Source for the whole file.
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function count(value: number, noun: string): string {
  return `${formatInteger(value)} ${noun}${value === 1 ? "" : "s"}`;
}
