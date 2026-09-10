import { StickyNote } from "./icons";
import { useEffect, useRef, type MouseEvent as ReactMouseEvent } from "react";
import type { RankedFile } from "../lib/fileIndex";
import { isNoteMentionPath } from "../lib/notes";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { FileTypeIcon } from "./FileTypeIcon";
import { MatchText } from "./MatchText";

type Props = {
  files: RankedFile[];
  query: string;
  active: number;
  loading?: boolean;
  includeNotes?: boolean;
  onActive: (index: number) => void;
  onPick: (file: RankedFile) => void;
};

export function FileMentionPicker({
  files,
  query,
  active,
  loading,
  includeNotes = false,
  onActive,
  onPick,
}: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);
  const pointer = useRef({ x: Number.NaN, y: Number.NaN, allow: false });
  const fromPointer = useRef(false);

  useEffect(() => {
    pointer.current.allow = false;
  }, [files]);

  useEffect(() => {
    if (fromPointer.current) {
      fromPointer.current = false;
      return;
    }
    pointer.current.allow = false;
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onListMouseMove = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.clientX === pointer.current.x && e.clientY === pointer.current.y) {
      return;
    }
    pointer.current = { x: e.clientX, y: e.clientY, allow: true };
  };

  const onRowEnter = (index: number) => {
    if (!pointer.current.allow) return;
    fromPointer.current = true;
    onActive(index);
  };

  return (
    <div
      data-mention-picker
      className="overflow-hidden rounded-lg border border-content/10 bg-content/5 backdrop-blur-xl"
    >
      {files.length === 0 ? (
        <p className="px-3 py-2.5 text-[12px] text-content/50">
            {loading
              ? "Indexing files…"
              : query.trim()
                ? includeNotes
                  ? "No matching files or notes"
                  : "No matching files or folders"
                : includeNotes
                  ? "No files or notes found"
                  : "No files or folders found"}
        </p>
      ) : (
        <div
          ref={lockOverscroll}
          role="listbox"
          aria-label={includeNotes ? "Files and notes" : "Files and folders"}
          onMouseMove={onListMouseMove}
          className="max-h-[min(240px,40vh)] overflow-y-auto overscroll-none px-1 py-1"
        >
          {files.map((file, index) => {
            const highlighted = index === active;
            const note = isNoteMentionPath(file.path);
            const slash = file.relative.lastIndexOf("/");
            const dir = note ? "" : slash === -1 ? "" : file.relative.slice(0, slash);
            const nameOffset = slash === -1 ? 0 : slash + 1;
            const namePositions = note
              ? file.positions
              : file.positions
                  .filter((pos) => pos >= nameOffset)
                  .map((pos) => pos - nameOffset);
            return (
              <button
                key={file.path}
                ref={highlighted ? activeRef : undefined}
                type="button"
                role="option"
                aria-selected={highlighted}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => onRowEnter(index)}
                onClick={() => onPick(file)}
                className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] leading-none ${
                  highlighted ? "bg-content/10 text-content" : "text-content"
                }`}
              >
                <span className="shrink-0">
                  {isNoteMentionPath(file.path) ? (
                    <StickyNote className="size-3.5" strokeWidth={1.75} />
                  ) : (
                    <FileTypeIcon
                      name={file.name}
                      isDir={Boolean(file.isDir)}
                      size={15}
                    />
                  )}
                </span>
                <span
                  className={`min-w-0 flex-1 truncate ${
                    highlighted ? "text-mention" : ""
                  }`}
                >
                  <MatchText
                    text={file.name}
                    positions={namePositions}
                    active={Boolean(query.trim())}
                  />
                  {file.isDir ? "/" : null}
                </span>
                {note ? (
                  <span className="shrink-0 font-mono text-[11px] text-content/40">
                    Note
                  </span>
                ) : dir ? (
                  <span className="min-w-0 max-w-[45%] truncate font-mono text-[11px] text-content/40">
                    <MatchText
                      text={dir}
                      positions={file.positions.filter((pos) => pos < slash)}
                      active={Boolean(query.trim())}
                    />
                  </span>
                ) : null}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
