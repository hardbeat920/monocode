import {
  CaseSensitive,
  ChevronLeft,
  LoaderCircle,
  Regex,
  WholeWord,
} from "./icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import {
  searchProject,
  type OpenFileFn,
  type ProjectSearchMatch,
} from "../lib/search";
import { FileTypeIcon } from "./FileTypeIcon";
import { t } from "../lib/i18n";

type Props = {
  cwd: string;
  focusToken?: number;
  onOpenFile: OpenFileFn;
  onClose: () => void;
};

type MatchGroup = {
  path: string;
  relative: string;
  name: string;
  matches: ProjectSearchMatch[];
};

export function ProjectSearch({
  cwd,
  focusToken = 0,
  onOpenFile,
  onClose,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const [query, setQuery] = useState("");
  const [include, setInclude] = useState("");
  const [exclude, setExclude] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  const [regex, setRegex] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [matches, setMatches] = useState<ProjectSearchMatch[]>([]);
  const [truncated, setTruncated] = useState(false);

  useEffect(() => {
    if (!focusToken) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusToken]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onCloseRef.current();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed || !cwd || cwd === "~") {
      setMatches([]);
      setTruncated(false);
      setError(null);
      setLoading(false);
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      setLoading(true);
      setError(null);
      void searchProject({
        cwd,
        query: trimmed,
        caseSensitive,
        wholeWord,
        regex,
        include: include.trim() || undefined,
        exclude: exclude.trim() || undefined,
      })
        .then((result) => {
          if (cancelled) return;
          setMatches(result.matches);
          setTruncated(result.truncated);
          setLoading(false);
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          setMatches([]);
          setTruncated(false);
          setError(err instanceof Error ? err.message : String(err));
          setLoading(false);
        });
    }, 200);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [caseSensitive, cwd, exclude, include, query, regex, wholeWord]);

  const groups = useMemo(() => groupMatches(matches), [matches]);
  const matchCount = matches.length;
  const fileCount = groups.length;

  const openMatch = (match: ProjectSearchMatch) => {
    onOpenFile(match.path, { line: match.line, column: match.column });
  };

  const onQueryKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === "Enter" && matches[0]) {
      event.preventDefault();
      openMatch(matches[0]);
    }
  };

  if (!cwd || cwd === "~") {
    return (
      <p className="px-3 py-2 text-[12px] text-content/50">{t("No project folder")}</p>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-1 border-b border-content/10 px-1.5 py-1">
        <button
          type="button"
          onClick={onClose}
          title={t("Back to files")}
          aria-label={t("Back to files")}
          className="grid size-7 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/10 hover:text-content"
        >
          <ChevronLeft className="size-4" strokeWidth={1.75} />
        </button>
        <span className="min-w-0 flex-1 truncate text-[12px] text-content/55">
          {t("Search in files")}
        </span>
      </div>
      <div className="shrink-0 space-y-2 border-b border-content/10 p-2">
        <div className="flex items-center gap-1 rounded-md border border-content/10 bg-content/5 px-2 pr-1">
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={onQueryKeyDown}
            placeholder={t("Search")}
            aria-label={t("Search")}
            spellCheck={false}
            className="min-w-0 flex-1 bg-transparent py-1.5 text-[12px] text-content outline-none placeholder:text-content/35"
          />
          <Toggle
            label={t("Match case")}
            active={caseSensitive}
            onClick={() => setCaseSensitive((value) => !value)}
          >
            <CaseSensitive className="size-3.5" strokeWidth={1.75} />
          </Toggle>
          <Toggle
            label={t("Match whole word")}
            active={wholeWord}
            onClick={() => setWholeWord((value) => !value)}
          >
            <WholeWord className="size-3.5" strokeWidth={1.75} />
          </Toggle>
          <Toggle
            label={t("Use regular expression")}
            active={regex}
            onClick={() => setRegex((value) => !value)}
          >
            <Regex className="size-3.5" strokeWidth={1.75} />
          </Toggle>
        </div>
        <input
          value={include}
          onChange={(event) => setInclude(event.target.value)}
          placeholder={t("files to include")}
          aria-label={t("files to include")}
          spellCheck={false}
          className="w-full rounded-md border border-content/10 bg-content/5 px-2 py-1.5 text-[11px] text-content outline-none placeholder:text-content/35"
        />
        <input
          value={exclude}
          onChange={(event) => setExclude(event.target.value)}
          placeholder={t("files to exclude")}
          aria-label={t("files to exclude")}
          spellCheck={false}
          className="w-full rounded-md border border-content/10 bg-content/5 px-2 py-1.5 text-[11px] text-content outline-none placeholder:text-content/35"
        />
      </div>

      <div className="flex min-h-8 shrink-0 items-center gap-2 px-3 py-1.5 text-[11px] text-content/45">
        {loading ? (
          <>
            <LoaderCircle className="size-3 animate-spin" strokeWidth={1.75} />
            <span>{t("Searching…")}</span>
          </>
        ) : error ? (
          <span className="text-red-400">{error}</span>
        ) : query.trim() ? (
          <span>
            {matchCount === 0
              ? t("No results")
              : t("{0} result{1} in {2} file{3}", [
                  matchCount,
                  matchCount === 1 ? "" : "s",
                  fileCount,
                  fileCount === 1 ? "" : "s",
                ])}
            {truncated ? ` ${t("(limited)")}` : ""}
          </span>
        ) : (
          <span>{t("Type to search across the project")}</span>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none">
        {groups.map((group) => (
          <section key={group.path} className="border-b border-content/8">
            <div className="flex items-center gap-1.5 px-2 py-1.5">
              <FileTypeIcon name={group.name} isDir={false} size={16} />
              <span className="min-w-0 flex-1 truncate text-[12px] text-content">
                {group.name}
              </span>
              <span className="rounded-full bg-accent/20 px-1.5 py-0.5 text-[10px] tabular-nums text-accent">
                {group.matches.length}
              </span>
            </div>
            <p
              className="truncate px-2 pb-1 text-[10px] text-content/40"
              title={group.relative}
            >
              {group.relative}
            </p>
            <ul>
              {group.matches.map((match) => (
                <li key={`${match.path}:${match.line}:${match.column}`}>
                  <button
                    type="button"
                    onClick={() => openMatch(match)}
                    className="flex w-full items-start gap-2 px-2 py-1 text-left hover:bg-content/5"
                  >
                    <span className="w-7 shrink-0 pt-px text-right font-mono text-[11px] text-content/35 tabular-nums">
                      {match.line}
                    </span>
                    <span className="min-w-0 flex-1 truncate font-mono text-[11px] leading-5 text-content/80">
                      <MatchPreview
                        preview={match.preview.trimEnd()}
                        query={query.trim()}
                        caseSensitive={caseSensitive}
                        regex={regex}
                      />
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}

function Toggle({
  label,
  active,
  onClick,
  children,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={`grid size-6 place-items-center rounded-sm ${
        active
          ? "bg-content/15 text-content"
          : "text-content/40 hover:bg-content/10 hover:text-content/70"
      }`}
    >
      {children}
    </button>
  );
}

function groupMatches(matches: ProjectSearchMatch[]): MatchGroup[] {
  const byPath = new Map<string, MatchGroup>();
  for (const match of matches) {
    const existing = byPath.get(match.path);
    if (existing) {
      existing.matches.push(match);
      continue;
    }
    byPath.set(match.path, {
      path: match.path,
      relative: match.relative,
      name: match.relative.split("/").pop() ?? match.relative,
      matches: [match],
    });
  }
  return [...byPath.values()];
}

function MatchPreview({
  preview,
  query,
  caseSensitive,
  regex,
}: {
  preview: string;
  query: string;
  caseSensitive: boolean;
  regex: boolean;
}) {
  if (!query) return <>{preview}</>;
  if (regex) {
    try {
      const pattern = new RegExp(query, caseSensitive ? "" : "i");
      const match = preview.match(pattern);
      if (!match || match.index == null) return <>{preview}</>;
      const start = match.index;
      const end = start + match[0].length;
      return (
        <>
          {preview.slice(0, start)}
          <mark className="rounded-sm bg-accent/35 text-content">
            {preview.slice(start, end)}
          </mark>
          {preview.slice(end)}
        </>
      );
    } catch {
      return <>{preview}</>;
    }
  }

  const source = caseSensitive ? preview : preview.toLowerCase();
  const needle = caseSensitive ? query : query.toLowerCase();
  const index = source.indexOf(needle);
  if (index < 0) return <>{preview}</>;
  const end = index + needle.length;
  return (
    <>
      {preview.slice(0, index)}
      <mark className="rounded-sm bg-accent/35 text-content">
        {preview.slice(index, end)}
      </mark>
      {preview.slice(end)}
    </>
  );
}
