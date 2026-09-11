import { useEffect, useMemo, useRef, useState } from "react";
import {
  type SessionFolderTarget,
  type SessionFolder,
} from "../lib/sessionFolders";
import { Folder, Plus, X } from "./icons";
import { useLockOverscroll } from "../hooks/useLockOverscroll";

type Props = {
  folders: SessionFolder[];
  onPick: (target: SessionFolderTarget) => void;
  onDismiss: () => void;
};

export function SessionFolderPicker({ folders, onPick, onDismiss }: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const name = query.trim();
  const matching = useMemo(() => {
    const needle = name.toLocaleLowerCase();
    if (!needle) return folders;
    return folders.filter((folder) =>
      folder.name.toLocaleLowerCase().includes(needle),
    );
  }, [folders, name]);
  const exact = folders.some(
    (folder) => folder.name.toLocaleLowerCase() === name.toLocaleLowerCase(),
  );
  const rows: SessionFolderTarget[] = [
    ...matching.map((folder): SessionFolderTarget => ({
      kind: "existing",
      folderId: folder.id,
    })),
    ...(name && !exact
      ? ([{ kind: "new", name }] satisfies SessionFolderTarget[])
      : []),
  ];

  useEffect(() => inputRef.current?.focus(), []);
  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    setActive((index) =>
      rows.length === 0 ? 0 : Math.min(index, rows.length - 1),
    );
  }, [rows.length]);

  const pick = (target: SessionFolderTarget | undefined) => {
    if (target) onPick(target);
  };

  return (
    <div
      data-session-folder-picker
      role="dialog"
      aria-label="Choose a session folder"
      className="overflow-hidden rounded-lg border border-content/10 bg-content/5 backdrop-blur-xl"
    >
      <div className="flex items-center gap-2 border-b border-content/10 px-2.5 py-2">
        <Folder className="size-3.5 shrink-0 text-content/50" />
        <input
          ref={inputRef}
          value={query}
          aria-label="Session folder"
          placeholder="Choose or name a session folder…"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              onDismiss();
              return;
            }
            if (event.key === "ArrowDown") {
              event.preventDefault();
              if (rows.length > 0)
                setActive((index) => (index + 1) % rows.length);
              return;
            }
            if (event.key === "ArrowUp") {
              event.preventDefault();
              if (rows.length > 0)
                setActive((index) => (index - 1 + rows.length) % rows.length);
              return;
            }
            if (event.key === "Enter") {
              event.preventDefault();
              pick(rows[active]);
            }
          }}
          className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/35"
        />
        <button
          type="button"
          title="Cancel"
          aria-label="Cancel"
          onClick={onDismiss}
          className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
        >
          <X className="size-3.5" />
        </button>
      </div>
      <div
        ref={lockOverscroll}
        role="listbox"
        aria-label="Session folders"
        className="max-h-[min(240px,40vh)] overflow-y-auto overscroll-none p-1"
      >
        {rows.length === 0 ? (
          <p className="px-2 py-2 text-[12px] text-content/50">
            Type a name to create the first folder
          </p>
        ) : (
          rows.map((row, index) => {
            const folder =
              row.kind === "existing"
                ? folders.find((entry) => entry.id === row.folderId)
                : undefined;
            const label =
              row.kind === "existing" ? (folder?.name ?? "") : row.name;
            return (
              <button
                key={row.kind === "existing" ? `folder:${row.folderId}` : "new"}
                type="button"
                role="option"
                aria-selected={index === active}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => setActive(index)}
                onClick={() => pick(row)}
                className={`flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-[13px] ${
                  index === active
                    ? "bg-skill/15 text-content"
                    : "text-content/75 hover:bg-content/5 hover:text-content"
                }`}
              >
                {folder ? (
                  <Folder className="size-3.5 shrink-0 text-content/50" />
                ) : (
                  <Plus className="size-3.5 shrink-0 text-skill" />
                )}
                <span className="min-w-0 flex-1 truncate">
                  {folder ? label : `Create “${label}”`}
                </span>
                {folder ? (
                  <span className="shrink-0 text-[11px] tabular-nums text-content/40">
                    {folder.sessionIds.length}
                  </span>
                ) : null}
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}
