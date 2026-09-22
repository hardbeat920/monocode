import { buildCiRepairRequest, type CiRepairRequest } from "../model/ciRepair";
import { Popover, type PopoverAnchor } from "../../../shared/ui/Popover";
import {
  Check,
  ChevronRight,
  LoaderCircle,
  MessageSquare,
  Plus,
  Search,
  Sparkles,
  X,
} from "../../../shared/ui/icons";
import { useEffect, useId, useRef, useState } from "react";
import {
  fetchGithubCheckDetails,
  githubActionsJobId,
  type GithubPrCheck,
} from "../model/githubPrChecks";

export type CheckRepair = {
  onOpenSession?: (sessionId: string) => void | Promise<void>;
  number: number;
  sessions: readonly { id: string; title: string }[];
  onStart: (
    request: CiRepairRequest,
    sessionId?: string,
  ) => void | Promise<void>;
};

export function CheckRepairForm({
  anchor,
  checks,
  headOid,
  cwd,
  repo,
  repair,
  onClose,
}: {
  anchor: PopoverAnchor;
  checks: GithubPrCheck[];
  headOid: string;
  cwd: string;
  repo: string;
  repair: CheckRepair;
  onClose: () => void;
}) {
  const [sessionId, setSessionId] = useState("");
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const listId = useId();
  const choices = [
    { id: "", title: "New project chat" },
    ...repair.sessions.filter((session) =>
      (session.title || "Untitled chat")
        .toLocaleLowerCase()
        .includes(query.trim().toLocaleLowerCase()),
    ),
  ];
  const selectedTitle = sessionId
    ? repair.sessions.find((session) => session.id === sessionId)?.title ||
      "Untitled chat"
    : "New project chat";
  useEffect(() => {
    searchRef.current?.focus();
  }, []);
  useEffect(() => {
    setActive(0);
  }, [query]);
  useEffect(() => {
    setActive((index) => Math.min(index, choices.length - 1));
  }, [choices.length]);
  useEffect(() => {
    listRef.current
      ?.querySelector(`[data-active="true"]`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [active]);
  function dismiss(restoreFocus = true) {
    onClose();
    if (restoreFocus && anchor instanceof HTMLElement && anchor.isConnected)
      anchor.focus();
  }
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const starting = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  async function start() {
    if (starting.current) return;
    starting.current = true;
    setBusy(true);
    setError(null);
    try {
      const evidence = [];
      for (const check of checks) {
        const jobId = githubActionsJobId(check.url, repo);
        let details;
        try {
          details = jobId
            ? await fetchGithubCheckDetails(cwd, repo, jobId)
            : undefined;
        } catch {
          details = {
            notice: "Job details unavailable. Inspect the check URL for logs.",
          };
        }
        if (!mounted.current) return;
        evidence.push({ ...check, details });
      }
      await repair.onStart(
        buildCiRepairRequest({
          repo,
          number: repair.number,
          headOid,
          evidence,
        }),
        sessionId || undefined,
      );
      if (mounted.current) onClose();
    } catch (reason) {
      if (mounted.current)
        setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      starting.current = false;
      if (mounted.current) setBusy(false);
    }
  }
  return (
    <Popover
      anchor={anchor}
      side="bottom"
      align="end"
      gap={6}
      width={320}
      maxHeight={460}
      role="dialog"
      aria-label="Fix checks with AI"
      onDismiss={(reason) => dismiss(reason === "escape")}
      className="flex min-h-0 flex-col overflow-hidden"
    >
      <div className="flex shrink-0 items-start gap-2.5 px-3.5 pb-3 pt-3.5">
        <Sparkles
          className="mt-0.5 size-4 shrink-0 text-content/65"
          strokeWidth={1.75}
        />
        <div className="min-w-0 flex-1">
          <p className="text-[13px] font-medium leading-5 text-content">
            Fix with AI
          </p>
          <p
            title={checks.map((check) => check.name).join(", ")}
            className="mt-0.5 truncate text-[11px] leading-4 text-content/45"
          >
            {checks.length === 1
              ? checks[0].name
              : `${checks.length} failed checks`}{" "}
            ? PR #{repair.number}
          </p>
        </div>
        <button
          type="button"
          aria-label="Close fix picker"
          onClick={() => dismiss()}
          className="grid size-6 shrink-0 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content"
        >
          <X className="size-3.5" strokeWidth={1.75} />
        </button>
      </div>
      <label className="mx-1.5 flex h-9 shrink-0 items-center gap-2 rounded-md bg-content/5 px-2.5 text-content/40 focus-within:text-content/65">
        <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
        <input
          ref={searchRef}
          value={query}
          disabled={busy}
          role="combobox"
          aria-label="Search project chats"
          aria-expanded={true}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={`${listId}-${active}`}
          placeholder="Search chats..."
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setActive(
                (index) =>
                  (index +
                    (event.key === "ArrowDown" ? 1 : -1) +
                    choices.length) %
                  choices.length,
              );
            } else if (event.key === "Enter") {
              event.preventDefault();
              setSessionId(choices[active]?.id ?? "");
            }
          }}
          className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
        />
      </label>
      <div
        ref={listRef}
        id={listId}
        role="listbox"
        aria-label="Project chats"
        className="my-1.5 min-h-0 max-h-56 overflow-y-auto overscroll-none px-1.5"
      >
        {choices.map((session, index) => {
          const selected = sessionId === session.id;
          const Icon = session.id ? MessageSquare : Plus;
          return (
            <button
              key={session.id}
              id={`${listId}-${index}`}
              type="button"
              role="option"
              aria-label={session.title || "Untitled chat"}
              aria-selected={selected}
              disabled={busy}
              data-active={index === active}
              onMouseEnter={() => setActive(index)}
              onFocus={() => setActive(index)}
              onClick={() => setSessionId(session.id)}
              className={`flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[12px] disabled:opacity-50 ${index === active ? "bg-selection text-content" : "text-content/70 hover:bg-content/5 hover:text-content"}`}
            >
              <Icon
                className="size-3.5 shrink-0 text-content/50"
                strokeWidth={1.75}
              />
              <span
                className="min-w-0 flex-1 truncate"
                title={session.title || "Untitled chat"}
              >
                {session.title || "Untitled chat"}
              </span>
              {selected ? (
                <Check
                  className="size-3.5 shrink-0 text-content/75"
                  strokeWidth={1.75}
                />
              ) : null}
            </button>
          );
        })}
        {choices.length === 1 && query.trim() ? (
          <p className="px-2.5 py-3 text-[12px] text-content/45">
            No matching chats
          </p>
        ) : null}
      </div>
      {error ? (
        <p
          role="alert"
          className="px-3.5 pb-3 text-[12px] leading-4 text-rose-400"
        >
          {error}
        </p>
      ) : null}
      <div className="flex shrink-0 items-center gap-3 border-t border-stroke px-3 py-2.5">
        <div className="min-w-0 flex-1 text-[11px] leading-4">
          <p className="truncate text-content/65" title={selectedTitle}>
            {selectedTitle}
          </p>
          <p className="text-content/35">CI details included</p>
        </div>
        <button
          type="button"
          disabled={busy}
          onClick={() => void start()}
          className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md bg-selection px-2.5 text-[12px] font-medium text-content hover:bg-selection-hover disabled:opacity-50 focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/50"
        >
          {busy ? (
            <LoaderCircle
              className="size-3.5 animate-spin"
              strokeWidth={1.75}
            />
          ) : null}
          {busy ? "Preparing?" : "Start fix"}
          {!busy ? (
            <ChevronRight className="size-3" strokeWidth={1.75} />
          ) : null}
        </button>
      </div>
    </Popover>
  );
}
