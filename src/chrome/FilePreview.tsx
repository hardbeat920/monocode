import { CircleDashed, X } from "./icons";
import { MAX_PREVIEW_LINES } from "../lib/harness/preview";
import { displayPath, resolveWorkspacePath } from "../lib/paths";
import type { ToolPreview, ToolPreviewLine } from "../lib/session";
import { FileTypeIcon } from "./FileTypeIcon";
import { t } from "../lib/i18n";

type Status = "pending" | "accepted" | "rejected";

const KEYWORDS = new Set([
  "import",
  "export",
  "from",
  "type",
  "interface",
  "const",
  "let",
  "var",
  "function",
  "return",
  "if",
  "else",
  "for",
  "while",
  "switch",
  "case",
  "class",
  "struct",
  "enum",
  "extends",
  "implements",
  "new",
  "async",
  "await",
  "try",
  "catch",
  "throw",
  "true",
  "false",
  "null",
  "undefined",
  "this",
  "in",
  "of",
  "as",
  "is",
  "void",
  "public",
  "private",
  "protected",
  "static",
  "default",
  "package",
  "def",
  "func",
]);

type Props = {
  preview: ToolPreview;
  status: Status;
  cwd?: string;
  onOpenFile?: (path: string) => void;
};

export function FilePreview({ preview, status, cwd, onOpenFile }: Props) {
  const path = preview.path;
  const filePath = path ? (resolveWorkspacePath(path, cwd) ?? path) : undefined;
  const fileName = preview.fileName || fileNameOf(path);
  const lines = (preview.lines ?? [])
    .filter(
      (line) =>
        line.kind === "add" || line.kind === "del" || line.kind === "context",
    )
    .slice(0, MAX_PREVIEW_LINES);
  const showDiff = lines.some(
    (line) => line.kind === "add" || line.kind === "del",
  );
  const added = preview.additions ?? 0;
  const deleted = preview.deletions ?? 0;
  const label = path
    ? displayPath(path, cwd)
    : fileName || preview.title || t("File");

  return (
    <div className="overflow-hidden rounded-[10px] border border-content/10 bg-content/6">
      <div className="flex items-center gap-2 px-2.5 py-2">
        <FileTypeIcon name={fileName || "file"} isDir={false} />
        {filePath && onOpenFile ? (
          <button
            type="button"
            className="min-w-0 flex-1 truncate text-left font-mono text-[12px] font-medium text-content/85 hover:text-sky-300 hover:underline"
            title={path}
            onClick={() => onOpenFile(filePath)}
          >
            {label}
          </button>
        ) : (
          <span
            className="min-w-0 flex-1 truncate font-mono text-[12px] font-medium text-content/85"
            title={path}
          >
            {label}
          </span>
        )}
        {added > 0 || deleted > 0 ? (
          <span className="shrink-0 font-mono text-[11px] font-semibold">
            {added > 0 ? (
              <span className="text-emerald-400">+{added}</span>
            ) : null}
            {added > 0 && deleted > 0 ? " " : null}
            {deleted > 0 ? (
              <span className="text-red-400">-{deleted}</span>
            ) : null}
          </span>
        ) : (
          <StatusIcon status={status} />
        )}
      </div>
      {showDiff ? (
        <>
          <div className="h-px bg-content/10" />
          <div>
            {lines.map((line, index) => (
              <PreviewLine
                key={`${line.number ?? index}-${line.kind}-${index}`}
                line={line}
                showGutter
              />
            ))}
          </div>
        </>
      ) : null}
    </div>
  );
}

function PreviewLine({
  line,
  showGutter,
}: {
  line: ToolPreviewLine;
  showGutter: boolean;
}) {
  const bg =
    line.kind === "add"
      ? "bg-teal-800/20"
      : line.kind === "del"
        ? "bg-rose-800/20"
        : "";
  const bar =
    line.kind === "add"
      ? "bg-teal-400"
      : line.kind === "del"
        ? "bg-rose-400"
        : "bg-transparent";
  const mark = line.kind === "add" ? "+" : line.kind === "del" ? "−" : " ";
  const markColor =
    line.kind === "add"
      ? "text-teal-400"
      : line.kind === "del"
        ? "text-rose-400"
        : "text-transparent";

  return (
    <div className={`relative flex items-baseline ${bg}`}>
      <span className={`absolute inset-y-0 left-0 w-0.5 ${bar}`} />
      <span className="w-7 shrink-0 pr-1 text-right font-mono text-[10px] text-content/35">
        {line.number ?? " "}
      </span>
      {showGutter ? (
        <span
          className={`w-3 shrink-0 text-center font-mono text-[10px] font-bold ${markColor}`}
        >
          {mark}
        </span>
      ) : null}
      <span className="min-w-0 flex-1 truncate pr-2 font-mono text-[11px] leading-4.5">
        {highlight(line.text, line.kind === "context")}
      </span>
    </div>
  );
}

function StatusIcon({ status }: { status: Status }) {
  if (status === "rejected") {
    return <X className="size-3.5 shrink-0 text-red-400" strokeWidth={2} />;
  }
  if (status === "pending") {
    return (
      <CircleDashed
        className="size-3.5 shrink-0 text-content/40"
        strokeWidth={1.75}
      />
    );
  }
  return null;
}

function highlight(text: string, dimmed: boolean) {
  const dim = dimmed ? "opacity-70" : "";
  const trimmed = text.trimStart();
  if (
    trimmed.startsWith("//") ||
    trimmed.startsWith("///") ||
    trimmed.startsWith("#")
  ) {
    return <span className={`text-content/45 ${dim}`}>{text}</span>;
  }

  const parts: { text: string; color: string }[] = [];
  const regex = /\b([A-Za-z_][A-Za-z0-9_]*)\b/g;
  let last = 0;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(text))) {
    if (match.index > last) {
      parts.push({ text: text.slice(last, match.index), color: "" });
    }
    const token = match[1];
    const color = KEYWORDS.has(token)
      ? "text-teal-300"
      : /^[A-Z]/.test(token)
        ? "text-amber-200/90"
        : "";
    parts.push({ text: token, color });
    last = match.index + token.length;
  }
  if (last < text.length) parts.push({ text: text.slice(last), color: "" });

  return (
    <span className={`text-content/80 ${dim}`}>
      {parts.map((part, index) =>
        part.color ? (
          <span key={index} className={part.color}>
            {part.text}
          </span>
        ) : (
          <span key={index}>{part.text}</span>
        ),
      )}
    </span>
  );
}

function fileNameOf(path?: string): string | undefined {
  if (!path) return undefined;
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1];
}
