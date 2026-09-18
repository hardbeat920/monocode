import { RefreshCw, Search } from "./icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { createPortal } from "react-dom";
import {
  loadProjectFiles,
  peekProjectFiles,
  rankProjectFiles,
  recentOpenedFiles,
  type RankedFile,
} from "../lib/fileIndex";
import { LAYER } from "../lib/layers";
import { fuzzyMatch, type FuzzyHit } from "../lib/fuzzy";
import { looksLikeProject } from "../lib/recents";
import type { OpenFileFn } from "../lib/search";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { FileTypeIcon } from "./FileTypeIcon";
import { MatchText } from "./MatchText";
import { MOD, SHIFT } from "../lib/platform";
type Action = {
  id: string;
  label: string;
  hint?: string;
};

type RankedAction = Action & FuzzyHit;
export function reloadActionHint(mod = MOD, shift = SHIFT) {
  return `${mod}${shift}R`;
}

const ACTIONS: Action[] = [
  { id: "reload", label: "Reload MonoCode", hint: reloadActionHint() },
];

type Props = {
  open: boolean;
  cwd: string;
  openPaths?: string[];
  initialQuery?: string;
  onOpenFile: OpenFileFn;
  onRunAction: (id: string) => void;
  onClose: () => void;
};

export function FilePicker({
  open,
  cwd,
  openPaths = [],
  initialQuery = "",
  onOpenFile,
  onRunAction,
  onClose,
}: Props) {
  const search = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const [query, setQuery] = useState(initialQuery);
  const [active, setActive] = useState(0);
  const [files, setFiles] = useState(() => peekProjectFiles(cwd) ?? []);
  const [loading, setLoading] = useState(() => peekProjectFiles(cwd) == null);
  const [error, setError] = useState<string | null>(null);

  const recents = useMemo(() => {
    const out: string[] = [];
    const seen = new Set<string>();
    for (const path of [...recentOpenedFiles(cwd), ...openPaths]) {
      if (seen.has(path)) continue;
      seen.add(path);
      out.push(path);
    }
    return out;
  }, [cwd, openPaths, open]);
  const paletteMode = query.trim().startsWith(">");
  const actionQuery = paletteMode ? query.trim().slice(1).trim() : "";

  const results = useMemo(
    () => (paletteMode ? [] : rankProjectFiles(files, query, recents)),
    [files, paletteMode, query, recents],
  );
  const actionResults = useMemo((): RankedAction[] => {
    if (!paletteMode) return [];
    if (!actionQuery) {
      return ACTIONS.map((action) => ({
        ...action,
        score: 0,
        positions: [],
      }));
    }
    return ACTIONS.flatMap((action) => {
      const hit = fuzzyMatch(actionQuery, action.label);
      return hit ? [{ ...action, ...hit }] : [];
    }).sort((a, b) => b.score - a.score);
  }, [actionQuery, paletteMode]);
  const optionCount = paletteMode ? actionResults.length : results.length;

  useEffect(() => {
    if (!open) return;
    setQuery(initialQuery);
    setActive(0);
    setError(null);
    const cached = peekProjectFiles(cwd);
    if (cached) {
      setFiles(cached);
      setLoading(false);
    } else {
      setFiles([]);
      setLoading(looksLikeProject(cwd));
    }
    if (!looksLikeProject(cwd)) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    void loadProjectFiles(cwd, true)
      .then((next) => {
        if (cancelled) return;
        setFiles(next);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, cwd, initialQuery]);

  useEffect(() => {
    setActive((index) =>
      optionCount === 0 ? 0 : Math.min(index, optionCount - 1),
    );
  }, [optionCount]);

  useEffect(() => {
    if (!open) return;
    search.current?.focus();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  if (!open) return null;

  const pick = (file: RankedFile) => {
    onOpenFile(file.path, undefined, { exact: true });
    onClose();
  };
  const runAction = (action: RankedAction) => {
    onRunAction(action.id);
    onClose();
  };

  const onSearchKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (optionCount === 0) return;
      setActive((index) => (index + 1) % optionCount);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (optionCount === 0) return;
      setActive((index) => (index - 1 + optionCount) % optionCount);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (paletteMode) {
        const action = actionResults[active];
        if (action) runAction(action);
      } else {
        const file = results[active];
        if (file) pick(file);
      }
      return;
    }
    if (e.key === "Tab") e.preventDefault();
  };

  const empty = emptyLabel({
    cwd,
    query,
    loading,
    error,
    fileCount: files.length,
    matchCount: results.length,
    paletteMode,
    actionCount: actionResults.length,
  });

  return createPortal(
    <div className="fixed inset-0" style={{ zIndex: LAYER.dialog }}>
      <div className="absolute inset-0" onMouseDown={onClose} />
      <div
        role="dialog"
        aria-label={paletteMode ? "Command Palette" : "Go to File"}
        data-file-picker
        onMouseDown={(e) => e.stopPropagation()}
        className="absolute left-1/2 top-[12%] flex w-[min(560px,calc(100vw-24px))] -translate-x-1/2 flex-col overflow-hidden rounded-lg border border-content/10 bg-content/5 backdrop-blur-xl"
      >
        <div className="pb-1.5">
          <label className="flex items-center gap-2 border-b border-stroke px-2 py-2.5 text-content/50">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              ref={search}
              type="text"
              value={query}
              placeholder="Go to File (type > for commands)"
              aria-label={paletteMode ? "Command Palette" : "Go to File"}
              spellCheck={false}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/40"
              onChange={(e) => {
                setQuery(e.target.value);
                setActive(0);
              }}
              onKeyDown={onSearchKey}
            />
          </label>
        </div>
        {empty ? (
          <p className="px-3 pb-3 pt-1 text-[12px] text-content/50">{empty}</p>
        ) : paletteMode ? (
          <ActionList
            actions={actionResults}
            active={active}
            query={actionQuery}
            onActive={setActive}
            onRun={runAction}
          />
        ) : (
          <FileList
            files={results}
            active={active}
            query={query}
            onActive={setActive}
            onPick={pick}
          />
        )}
      </div>
    </div>,
    document.body,
  );
}

function emptyLabel({
  cwd,
  query,
  loading,
  error,
  fileCount,
  matchCount,
  paletteMode,
  actionCount,
}: {
  cwd: string;
  query: string;
  loading: boolean;
  error: string | null;
  fileCount: number;
  matchCount: number;
  paletteMode: boolean;
  actionCount: number;
}): string | null {
  if (paletteMode) return actionCount === 0 ? "No matching commands" : null;
  if (error && fileCount === 0) return error;
  if (!looksLikeProject(cwd)) return "Open a project to search files";
  if (loading && fileCount === 0) return "Indexing files…";
  if (fileCount === 0) return "No files found";
  if (matchCount === 0) {
    return query.trim() ? "No matching files" : "Type a file name to search";
  }
  return null;
}

function ActionList({
  actions,
  active,
  query,
  onActive,
  onRun,
}: {
  actions: RankedAction[];
  active: number;
  query: string;
  onActive: (index: number) => void;
  onRun: (action: RankedAction) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <div
      ref={lockOverscroll}
      role="listbox"
      aria-label="Commands"
      className="max-h-[min(380px,50vh)] overflow-y-auto overscroll-none px-1.5 pb-1.5"
    >
      {actions.map((action, index) => {
        const highlighted = index === active;
        return (
          <button
            key={action.id}
            ref={highlighted ? activeRef : undefined}
            type="button"
            role="option"
            aria-selected={highlighted}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onActive(index)}
            onClick={() => onRun(action)}
            className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-sm leading-none ${
              highlighted ? "bg-selection text-content" : "text-content"
            }`}
          >
            <RefreshCw
              className="size-4 shrink-0 text-content/50"
              strokeWidth={1.75}
            />
            <span className="min-w-0 flex-1 truncate">
              <MatchText
                text={action.label}
                positions={action.positions}
                active={Boolean(query)}
              />
            </span>
            {action.hint ? (
              <kbd className="ml-auto shrink-0 rounded border border-content/10 bg-content/5 px-1.5 py-0.5 font-mono text-[10px] text-content/50">
                {action.hint}
              </kbd>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
function FileList({
  files,
  active,
  query,
  onActive,
  onPick,
}: {
  files: RankedFile[];
  active: number;
  query: string;
  onActive: (index: number) => void;
  onPick: (file: RankedFile) => void;
}) {
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
      ref={lockOverscroll}
      role="listbox"
      aria-label="Files"
      onMouseMove={onListMouseMove}
      className="max-h-[min(380px,50vh)] overflow-y-auto overscroll-none px-1.5 pb-1.5"
    >
      {files.map((file, index) => {
        const highlighted = index === active;
        const slash = file.relative.lastIndexOf("/");
        const dir = slash === -1 ? "" : file.relative.slice(0, slash);
        const nameOffset = slash === -1 ? 0 : slash + 1;
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
            className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-sm leading-none ${
              highlighted ? "bg-selection text-content" : "text-content"
            }`}
          >
            <span className="shrink-0">
              <FileTypeIcon name={file.name} isDir={false} />
            </span>
            <span className="min-w-0 flex-1 truncate">
              <MatchText
                text={file.name}
                positions={file.positions
                  .filter((pos) => pos >= nameOffset)
                  .map((pos) => pos - nameOffset)}
                active={Boolean(query.trim())}
              />
            </span>
            {dir ? (
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
  );
}
