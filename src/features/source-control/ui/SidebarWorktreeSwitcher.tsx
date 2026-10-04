import { useEffect, useRef, useState, type ReactNode } from "react";
import { useProjectWorktrees } from "../hooks/useProjectWorktrees";
import { useWorktreeFocus, type WorktreeFocus } from "../model/worktreeFocus";
import { pathKey, prettyCwd } from "../../../shared/lib/paths";
import { Popover } from "../../../shared/ui/Popover";
import {
  Check,
  ChevronsUpDown,
  FolderTree,
  GitBranch,
  Loader,
} from "../../../shared/ui/icons";

/** The sidebar title. Picking a worktree narrows the sidebar, and the
 * sessions opened from it, to that working copy and names it here. */
export function SidebarWorktreeSwitcher({
  cwd,
  tabStats,
  onSelect,
  pending = false,
  switchError,
}: {
  cwd: string;
  onSelect?: (focus?: WorktreeFocus) => void;
  pending?: boolean;
  switchError?: string;
  /** Open tabs per worktree path key; hidden worktrees can still hold some. */
  tabStats?: ReadonlyMap<string, { tabs: number; busy: boolean }>;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const focus = useWorktreeFocus(cwd);
  const { data, error, refresh } = useProjectWorktrees(cwd);
  // The project folder is the default, unfocused entry; missing worktrees
  // cannot be opened, so they are left out.
  const main = data?.worktrees.find((tree) => tree.isMain);
  const worktrees =
    data?.worktrees.filter((tree) => !tree.isMain && !tree.missing) ?? [];

  // A deleted worktree cannot stay focused, or new sessions would start there.
  useEffect(() => {
    if (
      focus &&
      !pending &&
      !switchError &&
      data &&
      !data.worktrees.some(
        (tree) =>
          !tree.isMain &&
          !tree.missing &&
          pathKey(tree.path) === pathKey(focus.path),
      )
    )
      onSelect?.(undefined);
  }, [cwd, data, focus, onSelect, pending, switchError]);

  useEffect(() => {
    if (switchError) setOpen(true);
  }, [switchError]);

  const focused =
    focus &&
    worktrees.find((tree) => pathKey(tree.path) === pathKey(focus.path));
  const title = focus
    ? (focused?.branch ?? focus.branch ?? "Detached worktree")
    : "Workspace";
  if (data && worktrees.length === 0 && !focus && !switchError && !pending)
    return (
      <span className="min-w-0 truncate text-sm font-medium leading-tight">
        {title}
      </span>
    );

  const row = (
    key: string,
    selected: boolean,
    icon: ReactNode,
    label: string,
    detail: string,
    onPick: () => void,
    path: string,
  ) => (
    <button
      key={key}
      type="button"
      role="option"
      aria-selected={selected}
      onClick={() => {
        onPick();
        setOpen(false);
      }}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-content/5"
    >
      {icon}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[12px]">{label}</span>
        <span className="block truncate text-[10px] text-content/40">
          {detail}
        </span>
      </span>
      <OpenTabs stats={tabStats?.get(pathKey(path))} />
      {selected ? <Check className="size-3.5 shrink-0" /> : null}
    </button>
  );

  return (
    <>
      <button
        ref={anchor}
        type="button"
        data-tauri-drag-region="false"
        aria-label="Switch working copy"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-busy={pending}
        title={
          focus
            ? `${focus.branch ?? "detached"}\n${prettyCwd(focus.path)}`
            : (main?.branch ?? "Project folder")
        }
        onClick={() => {
          if (!open) void refresh();
          setOpen(!open);
        }}
        className="-ml-1.5 flex h-6.5 min-w-0 max-w-full items-center gap-2 rounded-md px-1.5 text-sm font-medium leading-tight hover:bg-content/8 aria-expanded:bg-content/8"
      >
        <span className="min-w-0 truncate">{title}</span>
        {pending ? (
          <Loader
            aria-label="Switching working copy"
            className="size-3.5 shrink-0 animate-spin text-content/45"
          />
        ) : (
          <ChevronsUpDown className="size-3.5 shrink-0 text-content/45" />
        )}
      </button>
      {open ? (
        <Popover
          anchor={anchor}
          side="bottom"
          align="start"
          width={280}
          maxHeight={360}
          onDismiss={() => setOpen(false)}
          role="listbox"
          aria-label="Working copies"
          className="overflow-y-auto p-1"
        >
          {row(
            "default",
            !focus,
            <GitBranch className="size-3.5 shrink-0 text-content/50" />,
            main?.branch ?? "Project folder",
            "Project folder · all sessions",
            () => onSelect?.(undefined),
            main?.path ?? cwd,
          )}
          {!data && !error ? (
            <div className="flex items-center gap-2 p-2 text-[12px] text-content/50">
              <Loader className="size-3.5 animate-spin" />
              Loading working copies…
            </div>
          ) : null}
          {worktrees.map((tree) =>
            row(
              tree.path,
              !!focus && pathKey(focus.path) === pathKey(tree.path),
              <FolderTree className="size-3.5 shrink-0 text-content/50" />,
              tree.branch ?? `Detached ${tree.head.slice(0, 7)}`,
              prettyCwd(tree.path),
              () => onSelect?.({ path: tree.path, branch: tree.branch }),
              tree.path,
            ),
          )}
          {switchError || error ? (
            <p role="alert" className="px-2 py-2 text-[11px] text-red-400">
              {switchError || error}
            </p>
          ) : null}
        </Popover>
      ) : null}
    </>
  );
}

/** Tabs a worktree keeps open while another one is shown. */
function OpenTabs({ stats }: { stats?: { tabs: number; busy: boolean } }) {
  if (!stats?.tabs) return null;
  const label = `${stats.tabs} open tab${stats.tabs === 1 ? "" : "s"}${stats.busy ? ", working" : ""}`;
  return (
    <span
      title={label}
      aria-label={label}
      className="flex shrink-0 items-center gap-1 text-[11px] tabular-nums text-content/40"
    >
      {stats.busy ? (
        <span className="size-1.5 animate-pulse rounded-full bg-accent" />
      ) : null}
      {stats.tabs}
    </span>
  );
}
