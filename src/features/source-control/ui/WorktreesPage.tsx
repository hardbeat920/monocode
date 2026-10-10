import { listCowWorkspaces, type CowWorkspace } from "../model/cow";
import { useEffect, useMemo, useState } from "react";
import { CreateWorktreeDialog } from "./CreateWorktreeDialog";
import { DeleteWorktreeDialog } from "./DeleteWorktreeDialog";
import { SearchableProjectPicker } from "../../projects/ui/SearchableProjectPicker";
import {
  Copy,
  FolderOpen,
  FolderTree,
  GitBranch,
  Loader,
  Plus,
  RefreshCw,
  Trash2,
} from "../../../shared/ui/icons";
import { revealPath, subscribeGitChanged } from "../../../platform/tauri/fs";
import { useProjectWorktrees } from "../hooks/useProjectWorktrees";
import {
  isEqualOrInside,
  pathKey,
  prettyCwd,
  projectName,
} from "../../../shared/lib/paths";
import {
  loadArchivedProjects,
  type RecentProject,
} from "../../projects/model/recents";
import type { Session } from "../../sessions/model/session";
import {
  checkWorktreeRemoval,
  worktreeSessionIds,
  type RemoveWorktree,
  type Worktree,
} from "../model/worktrees";

export function WorktreesPage({
  cwd,
  recents = [],
  liveSessions = [],
  onRemove,
  onCheckRemove = checkWorktreeRemoval,
  onDeleteSessions,
}: {
  cwd: string;
  recents?: RecentProject[];
  liveSessions?: Session[];
  onRemove: RemoveWorktree;
  onCheckRemove?: RemoveWorktree;
  onDeleteSessions?: (sessionIds: readonly string[]) => Promise<boolean>;
}) {
  const projects = useMemo(() => {
    const choices: RecentProject[] = [];
    const seen = new Set<string>();
    const add = (choice: RecentProject) => {
      if (!choice.path || choice.path === "~") return;
      const key = pathKey(choice.path);
      if (seen.has(key)) return;
      seen.add(key);
      choices.push(choice);
    };
    add({ path: cwd, openedAt: Number.MAX_SAFE_INTEGER });
    recents.forEach(add);
    loadArchivedProjects().forEach((item) =>
      add({ path: item.path, openedAt: item.archivedAt }),
    );
    return choices;
  }, [cwd, recents]);
  const [project, setProject] = useState(
    cwd === "~" ? (projects[0]?.path ?? "") : cwd,
  );
  const { data, error: loadError, refresh } = useProjectWorktrees(project);
  const [error, setError] = useState<string>();
  const [cowError, setCowError] = useState<string>();
  const [cowWorkspaces, setCowWorkspaces] = useState<CowWorkspace[]>([]);
  const [cowRefresh, setCowRefresh] = useState(0);
  const workspaces: (Worktree & { cowId?: string })[] = [
    ...(data?.worktrees.filter((tree) => !tree.isMain) ?? []),
    ...cowWorkspaces.map((workspace) => ({
      path: workspace.path,
      branch: workspace.branch ?? null,
      head: workspace.head,
      isMain: false,
      locked: false,
      prunable: false,
      missing: false,
      dirty: workspace.dirty ?? null,
      unpushed: workspace.unpushed ?? null,
      sessionIds: workspace.sessionIds ?? [],
      cowId: workspace.id,
    })),
  ];
  useEffect(() => {
    setCowWorkspaces([]);
    setCowError(undefined);
  }, [project]);
  useEffect(() => {
    const refreshCopies = () => setCowRefresh((value) => value + 1);
    const unsubscribe = subscribeGitChanged(refreshCopies);
    window.addEventListener("focus", refreshCopies);
    return () => {
      unsubscribe();
      window.removeEventListener("focus", refreshCopies);
    };
  }, []);
  useEffect(() => {
    let disposed = false;
    if (project)
      void listCowWorkspaces(project).then(
        (rows) => {
          if (!disposed) {
            setCowWorkspaces(rows);
            setCowError(undefined);
          }
        },
        (error) => {
          if (!disposed) setCowError(String(error));
        },
      );
    return () => {
      disposed = true;
    };
  }, [project, cowRefresh]);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<Worktree & { cowId?: string }>();
  const [refreshingAfterFailure, setRefreshingAfterFailure] = useState(false);
  return (
    <div
      data-setting-id="project-worktrees"
      id="setting-project-worktrees"
      className="flex flex-col gap-4"
    >
      <div className="flex flex-wrap items-center gap-1">
        <SearchableProjectPicker
          cwd={project || "~"}
          recents={projects}
          className="w-fit max-w-full shrink-0"
          buttonClassName="h-7.5 max-w-full gap-2 bg-content/5 px-2.5 text-[13px] hover:bg-content/12 active:scale-[0.98]"
          onSelectProject={(path) => {
            setProject(path);
            setError(undefined);
            setDeleting(undefined);
          }}
        />
        <button
          type="button"
          disabled={!data}
          onClick={() => setCreating(true)}
          className="flex h-7.5 items-center gap-1.5 rounded-md px-2 text-[11px] hover:bg-content/12 disabled:opacity-40 active:scale-[0.97]"
        >
          <Plus className="size-3" />
          Create worktree
        </button>
      </div>
      <div className="flex items-center justify-between gap-3">
        <p className="min-w-0 flex-1 text-[12px] text-content/50">
          Deleting an isolated workspace keeps its sessions by default and
          discards uncommitted changes. Its branch and commits are kept.
        </p>
        <button
          type="button"
          title={
            loadError
              ? `Refresh failed: ${loadError}. Click to retry.`
              : "Refresh workspaces"
          }
          aria-label="Refresh workspaces"
          disabled={!project}
          onClick={() => {
            void refresh();
            setCowRefresh((value) => value + 1);
          }}
          className={`flex h-7 shrink-0 items-center gap-1.5 rounded-md bg-content/8 px-2 text-[11px] hover:bg-content/12 disabled:opacity-40 active:scale-[0.97] ${loadError ? "text-red-400" : "text-content/65"}`}
        >
          <RefreshCw className="size-3.5" />
          <span>Refresh</span>
        </button>
      </div>
      {error && (
        <p role="alert" className="break-words text-[12px] text-red-400">
          {error}
        </p>
      )}
      {cowError && (
        <p role="alert" className="break-words text-[12px] text-red-400">
          {cowError}
        </p>
      )}
      {!project ? (
        <p className="text-[12px] text-content/50">
          Add a project to manage its isolated workspaces.
        </p>
      ) : !data && !cowWorkspaces.length && loadError ? (
        <p role="alert" className="break-words text-[12px] text-red-400">
          {loadError}
        </p>
      ) : !data && !cowWorkspaces.length ? (
        <p className="flex items-center gap-2 text-[12px] text-content/50">
          <Loader className="size-4 animate-spin" />
          Loading workspaces…
        </p>
      ) : !workspaces.length ? (
        <div className="flex flex-col items-center gap-2 rounded-xl border border-stroke px-4 py-8 text-center">
          <FolderTree className="size-5 text-content/35" />
          <p className="text-[13px] font-medium">No isolated workspaces</p>
          <p className="text-[12px] text-content/50">
            Create a worktree to work on another branch in a separate folder.
          </p>
        </div>
      ) : (
        <div className="divide-y divide-stroke overflow-hidden rounded-xl border border-stroke">
          {workspaces.map((tree) => {
            const count = worktreeSessionIds(tree, liveSessions).length;
            const kind = tree.cowId ? "copy-on-write workspace" : "worktree";
            const Icon = tree.cowId ? Copy : FolderTree;
            const blocked = tree.locked
              ? "Unlock this worktree in Git first"
              : !tree.branch
                ? "Create a branch before deleting this detached worktree"
                : undefined;
            return (
              <div key={tree.path} className="flex items-start gap-3 p-4">
                <Icon className="mt-0.5 size-4 shrink-0 text-content/45" />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[13px] font-medium">
                      {projectName(tree.path)}
                      {tree.cowId && (
                        <span className="ml-2 text-[10px] text-content/40">
                          Copy-on-write
                        </span>
                      )}
                    </span>
                    {pathKey(tree.path) === pathKey(project) && (
                      <span className="text-[10px] text-content/40">
                        Selected project folder
                      </span>
                    )}
                  </div>
                  <p className="mt-1 break-all text-[11px] text-content/40">
                    {prettyCwd(tree.path)}
                  </p>
                  <p className="mt-2 flex items-center gap-1.5 text-[11px] text-content/55">
                    <GitBranch className="size-3 shrink-0" />
                    <span className="min-w-0 break-all">
                      {tree.branch
                        ? `Current branch: ${tree.branch}`
                        : `Detached at ${tree.head.slice(0, 7)}`}
                    </span>
                  </p>
                  <p className="mt-2 flex flex-wrap gap-x-3 text-[11px] text-content/55">
                    <span>
                      {count} session{count === 1 ? "" : "s"} in this {kind}
                    </span>
                    <span className={tree.dirty ? "text-amber-400" : ""}>
                      {tree.missing
                        ? "Missing folder"
                        : tree.dirty == null
                          ? "Status unavailable"
                          : tree.dirty
                            ? "Uncommitted changes"
                            : "Clean"}
                    </span>
                    {!!tree.unpushed && (
                      <span>
                        {tree.unpushed} unpublished commit
                        {tree.unpushed === 1 ? "" : "s"}
                      </span>
                    )}
                    {tree.locked && <span>Locked</span>}
                  </p>
                </div>
                <button
                  type="button"
                  disabled={tree.missing}
                  aria-label={`Reveal ${tree.branch ?? "worktree"}`}
                  title="Reveal folder"
                  onClick={() =>
                    void revealPath(tree.path).catch((e) => setError(String(e)))
                  }
                  className="rounded-md p-1.5 text-content/40 hover:bg-content/8 hover:text-content disabled:opacity-30"
                >
                  <FolderOpen className="size-4" />
                </button>
                <button
                  type="button"
                  disabled={!!blocked || refreshingAfterFailure || !!loadError}
                  aria-label={`Delete ${tree.branch ?? "worktree"}`}
                  title={blocked ?? `Delete ${kind}`}
                  onClick={() => {
                    setError(undefined);
                    setDeleting(tree);
                  }}
                  className="rounded-md p-1.5 text-content/40 hover:bg-red-500/10 hover:text-red-400 disabled:opacity-25"
                >
                  <Trash2 className="size-4" />
                </button>
              </div>
            );
          })}
        </div>
      )}
      {data && (
        <p className="break-all text-[11px] text-content/40">
          New worktrees are created in {prettyCwd(data.defaultRoot)}.
        </p>
      )}
      {creating && (
        <CreateWorktreeDialog
          cwd={project}
          baseCwd={project}
          defaultRoot={data?.defaultRoot}
          onCreated={() => {
            setCreating(false);
            refresh();
          }}
          onCancel={() => setCreating(false)}
        />
      )}
      {deleting && (
        <DeleteWorktreeDialog
          cwd={project}
          tree={deleting}
          copyOnWrite={!!deleting.cowId}
          sessionCount={worktreeSessionIds(deleting, liveSessions).length}
          onRemove={async (cwd, path, force, deleteSessions) => {
            // Check predictable blockers before any conversation is destroyed.
            await onCheckRemove(cwd, path, force);
            const sessionIds = worktreeSessionIds(deleting, liveSessions);
            let sessionsDeleted = false;
            try {
              if (sessionIds.length && deleteSessions) {
                if (!onDeleteSessions) {
                  throw new Error(
                    "Sessions still use this worktree and could not be deleted.",
                  );
                }
                if (!(await onDeleteSessions(sessionIds))) {
                  throw new Error(
                    "Some sessions could not be deleted, so the worktree was kept.",
                  );
                }
                sessionsDeleted = true;
              }
              await onRemove(cwd, path, force, !deleteSessions);
            } catch (error) {
              const failure = sessionsDeleted
                ? new Error(
                    `The sessions were deleted, but the worktree was kept. ${String(error)}`,
                  )
                : error;
              // A partial deletion invalidates the dialog's saved session IDs.
              // Require a fresh listing before another destructive attempt.
              setDeleting(undefined);
              setError(String(failure));
              setRefreshingAfterFailure(true);
              await refresh();
              setRefreshingAfterFailure(false);
              throw failure;
            }
          }}
          onClose={() => setDeleting(undefined)}
          onDeleted={() => {
            if (isEqualOrInside(project, deleting.path)) {
              const main = data?.worktrees.find((tree) => tree.isMain);
              if (main) setProject(main.path);
            }
            setDeleting(undefined);
            refresh();
            setCowRefresh((value) => value + 1);
          }}
        />
      )}
    </div>
  );
}
