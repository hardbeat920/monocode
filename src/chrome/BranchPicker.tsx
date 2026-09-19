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
import { CreateBranchDialog } from "./CreateBranchDialog";
import { GitPickerTrigger } from "./GitPickerTrigger";
import { Popover } from "./Popover";
import { SwitchBranchDialog } from "./SwitchBranchDialog";

type Props = {
  cwd: string;
  branch?: string;
  enabled?: boolean;
  worktree?: boolean;
  initialOpen?: boolean;
  onDismiss?: () => void;
  onChange?: () => void;
  onClose?: () => void;
};

const MENU_WIDTH = 280;

type CreateRow = { kind: "create"; name: string };
type BranchRow = { kind: "branch"; branch: GitBranchInfo };
type Row = CreateRow | BranchRow;

type PendingSwitch =
  | { kind: "create"; name: string }
  | { kind: "checkout"; name: string; remote: string | null };

const MENU_MIN_HEIGHT = 180;
const MENU_MAX_HEIGHT = 280;

export function BranchPicker({
  cwd,
  branch,
  enabled = true,
  worktree = false,
  initialOpen = false,
  onDismiss,
  onChange,
  onClose,
}: Props) {
  const [open, setOpen] = useState(initialOpen);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
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
    setCreating(false);
    setQuery("");
    setError(null);
    setBusy(false);
    setBlocked(null);
    setBlockedError(null);
    setBlockedBusy(null);
    onDismiss?.();
    if (restore) onCloseRef.current?.();
  };

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setError(null);
    setActive(0);
  }, [open]);

  useEffect(() => {
    // Popover measures itself off-screen behind `visibility: hidden` before
    // placing it; focusing during that pass is a no-op in real browsers, so
    // wait a frame for the popover to actually be visible.
    if (!open) return;
    const id = requestAnimationFrame(() => search.current?.focus());
    return () => cancelAnimationFrame(id);
  }, [open]);

  useEffect(() => {
    if (enabled) return;
    setOpen(false);
    setCreating(false);
    setQuery("");
    setError(null);
    setBusy(false);
    setBlocked(null);
    setBlockedError(null);
    setBlockedBusy(null);
  }, [enabled]);

  const createName = query.trim();
  const createTaken = (projectBranches?.branches ?? []).some(
    (entry) => !entry.remote && entry.name === createName,
  );
  const createRow: CreateRow | null = createTaken
    ? null
    : { kind: "create", name: createName };

  const rows = useMemo((): BranchRow[] => {
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
    const selected = branch || projectBranches?.current;
    return filtered.map((entry) => ({
      kind: "branch" as const,
      branch: {
        ...entry,
        current: entry.name === selected && !entry.remote,
      },
    }));
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

  const run = async (
    pending: PendingSwitch,
    source: "picker" | "dialog" = "picker",
  ) => {
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
        setCreating(false);
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
      if (source === "picker") search.current?.focus();
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
      if (!row.name) {
        setOpen(false);
        setQuery("");
        setError(null);
        setCreating(true);
        return;
      }
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
      if (createName && createRow) {
        pick(createRow);
        return;
      }
      const row = rows[active];
      if (row) pick(row);
    }
  };

  // Always keep the control mounted so the composer's toolbar does not resize
  // when a folder isn't a git repo. The loading skeleton, "No repo" label, and
  // real branch all share the same icon + 12px line box.
  const awaitingBranch = inProject && !current && !branchesSettled;
  const missingGit = !current && !awaitingBranch;
  const label = current
    ? detached
      ? `detached ${current}`
      : current
    : "No repo";
  const title = awaitingBranch
    ? "Loading branch…"
    : missingGit
      ? "No git repository"
      : label;
  const interactive = enabled && !awaitingBranch && !missingGit;

  return (
    <div ref={root} className="relative flex min-w-0 shrink-0">
      <GitPickerTrigger
        title={title}
        aria-label={
          awaitingBranch
            ? "Loading branch"
            : missingGit
              ? "No git repository"
              : `Branch ${label}`
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
        label={label}
        loading={awaitingBranch}
        worktree={worktree}
      />
      {blocked ? (
        <SwitchBranchDialog
          cwd={cwd}
          branch={blocked.name}
          creating={blocked.kind === "create"}
          busy={blockedBusy}
          error={blockedError}
          onStash={() => {
            void resolveBlocked("stash", () =>
              gitStash(cwd, `WIP before switching to ${blocked.name}`),
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
      {creating ? (
        <CreateBranchDialog
          busy={busy}
          error={error}
          onCreate={(name) => {
            void run({ kind: "create", name }, "dialog");
          }}
          onCancel={() => {
            if (busy) return;
            setCreating(false);
            setError(null);
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
          aria-label="Branch picker"
          data-branch-picker
          className="flex flex-col overflow-hidden"
        >
          <label className="flex shrink-0 items-center gap-2 border-b border-stroke px-3 py-2.5 text-content/50">
            <Search className="size-3.5 shrink-0" strokeWidth={1.75} />
            <input
              ref={search}
              type="text"
              value={query}
              placeholder="Search or create a branch..."
              aria-label="Search or create a branch"
              spellCheck={false}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              disabled={busy}
              className="min-w-0 flex-1 bg-transparent font-sans text-[13px] text-content outline-none placeholder:text-content/40 disabled:opacity-60"
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
            emptyLabel={query.trim() ? "No matching branches" : "No branches"}
            onActive={setActive}
            onPick={pick}
          />
          {error ? (
            <p className="max-h-16 shrink-0 overflow-y-auto whitespace-pre-wrap border-t border-stroke px-2.5 py-2 text-[11px] leading-4 text-red-400/90">
              {error}
            </p>
          ) : null}
          {createRow ? (
            <div className="shrink-0 border-t border-stroke p-1 px-1.5">
              <button
                type="button"
                disabled={busy}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => pick(createRow)}
                className="flex h-7.5 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px] text-content/75 hover:bg-content/8 hover:text-content disabled:opacity-60"
              >
                <Plus className="size-4 shrink-0" strokeWidth={1.75} />
                <span className="min-w-0 truncate">
                  {createRow.name
                    ? `Create and checkout ${createRow.name}`
                    : "New branch"}
                </span>
              </button>
            </div>
          ) : null}
        </Popover>
      ) : null}
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
  rows: BranchRow[];
  active: number;
  busy: boolean;
  emptyLabel: string;
  onActive: (index: number) => void;
  onPick: (row: BranchRow) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  if (rows.length === 0) {
    return (
      <div className="min-h-0 flex-1 px-3 py-4 text-[12px] text-content/50">
        {emptyLabel}
      </div>
    );
  }

  return (
    <div
      ref={lockOverscroll}
      role="listbox"
      aria-label="Branches"
      className="min-h-0 flex-1 overflow-y-auto overscroll-none px-1.5 py-1.5"
    >
      {rows.map((row, index) => {
        const highlighted = index === active;
        const selected = row.branch.current;
        return (
          <button
            key={`${row.branch.remote ?? "local"}:${row.branch.name}`}
            ref={highlighted ? activeRef : undefined}
            type="button"
            role="option"
            aria-selected={selected}
            disabled={busy}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onActive(index)}
            onClick={() => onPick(row)}
            className={`flex h-8 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] disabled:opacity-60 ${
              highlighted || selected
                ? "bg-selection text-content"
                : "text-content hover:bg-content/5"
            }`}
          >
            {selected ? (
              <Check className="size-3.5 shrink-0" strokeWidth={1.75} />
            ) : (
              <GitBranch
                className="size-3.5 shrink-0 text-content/50"
                strokeWidth={1.75}
              />
            )}
            <span
              className={`min-w-0 flex-1 truncate ${selected ? "font-medium" : ""}`}
            >
              {row.branch.name}
            </span>
            {row.branch.remote ? (
              <span className="shrink-0 rounded bg-content/6 px-1.5 py-0.5 text-[10px] text-content/40">
                {row.branch.remote}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
