import { Folder, LoaderCircle, MessageSquare, Search } from "../chrome/icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { FileTypeIcon } from "../chrome/FileTypeIcon";
import { HarnessIcon } from "../chrome/HarnessIcon";
import { MatchText } from "../chrome/MatchText";
import { ProjectLogoIcon } from "../chrome/ProjectLogoIcon";
import { OverlayNav } from "../chrome/TitleBar";
import { WindowControls } from "../chrome/WindowControls";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import {
  conversationRowsFrom,
  flattenGrouped,
  groupHits,
  hitsFromContentMatches,
  hitsFromFileRanks,
  hitsFromSessionSearch,
  mergeHits,
  searchConversationTitles,
  searchRecentProjects,
  searchSessionMessages,
  type AppSearchHit,
  type SearchScope,
} from "../lib/appSearch";
import {
  loadProjectFiles,
  peekProjectFiles,
  rankProjectFiles,
  recentOpenedFiles,
} from "../lib/fileIndex";
import { prettyCwd, projectName } from "../lib/paths";
import { IS_MAC } from "../lib/platform";
import { looksLikeProject, type RecentProject } from "../lib/recents";
import { searchProject, type OpenFileFn } from "../lib/search";
import { type Session } from "../lib/session";
import { searchSessions, type SessionSummary } from "../lib/sessionStore";

const SCOPES: { id: SearchScope; label: string }[] = [
  { id: "all", label: "All" },
  { id: "conversations", label: "Conversations" },
  { id: "files", label: "Files" },
  { id: "projects", label: "Projects" },
];

type Props = {
  open: boolean;
  cwd: string;
  recents: RecentProject[];
  history: SessionSummary[];
  sessions: Session[];
  focusToken?: number;
  besideRail?: boolean;
  onClose: () => void;
  onToggleSidebar?: () => void;
  onOpenFile: OpenFileFn;
  onOpenSession: (sessionId: string) => void;
  onOpenProject: (path: string) => void;
};

export function SearchView({
  open,
  cwd,
  recents,
  history,
  sessions,
  focusToken = 0,
  besideRail = false,
  onClose,
  onToggleSidebar,
  onOpenFile,
  onOpenSession,
  onOpenProject,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<SearchScope>("all");
  const [active, setActive] = useState(0);
  const [files, setFiles] = useState(() => peekProjectFiles(cwd) ?? []);
  const [contentHits, setContentHits] = useState<AppSearchHit[]>([]);
  const [remoteHits, setRemoteHits] = useState<AppSearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const trimmed = query.trim();

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setScope("all");
    setActive(0);
    setContentHits([]);
    setRemoteHits([]);
    setError(null);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const frame = window.requestAnimationFrame(() => {
      inputRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [open, focusToken]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    if (!looksLikeProject(cwd)) {
      setFiles([]);
      return;
    }
    const cached = peekProjectFiles(cwd);
    if (cached) setFiles(cached);
    let cancelled = false;
    void loadProjectFiles(cwd).then((next) => {
      if (!cancelled) setFiles(next);
    });
    return () => {
      cancelled = true;
    };
  }, [cwd, open]);

  const conversationRows = useMemo(
    () => conversationRowsFrom(history, sessions),
    [history, sessions],
  );
  const fileHits = useMemo(
    () =>
      trimmed && looksLikeProject(cwd)
        ? hitsFromFileRanks(
            rankProjectFiles(files, trimmed, recentOpenedFiles(cwd), 40),
          )
        : [],
    [cwd, files, trimmed],
  );
  const titleHits = useMemo(
    () => (trimmed ? searchConversationTitles(conversationRows, trimmed) : []),
    [conversationRows, trimmed],
  );
  const liveMessageHits = useMemo(
    () =>
      trimmed
        ? searchSessionMessages(
            sessions.map((session) => ({
              id: session.id,
              cwd: session.cwd,
              harness: session.harness,
              title: session.title,
              updatedAt: Date.now(),
              blocks: session.blocks,
            })),
            trimmed,
          )
        : [],
    [sessions, trimmed],
  );
  const projectHits = useMemo(
    () => (trimmed ? searchRecentProjects(recents, trimmed) : []),
    [recents, trimmed],
  );

  useEffect(() => {
    if (!open || !trimmed) {
      setRemoteHits([]);
      setContentHits([]);
      setLoading(false);
      setError(null);
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      const jobs: Promise<void>[] = [];
      const wantSessions = scope === "all" || scope === "conversations";
      const wantFiles = scope === "all" || scope === "files";

      if (wantSessions) {
        setLoading(true);
        jobs.push(
          searchSessions({ query: trimmed })
            .then((result) => {
              if (!cancelled) setRemoteHits(hitsFromSessionSearch(result.hits));
            })
            .catch(() => {
              if (!cancelled) setRemoteHits([]);
            }),
        );
      } else {
        setRemoteHits([]);
      }

      if (wantFiles && looksLikeProject(cwd)) {
        setLoading(true);
        jobs.push(
          searchProject({ cwd, query: trimmed })
            .then((result) => {
              if (!cancelled) {
                setContentHits(hitsFromContentMatches(result.matches));
                setError(null);
              }
            })
            .catch((err: unknown) => {
              if (!cancelled) {
                setContentHits([]);
                setError(err instanceof Error ? err.message : String(err));
              }
            }),
        );
      } else {
        setContentHits([]);
      }

      void Promise.all(jobs).then(() => {
        if (!cancelled) setLoading(false);
      });
    }, 200);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [cwd, open, scope, trimmed]);

  const hits = useMemo(() => {
    if (!trimmed) return [];
    return flattenGrouped(
      groupHits(
        mergeHits(
          titleHits,
          liveMessageHits,
          remoteHits,
          fileHits,
          contentHits,
          projectHits,
        ),
        scope,
      ),
    );
  }, [
    contentHits,
    fileHits,
    liveMessageHits,
    projectHits,
    remoteHits,
    scope,
    titleHits,
    trimmed,
  ]);

  const activeHit = hits[active] ?? null;

  useEffect(() => {
    setActive(0);
  }, [trimmed, scope]);

  useEffect(() => {
    if (active >= hits.length) setActive(0);
  }, [active, hits.length]);

  const openHit = (hit: AppSearchHit | null) => {
    if (!hit) return;
    if (hit.kind === "file") onOpenFile(hit.path);
    else if (hit.kind === "content") {
      onOpenFile(hit.path, { line: hit.line, column: hit.column });
    } else if (hit.kind === "conversation" || hit.kind === "message") {
      onOpenSession(hit.sessionId);
    } else onOpenProject(hit.path);
    onClose();
  };

  const onQueryKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (hits.length === 0) return;
      setActive((index) => (index + 1) % hits.length);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (hits.length === 0) return;
      setActive((index) => (index - 1 + hits.length) % hits.length);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      openHit(activeHit);
    }
  };

  if (!open) return null;

  const empty = !trimmed;
  const noResults = !empty && hits.length === 0 && !loading;

  return (
    <div
      role="search"
      aria-label="Search"
      data-app-search
      className="flex min-h-0 min-w-0 flex-1 flex-col text-content"
    >
      <div
        className="flex h-10 shrink-0 select-none items-center border-b border-content/10"
        data-tauri-drag-region="deep"
      >
        {IS_MAC && !besideRail ? <div className="w-[78px] shrink-0" /> : null}
        {besideRail ? null : (
          <OverlayNav onBack={onClose} onToggleSidebar={onToggleSidebar} />
        )}
        <label className="flex min-w-0 flex-1 items-center gap-2 px-3 text-content/50">
          <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={onQueryKeyDown}
            placeholder="Search everything..."
            aria-label="Search"
            spellCheck={false}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            data-tauri-drag-region="false"
            className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none select-text placeholder:text-content/40"
          />
          {loading ? (
            <LoaderCircle
              className="size-3.5 shrink-0 animate-spin text-content/35"
              strokeWidth={1.75}
            />
          ) : null}
        </label>
        {!IS_MAC ? <WindowControls /> : null}
      </div>

      <div className="flex h-9 shrink-0 items-center gap-px border-b border-content/10 px-3">
        {SCOPES.map((item) => {
          const selected = scope === item.id;
          return (
            <button
              key={item.id}
              type="button"
              aria-pressed={selected}
              onClick={() => setScope(item.id)}
              className={`rounded-md px-2 py-1 text-[12px] ${
                selected
                  ? "bg-content/10 text-content"
                  : "text-content/50 hover:bg-content/5 hover:text-content"
              }`}
            >
              {item.label}
            </button>
          );
        })}
      </div>

      <div
        ref={lockOverscroll}
        className={
          empty
            ? "flex min-h-0 flex-1 items-center justify-center overflow-y-auto overscroll-none"
            : "min-h-0 flex-1 overflow-y-auto overscroll-none px-1.5 py-1.5"
        }
      >
        {empty ? (
          <EmptyState />
        ) : error && hits.length === 0 ? (
          <p className="px-2 py-1.5 text-[12px] text-red-400">{error}</p>
        ) : noResults ? (
          <p className="px-2 py-1.5 text-[12px] text-content/50">No results</p>
        ) : (
          <ResultList
            hits={hits}
            active={active}
            query={trimmed}
            onActive={setActive}
            onOpen={openHit}
          />
        )}
      </div>
    </div>
  );
}

const EMPTY_DOT_COLS = 27;
const EMPTY_DOT_ROWS = 19;

function EmptyState() {
  return (
    <div className="flex flex-col items-center justify-center px-6 pb-24">
      <div className="relative mb-2 grid h-48 w-72 place-items-center">
        <div
          className="grid gap-[7px] opacity-[0.14] [mask-image:radial-gradient(ellipse_72%_68%_at_50%_50%,#000_18%,transparent_76%)]"
          style={{
            gridTemplateColumns: `repeat(${EMPTY_DOT_COLS}, minmax(0, 1fr))`,
          }}
        >
          {Array.from(
            { length: EMPTY_DOT_COLS * EMPTY_DOT_ROWS },
            (_, index) => (
              <span
                key={index}
                className="mx-auto size-[3px] rounded-full bg-content"
              />
            ),
          )}
        </div>
        <div className="absolute grid size-14 place-items-center rounded-2xl bg-content/6 backdrop-blur-sm">
          <Search className="size-6 text-content/50" strokeWidth={1.75} />
        </div>
      </div>

      <p className="max-w-xs text-center text-[13px] text-content/45">
        Find files, conversations, messages, and projects.
      </p>
    </div>
  );
}

function ResultList({
  hits,
  active,
  query,
  onActive,
  onOpen,
}: {
  hits: AppSearchHit[];
  active: number;
  query: string;
  onActive: (index: number) => void;
  onOpen: (hit: AppSearchHit) => void;
}) {
  const activeRef = useRef<HTMLButtonElement>(null);
  const pointer = useRef({ x: Number.NaN, y: Number.NaN, allow: false });
  const fromPointer = useRef(false);

  useEffect(() => {
    pointer.current.allow = false;
  }, [hits]);

  useEffect(() => {
    if (fromPointer.current) {
      fromPointer.current = false;
      return;
    }
    pointer.current.allow = false;
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onListMouseMove = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (
      event.clientX === pointer.current.x &&
      event.clientY === pointer.current.y
    ) {
      return;
    }
    pointer.current = { x: event.clientX, y: event.clientY, allow: true };
  };

  const onRowEnter = (index: number) => {
    if (!pointer.current.allow) return;
    fromPointer.current = true;
    onActive(index);
  };

  return (
    <div
      role="listbox"
      aria-label="Search results"
      onMouseMove={onListMouseMove}
    >
      {hits.map((hit, index) => {
        const highlighted = index === active;
        const row = rowCopy(hit, query);
        return (
          <button
            key={hit.id}
            ref={highlighted ? activeRef : undefined}
            type="button"
            role="option"
            aria-selected={highlighted}
            onMouseDown={(event) => event.preventDefault()}
            onMouseEnter={() => onRowEnter(index)}
            onClick={() => onOpen(hit)}
            className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] leading-none ${
              highlighted ? "bg-content/10 text-content" : "text-content"
            }`}
          >
            <span className="grid size-4 shrink-0 place-items-center">
              {row.icon}
            </span>
            <span className="min-w-0 flex-1 truncate">{row.title}</span>
            {row.meta ? (
              <span className="min-w-0 max-w-[45%] truncate font-mono text-[11px] text-content/40">
                {row.meta}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}

function rowCopy(
  hit: AppSearchHit,
  query: string,
): { icon: ReactNode; title: ReactNode; meta: string } {
  if (hit.kind === "conversation") {
    return {
      icon: <HarnessIcon harness={hit.harness} className="size-3.5" />,
      title: <MatchText text={hit.title} positions={hit.positions} active />,
      meta: projectName(hit.cwd),
    };
  }
  if (hit.kind === "message") {
    return {
      icon: (
        <MessageSquare
          className="size-3.5 text-content/55"
          strokeWidth={1.75}
        />
      ),
      title: <Highlight text={hit.preview || hit.title} query={query} />,
      meta: hit.title,
    };
  }
  if (hit.kind === "file") {
    return {
      icon: <FileTypeIcon name={hit.name} isDir={false} size={16} />,
      title: (
        <MatchText text={hit.name} positions={namePositions(hit)} active />
      ),
      meta: hit.relative,
    };
  }
  if (hit.kind === "content") {
    return {
      icon: <FileTypeIcon name={hit.name} isDir={false} size={16} />,
      title: <Highlight text={hit.preview} query={query} />,
      meta: `${hit.relative}:${hit.line}`,
    };
  }
  return {
    icon: (
      <ProjectLogoIcon
        path={hit.path}
        className="size-3.5 rounded-sm"
        fallback={Folder}
      />
    ),
    title: <MatchText text={hit.name} positions={hit.positions} active />,
    meta: prettyCwd(hit.path),
  };
}

function Highlight({ text, query }: { text: string; query: string }) {
  const needle = query.trim();
  if (!needle) return <>{text}</>;
  const index = text.toLowerCase().indexOf(needle.toLowerCase());
  if (index < 0) return <>{text}</>;
  const end = index + needle.length;
  return (
    <>
      {text.slice(0, index)}
      <span className="text-accent">{text.slice(index, end)}</span>
      {text.slice(end)}
    </>
  );
}

function namePositions(hit: {
  name: string;
  relative: string;
  positions: number[];
}) {
  const offset = Math.max(0, hit.relative.length - hit.name.length);
  return hit.positions
    .map((index) => index - offset)
    .filter((index) => index >= 0 && index < hit.name.length);
}
