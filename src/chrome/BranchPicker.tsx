import { Check, GitBranch, Plus, Search } from "./icons";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  gitCheckout,
  gitCommit,
  gitCreateBranch,
  gitStageAll,
  gitStash,
  isCheckoutBlockedByChanges,
  notifyGitChanged,
  type GitBranchInfo,
} from "../lib/fs";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { useProjectBranchesState } from "../hooks/useProjectBranches";
import { Popover } from "./Popover";
import { SwitchBranchDialog } from "./SwitchBranchDialog";
import { t } from "../lib/i18n";

type Props = {
  cwd: string;
  branch?: string;
  enabled?: boolean;
  onChange?: () => void;
  onClose?: () => void;
};

const MENU_WIDTH = 280;

type Row =
  | { kind: "create"; name: string }
  | { kind: "branch"; branch: GitBranchInfo };

type PendingSwitch =
  | { kind: "create"; name: string }
  | { kind: "checkout"; name: string; remote: string | null };

const MENU_MIN_HEIGHT = 180;
const MENU_MAX_HEIGHT = 280;

export function BranchPicker({
  cwd,
  branch,
  enabled = true,
  onChange,
  onClose,
}: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [blocked, setBlocked] = useState<PendingSwitch | null>(null);
  const [blockedError, setBlockedError] = useState<string | null>(null);
  const [blockedBusy, setBlockedBusy] = useState<"stash" | "commit" | null>(
    null,
  );
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  const inProject = Boolean(cwd) && cwd !== "~";
  const { branches: projectBranches, settled: branchesSettled } =
    useProjectBranchesState(cwd, inProject);

  const current = branch || projectBranches?.current || null;
  const detached = !branch && !!projectBranches?.detached;

  const dismiss = (restore: boolean) => {
    setOpen(false);
    setQuery("");
    setError(null);
    setBusy(false);
    setBlocked(null);
    setBlockedError(null);
    setBlockedBusy(null);
    if (restore) onCloseRef.current?.();
  };

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setError(null);
    setActive(0);
  }, [open]);

  useEffect(() => {
    if (open) search.current?.focus();
  }, [open]);

  useEffect(() => {
    if (enabled) return;
    setOpen(false);
    setQuery("");
    setError(null);
    setBusy(false);
    setBlocked(null);
    setBlockedError(null);
    setBlockedBusy(null);
  }, [enabled]);

  const rows = useMemo((): Row[] => {
    const branches = projectBranches?.branches ?? [];
    const name = query.trim();
    const needle = name.toLowerCase();
    const filtered = needle
      ? branches.filter((entry) => {
          const hay = entry.remote
            ? `${entry.name} ${entry.remote}`
            : entry.name;
          return hay.toLowerCase().includes(needle);
        })
      : branches;
    const taken = branches.some(
      (entry) => !entry.remote && entry.name === name,
    );
    const selected = branch || projectBranches?.current;
    const create: Row[] =
      name && !taken ? [{ kind: "create", name }] : [];
    return [
      ...create,
      ...filtered.map((entry) => ({
        kind: "branch" as const,
        branch: {
          ...entry,
          current: entry.name === selected && !entry.remote,
        },
      })),
    ];
  }, [branch, projectBranches, query]);

  useEffect(() => {
    setActive((i) => (rows.length === 0 ? 0 : Math.min(i, rows.length - 1)));
  }, [rows.length]);

  const applySwitch = (pending: PendingSwitch) =>
    pending.kind === "create"
      ? gitCreateBranch(cwd, pending.name)
      : gitCheckout(cwd, pending.name, pending.remote);

  const finishSwitch = () => {
    notifyGitChanged();
    onChangeRef.current?.();
    dismiss(true);
  };

  const failMessage = (err: unknown) =>
    err instanceof Error ? err.message : String(err);

  const run = async (pending: PendingSwitch) => {
    if (busy || blocked) return;
    setBusy(true);
    setError(null);
    try {
      await applySwitch(pending);
      finishSwitch();
    } catch (err) {
      const message = failMessage(err);
      if (isCheckoutBlockedByChanges(message)) {
        setOpen(false);
        setQuery("");
        setError(null);
        setBusy(false);
        setBlockedError(null);
        setBlockedBusy(null);
        setBlocked(pending);
        return;
      }
      setError(message);
      setBusy(false);
      search.current?.focus();
    }
  };

  const resolveBlocked = async (
    kind: "stash" | "commit",
    work: () => Promise<unknown>,
  ) => {
    if (!blocked || blockedBusy) return;
    setBlockedBusy(kind);
    setBlockedError(null);
    try {
      await work();
      await applySwitch(blocked);
      finishSwitch();
    } catch (err) {
      setBlockedError(failMessage(err));
      setBlockedBusy(null);
    }
  };

  const pick = (row: Row) => {
    if (row.kind === "create") {
      void run({ kind: "create", name: row.name });
      return;
    }
    if (row.branch.current) {
      dismiss(true);
      return;
    }
    void run({
      kind: "checkout",
      name: row.branch.name,
      remote: row.branch.remote,
    });
  };

  const onSearchKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (rows.length === 0) return;
      setActive((i) => Math.min(rows.length - 1, i + 1));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length === 0) return;
      setActive((i) => Math.max(0, i - 1));
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[active];
      if (row) pick(row);
    }
  };

  // Always keep the control mounted so the composer's toolbar does not resize
  // when a folder isn't a git repo. The loading skeleton, "No repo" label, and
  // real branch all share the same icon + 12px mono line box.
  const awaitingBranch = inProject && !current && !branchesSettled;
  const missingGit = !current && !awaitingBranch;
  const label = current
    ? detached
      ? t("detached {0}", [current])
      : current
    : t("No repo");
  const title = awaitingBranch
    ? t("Loading branch…")
    : missingGit
      ? t("No git repository")
      : label;
  const interactive = enabled && !awaitingBranch && !missingGit;

  return (
    <div className="flex max-w-[45%] shrink-0 items-center gap-2.5">
      <div ref={root} className="relative min-w-0">
        <button
          type="button"
          title={title}
          aria-label={
            awaitingBranch
              ? t("Loading branch")
              : missingGit
                ? t("No git repository")
                : t("Branch {0}", [label])
          }
          aria-expanded={missingGit ? undefined : open}
          aria-haspopup={missingGit ? undefined : "dialog"}
          disabled={!interactive}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            if (!interactive || blocked) return;
            if (open) {
              dismiss(true);
              return;
            }
            setOpen(true);
          }}
          className={
            missingGit
              ? "flex min-w-0 cursor-default items-center gap-1.5 text-content/50"
              : `flex min-w-0 items-center gap-1.5 ${
                  open ? "text-content" : "text-content/50 hover:text-content"
                } disabled:opacity-40 disabled:hover:text-content/50`
          }
        >
          <GitBranch className="size-3.5 shrink-0" strokeWidth={1.5} />
          <span className="relative truncate font-mono text-[12px]">
            {awaitingBranch ? (
              <>
                {/*
                  A real text node, hidden rather than absent, so the pending
                  line box is produced exactly the way the loaded one is — the
                  toolbar's height cannot depend on which branch we are in.
                  "main" also keeps the reserved width near a typical branch.
                */}
                <span className="invisible">main</span>
                <span className="absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 rounded-full bg-current opacity-50" />
              </>
            ) : (
              label
            )}
          </span>
        </button>
        {blocked ? (
          <SwitchBranchDialog
            cwd={cwd}
            branch={blocked.name}
            creating={blocked.kind === "create"}
            busy={blockedBusy}
            error={blockedError}
            onStash={() => {
              void resolveBlocked("stash", () =>
                gitStash(cwd, t("WIP before switching to {0}", [blocked.name])),
              );
            }}
            onCommit={(message) => {
              void resolveBlocked("commit", async () => {
                await gitStageAll(cwd);
                await gitCommit(cwd, message);
              });
            }}
            onCancel={() => {
              if (blockedBusy) return;
              setBlocked(null);
              setBlockedError(null);
              onCloseRef.current?.();
            }}
          />
        ) : null}
        {open ? (
          <Popover
            anchor={root}
            side="top"
            width={MENU_WIDTH}
            minHeight={MENU_MIN_HEIGHT}
            maxHeight={MENU_MAX_HEIGHT}
            onDismiss={(reason) => dismiss(reason === "escape")}
            role="dialog"
            aria-label={t("Branch picker")}
            data-branch-picker
            className="flex flex-col overflow-hidden"
          >
            <label className="flex shrink-0 items-center gap-2 border-b border-content/10 px-2 py-2.5 text-content/50">
              <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
              <input
                ref={search}
                type="text"
                value={query}
                placeholder={t("Search or create a branch...")}
                aria-label={t("Search or create a branch")}
                spellCheck={false}
                autoComplete="off"
                autoCorrect="off"
                autoCapitalize="off"
                disabled={busy}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/40 disabled:opacity-60"
                onChange={(e) => {
                  setQuery(e.target.value);
                  setActive(0);
                  setError(null);
                }}
                onKeyDown={onSearchKey}
              />
            </label>
            <BranchList
              rows={rows}
              active={active}
              busy={busy}
              emptyLabel={query.trim() ? t("No matching branches") : t("No branches")}
              onActive={setActive}
              onPick={pick}
            />
            {error ? (
              <p className="max-h-16 shrink-0 overflow-y-auto whitespace-pre-wrap border-t border-content/10 px-2.5 py-2 text-[11px] leading-4 text-red-400/90">
                {error}
              </p>
            ) : null}
          </Popover>
        ) : null}
      </div>
    </div>
  );
}

function BranchList({
  rows,
  active,
  busy,
  emptyLabel,
  onActive,
  onPick,
}: {
  rows: Row[];
  active: number;
  busy: boolean;
  emptyLabel: string;
  onActive: (index: number) => void;
  onPick: (row: Row) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  if (rows.length === 0) {
    return (
      <div className="px-3 py-4 text-[12px] text-content/50">{emptyLabel}</div>
    );
  }

  return (
    <div
      ref={lockOverscroll}
      role="listbox"
      aria-label={t("Branches")}
      className="min-h-0 flex-1 overflow-y-auto overscroll-none px-1.5 py-1.5"
    >
      {rows.map((row, index) => {
        const highlighted = index === active;
        const selected = row.kind === "branch" && row.branch.current;
        return (
          <button
            key={
              row.kind === "create"
                ? `create:${row.name}`
                : `${row.branch.remote ?? "local"}:${row.branch.name}`
            }
            ref={highlighted ? activeRef : undefined}
            type="button"
            role="option"
            aria-selected={selected}
            disabled={busy}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onActive(index)}
            onClick={() => onPick(row)}
            className={
              row.kind === "create"
                ? `mb-1 flex h-8 w-full min-w-0 items-center gap-2 rounded-md px-2 text-left disabled:opacity-60 ${
                    highlighted
                      ? "bg-content/15 text-content"
                      : "bg-content/10 text-content hover:bg-content/15"
                  }`
                : `flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left disabled:opacity-60 ${
                    highlighted || selected
                      ? "bg-content/10 text-content"
                      : "text-content hover:bg-content/5"
                  }`
            }
          >
            {row.kind === "create" ? (
              <>
                <Plus className="size-3.5 shrink-0" strokeWidth={1.75} />
                <span className="min-w-0 truncate text-[12px]">
                  {t("Create and checkout {0}", [row.name])}
                </span>
              </>
            ) : (
              <>
                {selected ? (
                  <Check className="size-3.5 shrink-0" strokeWidth={1.75} />
                ) : (
                  <GitBranch
                    className="size-3.5 shrink-0 text-content/50"
                    strokeWidth={1.75}
                  />
                )}
                <span className="min-w-0 flex-1 truncate font-mono text-[12px]">
                  {row.branch.name}
                </span>
                {row.branch.remote ? (
                  <span className="shrink-0 text-[10px] text-content/40">
                    {row.branch.remote}
                  </span>
                ) : null}
              </>
            )}
          </button>
        );
      })}
    </div>
  );
}
