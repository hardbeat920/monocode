import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Check,
  ChevronDown,
  ChevronRight,
  CloudUpload,
  ExternalLink,
  FileDiff,
  FolderTree,
  GitBranch,
  GitPullRequest,
  ListBullet,
  Loader,
  Minus,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Undo2,
  WandSparkles,
  X,
} from "../../../shared/ui/icons";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import {
  GitHistoryGraph,
  GraphResizeSash,
  GRAPH_PANEL_DEFAULT,
  GRAPH_PANEL_MIN,
  loadGraphPanelHeight,
  saveGraphPanelHeight,
} from "./GitHistoryGraph";
import {
  basename,
  gitCommit,
  gitDiffIndex,
  gitDiscardAll,
  gitDiscardFile,
  gitHeadMessage,
  gitPrCreate,
  gitPrStatus,
  gitPull,
  gitPush,
  gitRangeContext,
  gitStageAll,
  gitStageFile,
  gitSync,
  gitUnstageAll,
  gitUnstageFile,
  notifyGitChanged,
  subscribeGitChanged,
  type GitChangedFile,
  type GitDiffIndex,
  type GitFileDiffKind,
  type GitHistoryCommit,
  type GitPr,
} from "../../../platform/tauri/fs";
import type { HarnessId } from "../../sessions/model/session";
import { recordInboxSelfActivity } from "../../inbox/model/inboxSelfActivity";
import {
  loadChangesView,
  saveChangesView,
  type ChangesView,
} from "../../settings/model/appearance";
import {
  generateCommitMessage,
  generatePrContent,
} from "../../../integrations/harness";
import { invalidateWatchedFiles } from "../../files/model/fileWatch";
import { MOD } from "../../../platform/tauri/platform";
import { applyProjectDiffStats } from "../hooks/useProjectDiffStats";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { isRemoteProjectPath } from "../../projects/model/recents";
import {
  buildChangeTree,
  visibleChangeOrder,
  type ChangeDir,
} from "../model/changeTree";
import { useMultiSelection } from "../../../shared/hooks/useMultiSelection";
import type { SelectMode } from "../../../shared/lib/multiSelection";
import { runSequential } from "../../../shared/lib/concurrent";

const GIT_POLL_MS = 2000;

function confirmNative(message: string, okLabel?: string): Promise<boolean> {
  return ask(message, {
    title: "MonoCode",
    kind: "warning",
    ...(okLabel ? { okLabel } : {}),
  });
}

let stagedOpen = true;
let changesOpen = true;
let graphOpen = true;
let changesView: ChangesView = loadChangesView();
/** Folders the user collapsed in tree view, keyed `<kind>:<dir>`. */
const collapsedDirs = new Set<string>();
const indexByCwd = new Map<string, GitDiffIndex>();
const prByCwd = new Map<string, GitPr | null>();
const NO_PATHS: ReadonlySet<string> = new Set();

type AmendTarget = { branch: string | null; head: string | null };

type Props = {
  cwd: string;
  enabled: boolean;
  textHarness?: HarnessId;
  selectedPath?: string;
  selectedKind?: GitFileDiffKind;
  selectedSha?: string;
  onOpenFile: (path: string, kind: GitFileDiffKind, pin?: boolean) => void;
  onOpenAllChanges: (kind: GitFileDiffKind) => void;
  onOpenCommit: (commit: GitHistoryCommit, pin?: boolean) => void;
};

export function GitChangesPanel({
  cwd,
  enabled,
  textHarness,
  selectedPath,
  selectedKind,
  selectedSha,
  onOpenFile,
  onOpenAllChanges,
  onOpenCommit,
}: Props) {
  const { index, reload } = useDiffIndex(cwd, enabled);
  const files = index?.files ?? [];
  const paneRef = useRef<HTMLDivElement>(null);
  const branchMenuRef = useRef<HTMLDivElement>(null);
  const [branchMenuOpen, setBranchMenuOpen] = useState(false);
  // Shared across the header and the changed-files list so no two Git
  // mutations ever run against the same checkout at once.
  const [busy, setBusy] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [graphHeight, setGraphHeight] = useState(loadGraphPanelHeight);
  const [graphExpanded, setGraphExpanded] = useState(graphOpen);

  useEffect(() => {
    if (!status) return;
    const timer = window.setTimeout(() => setStatus(null), 4000);
    return () => window.clearTimeout(timer);
  }, [status]);

  useEffect(() => {
    if (!branchMenuOpen) return;
    const onPointer = (event: PointerEvent) => {
      if (!branchMenuRef.current?.contains(event.target as Node)) {
        setBranchMenuOpen(false);
      }
    };
    window.addEventListener("pointerdown", onPointer);
    return () => window.removeEventListener("pointerdown", onPointer);
  }, [branchMenuOpen]);

  const canPull = Boolean(index?.remote) && Boolean(index?.upstream);

  const pull = async () => {
    if (!canPull) return;
    setStatus(null);
    setBusy("pull");
    try {
      await gitPull(cwd);
      reload();
      notifyGitChanged();
      invalidateWatchedFiles();
      setStatus("Pull complete");
    } catch (error) {
      window.alert(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(null);
      setBranchMenuOpen(false);
    }
  };

  useLayoutEffect(() => {
    const pane = paneRef.current;
    if (!pane || pane.clientHeight < GRAPH_PANEL_MIN + 160) return;
    const max = pane.clientHeight - 160;
    if (graphHeight > max) {
      setGraphHeight(max);
      saveGraphPanelHeight(max);
    }
  }, [graphHeight]);

  if (!cwd || cwd === "~") {
    return (
      <p className="px-3 py-2 text-[12px] text-content/50">No project folder</p>
    );
  }

  return (
    <div
      ref={paneRef}
      className="flex h-full min-h-0 flex-1 flex-col overflow-hidden"
    >
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-stroke px-3">
        <span className="text-[12px] font-medium text-content">Changes</span>
        {status ? (
          <span role="status" className="text-[11px] text-content/50">
            {status}
          </span>
        ) : null}
        {index?.branch ? (
          <div
            ref={branchMenuRef}
            className="relative ml-auto flex min-w-0 items-center gap-1"
          >
            <span className="flex min-w-0 items-center gap-1 text-[11px] text-content/50">
              <GitBranch className="size-3 shrink-0" strokeWidth={1.75} />
              <span className="min-w-0 truncate">{index.branch}</span>
              {index.ahead > 0 ? (
                <span className="shrink-0 tabular-nums text-content/40">
                  ↑{index.ahead}
                </span>
              ) : null}
              {index.behind > 0 ? (
                <span className="shrink-0 tabular-nums text-content/40">
                  ↓{index.behind}
                </span>
              ) : null}
            </span>
            <button
              type="button"
              aria-haspopup="menu"
              aria-label="Branch actions"
              aria-expanded={branchMenuOpen}
              disabled={busy !== null}
              onClick={() => setBranchMenuOpen((open) => !open)}
              className="grid size-5 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/10 hover:text-content disabled:opacity-40 aria-expanded:bg-content/10 aria-expanded:text-content"
            >
              {busy === "pull" ? (
                <Loader className="size-3.5 animate-spin" strokeWidth={1.75} />
              ) : (
                <MoreHorizontal className="size-4" strokeWidth={2} />
              )}
            </button>
            {branchMenuOpen ? (
              <div
                role="menu"
                aria-label="Branch actions"
                className="absolute top-full right-0 z-30 mt-1 min-w-36 rounded-md border border-content/10 bg-background-base py-1 shadow-lg"
              >
                <button
                  type="button"
                  role="menuitem"
                  disabled={busy !== null || !canPull}
                  title={
                    canPull
                      ? undefined
                      : "This branch needs a remote and upstream before it can pull"
                  }
                  onClick={() => void pull()}
                  className="flex h-7 w-full items-center gap-2 px-3 text-left text-[12px] text-content hover:bg-content/10 disabled:opacity-40"
                >
                  {busy === "pull" ? (
                    <Loader
                      className="size-3.5 animate-spin"
                      strokeWidth={1.75}
                    />
                  ) : (
                    <RefreshCw className="size-3.5" strokeWidth={1.75} />
                  )}
                  {busy === "pull" ? "Pulling…" : "Pull"}
                </button>
              </div>
            ) : null}
          </div>
        ) : (
          <span className="ml-auto" />
        )}
      </header>
      <ChangedFiles
        cwd={cwd}
        textHarness={textHarness}
        index={index}
        files={files}
        selected={selectedPath}
        selectedKind={selectedKind}
        enabled={enabled}
        fill
        busy={busy}
        setBusy={setBusy}
        onOpenFile={onOpenFile}
        onOpenAllChanges={onOpenAllChanges}
        onMutated={(paths) => {
          reload();
          notifyGitChanged();
          invalidateWatchedFiles(paths);
          window.setTimeout(() => invalidateWatchedFiles(paths), 150);
        }}
      />
      {graphExpanded ? (
        <GraphResizeSash
          height={graphHeight}
          onHeightPaint={setGraphHeight}
          onHeightCommit={(next) => {
            setGraphHeight(next);
            saveGraphPanelHeight(next);
          }}
          maxHeight={() => {
            const pane = paneRef.current;
            if (!pane) return GRAPH_PANEL_DEFAULT * 2;
            return Math.max(GRAPH_PANEL_MIN, pane.clientHeight - 160);
          }}
        />
      ) : null}
      <div
        className={`shrink-0 overflow-hidden border-t border-stroke ${
          graphExpanded ? "min-h-0" : "h-7"
        }`}
        style={graphExpanded ? { height: graphHeight } : undefined}
      >
        <GitHistoryGraph
          cwd={cwd}
          enabled={enabled}
          expanded={graphExpanded}
          selectedSha={selectedSha}
          onToggleExpanded={() => {
            graphOpen = !graphExpanded;
            setGraphExpanded(graphOpen);
          }}
          onOpenCommit={onOpenCommit}
        />
      </div>
    </div>
  );
}

function ChangedFiles({
  cwd,
  textHarness,
  index,
  files,
  selected,
  selectedKind,
  enabled,
  fill,
  busy,
  setBusy,
  onOpenFile,
  onOpenAllChanges,
  onMutated,
}: {
  cwd: string;
  textHarness?: HarnessId;
  index: GitDiffIndex | null;
  files: GitChangedFile[];
  selected?: string;
  selectedKind?: GitFileDiffKind;
  enabled: boolean;
  fill: boolean;
  busy: string | null;
  setBusy: (value: string | null) => void;
  onOpenFile: (path: string, kind: GitFileDiffKind, pin?: boolean) => void;
  onOpenAllChanges: (kind: GitFileDiffKind) => void;
  onMutated: (paths?: string[]) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const menuRef = useRef<HTMLDivElement>(null);
  const messageRef = useRef<HTMLTextAreaElement>(null);
  const generateAbortRef = useRef<AbortController | null>(null);
  const [message, setMessage] = useState("");
  const [amendTarget, setAmendTarget] = useState<AmendTarget | null>(null);
  const amend = amendTarget !== null;
  const [menuOpen, setMenuOpen] = useState(false);
  const [stagedExpanded, setStagedExpanded] = useState(stagedOpen);
  const [changesExpanded, setChangesExpanded] = useState(changesOpen);
  const [view, setView] = useState<ChangesView>(changesView);
  const { pr, reload: reloadPr } = usePrStatus(cwd, index?.branch);
  const staged = useMemo(() => files.filter((file) => file.staged), [files]);
  const unstaged = useMemo(
    () => files.filter((file) => file.unstaged),
    [files],
  );
  const hasRemote = Boolean(index?.remote);
  const hasOpenPr = pr?.state === "open";
  const diverged = (index?.ahead ?? 0) > 0 && (index?.behind ?? 0) > 0;
  const onDefault =
    !!index?.branch &&
    !!index.defaultBranch &&
    index.branch === index.defaultBranch;
  const canGenerate = files.length > 0 && !busy && !isRemoteProjectPath(cwd);
  const canCommit =
    (staged.length > 0 || amend) && message.trim().length > 0 && !busy;
  const canCreatePr =
    hasRemote &&
    !!index?.branch &&
    !!index.defaultBranch &&
    !hasOpenPr &&
    !onDefault &&
    !diverged &&
    files.length === 0 &&
    (index?.aheadOfDefault ?? 0) > 0 &&
    (index?.behind ?? 0) === 0;
  const canViewPr = hasOpenPr && !!pr?.url;
  const canPublish = hasRemote && !index?.upstream;
  const canSync =
    hasRemote &&
    Boolean(index?.upstream) &&
    ((index?.ahead ?? 0) > 0 || (index?.behind ?? 0) > 0);
  const canCommitPush =
    canCommit && hasRemote && !diverged && (!amend || !index?.headPushed);
  const canCommitPushPr = canCommitPush && !hasOpenPr && !onDefault;
  const canEditMessage = (staged.length > 0 || amend) && !busy;
  // Rows a bulk stage/unstage/discard is running on.
  const [busyFiles, setBusyFiles] = useState<ReadonlySet<string>>(NO_PATHS);
  const isBusy = (relative: string) =>
    busy === relative || busyFiles.has(relative);

  // Multi-selection: scope is the section kind, ids are repo-relative paths.
  const visibleOrder = (scope: string): string[] => {
    const open = scope === "staged" ? stagedExpanded : changesExpanded;
    if (!open) return [];
    return visibleChangeOrder(
      scope === "staged" ? staged : unstaged,
      view,
      (path) => collapsedDirs.has(`${scope}:${path}`),
    );
  };
  const {
    selection,
    onRowClick,
    isSelected,
    targetsFor,
    clear: clearSelection,
    prune: pruneSelection,
    onKeyDown: onSelectionKeyDown,
  } = useMultiSelection({
    visibleOrder,
    fallbackAnchor: (scope) =>
      selected && (!selectedKind || selectedKind === scope) ? selected : null,
  });
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const pruneHiddenRef = useRef(() => {});
  // Keeps the selection to rows that are still on screen.
  pruneHiddenRef.current = () => {
    const scope = selectionRef.current.scope;
    if (scope === null) return;
    const visible = new Set(visibleOrder(scope));
    pruneSelection((id) => visible.has(id));
  };
  const pruneHidden = useCallback(() => pruneHiddenRef.current(), []);

  useEffect(() => {
    pruneHidden();
  }, [staged, unstaged, view, stagedExpanded, changesExpanded, pruneHidden]);

  // Opening a file outside the selection (e.g. from elsewhere) drops it.
  useEffect(() => {
    const current = selectionRef.current;
    const inSelection =
      !!selected &&
      current.ids.includes(selected) &&
      (!selectedKind || selectedKind === current.scope);
    if (!inSelection) clearSelection();
  }, [selected, selectedKind, clearSelection]);

  useEffect(() => {
    if (!amendTarget) return;
    if (
      amendTarget.branch === index?.branch &&
      amendTarget.head === index?.head
    ) {
      return;
    }
    setAmendTarget(null);
    setMessage("");
  }, [amendTarget, index?.branch, index?.head]);
  const canOpenMenu = !!index?.branch && !busy;

  useEffect(() => {
    if (!enabled) return;
    const el = messageRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [message, enabled]);

  useEffect(
    () => () => {
      if (generateAbortRef.current) {
        generateAbortRef.current.abort();
        generateAbortRef.current = null;
        setBusy(null);
      }
    },
    [cwd, setBusy],
  );

  useEffect(() => {
    if (!menuOpen) return;
    const onPointer = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("pointerdown", onPointer);
    return () => window.removeEventListener("pointerdown", onPointer);
  }, [menuOpen]);

  const toggleView = () => {
    changesView = view === "tree" ? "list" : "tree";
    saveChangesView(changesView);
    setView(changesView);
  };

  const fail = (error: unknown) => {
    window.alert(error instanceof Error ? error.message : String(error));
  };

  const recordPrActivity = (number = pr?.number) => {
    if (!number) return;
    recordInboxSelfActivity({
      provider: "github",
      kind: "pr",
      number,
      projectPath: cwd,
    });
  };

  const confirmDefault = async (kind: "push" | "pr") => {
    if (!onDefault || !index?.branch) return true;
    const branch = index.branch;
    return confirmNative(
      kind === "pr"
        ? `Create a pull request from default branch "${branch}"?`
        : `Push to default branch "${branch}"?`,
    );
  };

  const run = async (
    file: GitChangedFile,
    action: "stage" | "unstage" | "discard",
  ) => {
    if (busy) return;
    if (action === "discard") {
      const name = basename(file.relative);
      const untracked = file.status === "untracked";
      const ok = await confirmNative(
        untracked
          ? `Delete untracked file ${name}?`
          : `Discard changes in ${name}? This cannot be undone.`,
        untracked ? "Delete" : "Discard",
      );
      if (!ok) return;
    }
    setBusy(file.relative);
    try {
      if (action === "stage") await gitStageFile(cwd, file.relative);
      else if (action === "unstage") await gitUnstageFile(cwd, file.relative);
      else await gitDiscardFile(cwd, file.relative);
      onMutated([file.path]);
    } catch (error) {
      fail(error);
    } finally {
      setBusy(null);
    }
  };

  /** Runs a row action on the selection when the row is in it, else on the row. */
  const runTargets = (
    file: GitChangedFile,
    kind: GitFileDiffKind,
    action: "stage" | "unstage" | "discard",
  ) => {
    const ids = new Set(targetsFor(file.relative, kind));
    if (ids.size <= 1) return run(file, action);
    const list = kind === "staged" ? staged : unstaged;
    const byPath = new Map(list.map((f) => [f.relative, f]));
    const targets = visibleOrder(kind)
      .filter((id) => ids.has(id))
      .flatMap((id) => byPath.get(id) ?? []);
    return runMany(targets, action);
  };

  const runMany = async (
    targets: GitChangedFile[],
    action: "stage" | "unstage" | "discard",
  ) => {
    if (busy || targets.length === 0) return;
    if (action === "discard") {
      const n = targets.length;
      const untracked = targets.filter((f) => f.status === "untracked").length;
      const ok = await confirmNative(
        `Discard changes in ${n} files? This cannot be undone.${
          untracked
            ? ` ${untracked} untracked file${untracked === 1 ? "" : "s"} will be deleted.`
            : ""
        }`,
        "Discard",
      );
      if (!ok) return;
    }
    setBusy("files");
    setBusyFiles(new Set(targets.map((f) => f.relative)));
    try {
      // One at a time: git commands contend for the index lock.
      const { done, failed } = await runSequential(targets, (f) =>
        action === "stage"
          ? gitStageFile(cwd, f.relative)
          : action === "unstage"
            ? gitUnstageFile(cwd, f.relative)
            : gitDiscardFile(cwd, f.relative),
      );
      if (done.length) onMutated(done.map((f) => f.path));
      if (failed.length) {
        const first = failed[0].error;
        const reason = first instanceof Error ? first.message : String(first);
        fail(`${reason} (${failed.length} of ${targets.length} files failed)`);
      }
    } finally {
      setBusy(null);
      setBusyFiles(NO_PATHS);
    }
  };

  const runAll = async (action: "stage" | "unstage" | "discard") => {
    if (busy) return;
    if (action === "discard") {
      const n = unstaged.length;
      if (n === 0) return;
      const only = unstaged[0];
      const untrackedOnly = n === 1 && only?.status === "untracked";
      const ok = await confirmNative(
        untrackedOnly
          ? `Delete untracked file ${basename(only.relative)}?`
          : n === 1 && only
            ? `Discard changes in ${basename(only.relative)}? This cannot be undone.`
            : `Discard all unstaged changes in ${n} files? This cannot be undone.`,
        untrackedOnly ? "Delete" : "Discard",
      );
      if (!ok) return;
    }
    setBusy(action);
    try {
      if (action === "stage") await gitStageAll(cwd);
      else if (action === "unstage") await gitUnstageAll(cwd);
      else await gitDiscardAll(cwd);
      onMutated(
        action === "discard" ? unstaged.map((file) => file.path) : undefined,
      );
    } catch (error) {
      fail(error);
    } finally {
      setBusy(null);
    }
  };

  const generate = async () => {
    if (!canGenerate || generateAbortRef.current) return;
    const controller = new AbortController();
    generateAbortRef.current = controller;
    setBusy("generate");
    try {
      const generated = await generateCommitMessage(
        cwd,
        textHarness,
        controller.signal,
      );
      if (!controller.signal.aborted) setMessage(generated);
    } catch (error) {
      if (!controller.signal.aborted) fail(error);
    } finally {
      if (generateAbortRef.current === controller) {
        generateAbortRef.current = null;
        setBusy(null);
      }
    }
  };

  const cancelGenerate = () => {
    generateAbortRef.current?.abort();
    generateAbortRef.current = null;
    setBusy(null);
  };

  const toggleAmend = async () => {
    setMenuOpen(false);
    if (amend) {
      setAmendTarget(null);
      return;
    }
    try {
      const headMessage = await gitHeadMessage(cwd);
      if (!message.trim()) setMessage(headMessage);
      setAmendTarget({
        branch: index?.branch ?? null,
        head: index?.head ?? null,
      });
    } catch (error) {
      fail(error);
    }
  };

  const confirmAmend = async () => {
    if (!amend || !index?.headPushed) return true;
    return confirmNative(
      "Amend a commit that is already pushed? MonoCode cannot push the result. You will need a force push from the terminal.",
      "Amend",
    );
  };

  const commit = async (push: boolean, createPr = false) => {
    if (!canCommit) return;
    if (
      (push || createPr) &&
      !(await confirmDefault(createPr ? "pr" : "push"))
    ) {
      return;
    }
    if (!(await confirmAmend())) return;
    setBusy(createPr ? "pr" : "commit");
    setMenuOpen(false);
    try {
      await gitCommit(cwd, message, amend);
      if (push || createPr) {
        await gitPush(cwd);
        recordPrActivity();
      }
      setMessage("");
      setAmendTarget(null);
      onMutated();
      if (createPr) {
        await openCreatedPr();
        reloadPr();
      }
    } catch (error) {
      fail(error);
      onMutated();
    } finally {
      setBusy(null);
    }
  };

  const sync = async () => {
    if (!index || !(canSync || canPublish)) return;
    const pushesCommits = index.ahead > 0;
    setBusy("sync");
    try {
      await gitSync(cwd);
      if (pushesCommits) recordPrActivity();
      onMutated();
      reloadPr();
    } catch (error) {
      fail(error);
      onMutated();
    } finally {
      setBusy(null);
    }
  };

  const openCreatedPr = async () => {
    const content = isRemoteProjectPath(cwd)
      ? await remotePrContent(cwd)
      : await generatePrContent(cwd, textHarness);
    if (!content) throw new Error("Could not prepare pull request content");
    const url = await gitPrCreate(
      cwd,
      content.title,
      content.body,
      content.base,
      content.head,
    );
    const number = Number(/\/pull\/(\d+)(?:[/?#]|$)/.exec(url)?.[1]);
    if (Number.isInteger(number) && number > 0) recordPrActivity(number);
    await openUrl(url.trim());
  };

  const createPr = async () => {
    if (!canCreatePr) return;
    if (!(await confirmDefault("pr"))) return;
    setBusy("pr");
    try {
      if ((index?.ahead ?? 0) > 0) await gitPush(cwd);
      await openCreatedPr();
      onMutated();
      reloadPr();
    } catch (error) {
      fail(error);
      onMutated();
    } finally {
      setBusy(null);
    }
  };

  return (
    <aside
      className={`flex min-h-0 min-w-0 flex-col ${fill ? "flex-1" : "shrink-0"}`}
    >
      <div className="shrink-0 border-b border-stroke p-2">
        <div className="relative">
          <textarea
            ref={messageRef}
            rows={1}
            value={message}
            placeholder={
              amend
                ? `Amend message (${MOD}↩ to amend)`
                : `Message (${MOD}↩ to commit)`
            }
            disabled={!canEditMessage}
            onChange={(event) => setMessage(event.target.value)}
            onKeyDown={(event) => {
              if (
                (event.metaKey || event.ctrlKey) &&
                event.key === "Enter" &&
                canCommit
              ) {
                event.preventDefault();
                void commit(false);
              }
            }}
            className="max-h-40 w-full resize-none overflow-y-auto rounded-md bg-content/10 py-1 pr-8 pl-2 text-[13px] leading-5 text-content outline-none placeholder:text-content/35 disabled:opacity-40"
          />
          <button
            type="button"
            title={
              busy === "generate"
                ? "Cancel commit message generation"
                : "Generate commit message"
            }
            aria-label={
              busy === "generate"
                ? "Cancel commit message generation"
                : "Generate commit message"
            }
            disabled={busy !== "generate" && !canGenerate}
            onClick={() =>
              busy === "generate" ? cancelGenerate() : void generate()
            }
            className="group absolute top-1 right-1 grid size-5 place-items-center rounded-md bg-content/10 text-content hover:bg-content/20 hover:text-content disabled:opacity-40"
          >
            {busy === "generate" ? (
              <>
                <Loader
                  className="size-3.5 animate-spin group-hover:hidden group-focus-visible:hidden"
                  strokeWidth={1.75}
                />
                <X
                  className="hidden size-3.5 group-hover:block group-focus-visible:block"
                  strokeWidth={1.75}
                />
              </>
            ) : (
              <WandSparkles className="size-3" strokeWidth={1} />
            )}
          </button>
        </div>
        <div ref={menuRef} className="relative mt-1.5 flex">
          <button
            type="button"
            disabled={!canCommit}
            onClick={() => void commit(false)}
            className={`flex h-7 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-l-md text-[12px] font-medium ${
              canCommit
                ? "bg-content text-background-base"
                : "bg-content/40 text-background-base"
            }`}
          >
            <Check className="size-3.5" strokeWidth={2} />
            {amend ? "Amend Commit" : "Commit"}
          </button>

          <button
            type="button"
            title="Commit options"
            aria-label="Commit options"
            aria-expanded={menuOpen}
            disabled={!canOpenMenu}
            onClick={() => setMenuOpen((open) => !open)}
            className={`grid h-7 w-7 shrink-0 place-items-center rounded-r-md border-l border-background-base/10 ${
              canCommit
                ? "bg-content text-background-base hover:bg-content/80"
                : "bg-content/40 text-background-base hover:bg-content"
            } disabled:pointer-events-none aria-expanded:bg-content aria-expanded:text-background-base`}
          >
            <ChevronDown className="size-3.5" strokeWidth={2} />
          </button>
          {menuOpen ? (
            <div
              role="menu"
              aria-label="Commit options"
              className="absolute top-full right-0 z-30 mt-1 min-w-48 rounded-md border border-content/10 bg-background-base py-1 shadow-lg"
            >
              <button
                type="button"
                role="menuitem"
                disabled={!canCommitPush}
                onClick={() => void commit(true)}
                className="flex h-7 w-full items-center px-3 text-left text-[12px] text-content hover:bg-content/10 disabled:opacity-40"
              >
                Commit & Push
              </button>
              <button
                type="button"
                role="menuitem"
                disabled={!canCommitPushPr}
                onClick={() => void commit(true, true)}
                className="flex h-7 w-full items-center px-3 text-left text-[12px] text-content hover:bg-content/10 disabled:opacity-40"
              >
                Commit, Push & Create PR
              </button>
              <div className="my-1 border-t border-content/10" />
              <button
                type="button"
                role="menuitemcheckbox"
                aria-checked={amend}
                onClick={() => void toggleAmend()}
                className="flex h-7 w-full items-center justify-between gap-2 px-3 text-left text-[12px] text-content hover:bg-content/10"
              >
                Amend Last Commit
                <span className="grid size-3.5 shrink-0 place-items-center">
                  {amend ? (
                    <Check className="size-3.5" strokeWidth={2} />
                  ) : null}
                </span>
              </button>
            </div>
          ) : null}
        </div>
        {index ? (
          <GitSyncActions
            index={index}
            pr={pr}
            busy={busy}
            hasRemote={hasRemote}
            hasOpenPr={hasOpenPr}
            onDefault={onDefault}
            canSync={canSync}
            canPublish={canPublish}
            canCreatePr={canCreatePr}
            canViewPr={canViewPr}
            onSync={() => void sync()}
            onCreatePr={() => void createPr()}
            onViewPr={() => {
              if (pr?.url) void openUrl(pr.url);
            }}
          />
        ) : null}
      </div>
      <div
        ref={lockOverscroll}
        // Focusable so Escape / Mod+A reach it after clicking a row (WebKit
        // doesn't focus buttons on click).
        tabIndex={-1}
        onKeyDown={(event) => onSelectionKeyDown(event)}
        className="min-h-0 flex-1 overflow-y-auto overscroll-none py-1 outline-none"
      >
        {files.length === 0 ? (
          <p className="px-3 py-2 text-[12px] text-content/45">
            {index
              ? index.ahead > 0 || index.behind > 0
                ? syncStatusLabel(index)
                : "No uncommitted changes"
              : "Loading changes…"}
          </p>
        ) : (
          <>
            {staged.length > 0 ? (
              <FileSection
                title="Staged Changes"
                count={staged.length}
                open={stagedExpanded}
                onToggle={() => {
                  stagedOpen = !stagedExpanded;
                  setStagedExpanded(stagedOpen);
                }}
                view={view}
                onToggleView={toggleView}
                headerActions={[
                  {
                    title: "Open All Changes",
                    icon: <FileDiff className="size-3.5" strokeWidth={1.75} />,
                    onClick: () => onOpenAllChanges("staged"),
                  },
                  {
                    title: "Unstage All Changes",
                    icon: <Minus className="size-3.5" strokeWidth={1.75} />,
                    onClick: () => void runAll("unstage"),
                  },
                ]}
              >
                <ChangeList
                  files={staged}
                  view={view}
                  kind="staged"
                  selected={selected}
                  selectedKind={selectedKind}
                  isBusy={isBusy}
                  isSelected={isSelected}
                  onRowClick={onRowClick}
                  onCollapse={pruneHidden}
                  onOpenFile={onOpenFile}
                  onAction={(file, action) =>
                    void runTargets(file, "staged", action)
                  }
                />
              </FileSection>
            ) : null}
            {unstaged.length > 0 ? (
              <FileSection
                title="Changes"
                count={unstaged.length}
                open={changesExpanded}
                onToggle={() => {
                  changesOpen = !changesExpanded;
                  setChangesExpanded(changesOpen);
                }}
                view={view}
                onToggleView={toggleView}
                headerActions={[
                  {
                    title: "Open All Changes",
                    icon: <FileDiff className="size-3.5" strokeWidth={1.75} />,
                    onClick: () => onOpenAllChanges("unstaged"),
                  },
                  {
                    title: "Discard All Changes",
                    icon: <Undo2 className="size-3.5" strokeWidth={1.75} />,
                    onClick: () => void runAll("discard"),
                  },
                  {
                    title: "Stage All Changes",
                    icon: <Plus className="size-3.5" strokeWidth={1.75} />,
                    onClick: () => void runAll("stage"),
                  },
                ]}
              >
                <ChangeList
                  files={unstaged}
                  view={view}
                  kind="unstaged"
                  selected={selected}
                  selectedKind={selectedKind}
                  isBusy={isBusy}
                  isSelected={isSelected}
                  onRowClick={onRowClick}
                  onCollapse={pruneHidden}
                  onOpenFile={onOpenFile}
                  onAction={(file, action) =>
                    void runTargets(file, "unstaged", action)
                  }
                />
              </FileSection>
            ) : null}
          </>
        )}
      </div>
    </aside>
  );
}

function usePrStatus(
  cwd: string,
  branch: string | null | undefined,
): { pr: GitPr | null; reload: () => void } {
  const [pr, setPr] = useState<GitPr | null>(() => cachedPr(cwd, branch));
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((value) => value + 1), []);

  useEffect(() => {
    if (!cwd || cwd === "~" || !branch) {
      setPr(null);
      return;
    }
    let cancelled = false;
    const load = () => {
      void gitPrStatus(cwd)
        .then((next) => {
          if (cancelled) return;
          prByCwd.set(cwd, next);
          setPr(next);
        })
        .catch(() => {
          if (cancelled) return;
          prByCwd.set(cwd, null);
          setPr(null);
        });
    };
    load();
    const onResume = () => load();
    window.addEventListener("focus", onResume);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", onResume);
    };
  }, [branch, cwd, nonce]);

  return { pr, reload };
}

function cachedPr(
  cwd: string,
  branch: string | null | undefined,
): GitPr | null {
  if (!cwd || cwd === "~" || !branch) return null;
  return prByCwd.get(cwd) ?? null;
}

function syncStatusLabel(index: GitDiffIndex): string {
  if (index.ahead > 0 && index.behind > 0) {
    return `Diverged from ${index.upstream ?? "upstream"}`;
  }
  if (index.ahead > 0) {
    const n = index.ahead;
    return `${n} unpushed commit${n === 1 ? "" : "s"}`;
  }
  if (index.behind > 0) {
    const n = index.behind;
    return `${n} incoming commit${n === 1 ? "" : "s"}`;
  }
  return "No files";
}

function GitSyncActions({
  index,
  pr,
  busy,
  hasRemote,
  hasOpenPr,
  onDefault,
  canSync,
  canPublish,
  canCreatePr,
  canViewPr,
  onSync,
  onCreatePr,
  onViewPr,
}: {
  index: GitDiffIndex;
  pr: GitPr | null;
  busy: string | null;
  hasRemote: boolean;
  hasOpenPr: boolean;
  onDefault: boolean;
  canSync: boolean;
  canPublish: boolean;
  canCreatePr: boolean;
  canViewPr: boolean;
  onSync: () => void;
  onCreatePr: () => void;
  onViewPr: () => void;
}) {
  if (!hasRemote) return null;
  const ahead = index.ahead;
  const behind = index.behind;
  const dest =
    index.upstream ?? `${index.remote ?? "origin"}/${index.branch ?? "HEAD"}`;
  const syncing = busy === "sync";
  const syncTitle = syncing
    ? "Synchronizing Changes..."
    : canPublish
      ? index.branch
        ? `Publish Branch "${index.branch}"`
        : "Publish Branch"
      : behind > 0 && ahead > 0
        ? `Pull ${behind} and push ${ahead} commits between ${dest}`
        : behind > 0
          ? `Pull ${behind} commit${behind === 1 ? "" : "s"} from ${dest}`
          : `Push ${ahead} commit${ahead === 1 ? "" : "s"} to ${dest}`;
  const createTitle = index.defaultBranch
    ? `Create a pull request into ${index.defaultBranch}`
    : "Create pull request";
  const viewTitle = pr?.title
    ? `View PR #${pr.number}: ${pr.title}`
    : "View pull request";
  const btn =
    "flex h-7 w-full min-w-0 items-center justify-center gap-1.5 rounded-md px-2 text-[12px] font-medium disabled:opacity-40";
  const secondary = `${btn} bg-content/10 text-content hover:bg-content/15`;
  const showCreatePr = !hasOpenPr && !onDefault;
  const showViewPr = hasOpenPr;
  if (!canPublish && !canSync && !showCreatePr && !showViewPr) return null;

  return (
    <div className="mt-1.5 flex flex-col gap-1.5">
      {canPublish ? (
        <button
          type="button"
          title={syncTitle}
          disabled={!!busy}
          onClick={onSync}
          className={secondary}
        >
          {syncing ? (
            <Loader
              className="size-3.5 shrink-0 animate-spin"
              strokeWidth={1.75}
            />
          ) : (
            <CloudUpload className="size-3.5 shrink-0" strokeWidth={1.75} />
          )}
          <span className="min-w-0 truncate">Publish Branch</span>
        </button>
      ) : canSync ? (
        <button
          type="button"
          title={syncTitle}
          disabled={!!busy}
          onClick={onSync}
          className={secondary}
        >
          <RefreshCw
            className={`size-3.5 shrink-0 ${syncing ? "animate-spin" : ""}`}
            strokeWidth={1.75}
          />
          <span className="min-w-0 truncate">Sync Changes</span>
          {behind > 0 ? (
            <span className="shrink-0 tabular-nums text-content/55">
              ↓{behind}
            </span>
          ) : null}
          {ahead > 0 ? (
            <span className="shrink-0 tabular-nums text-content/55">
              ↑{ahead}
            </span>
          ) : null}
        </button>
      ) : null}
      {showCreatePr ? (
        <button
          type="button"
          title={createTitle}
          disabled={!canCreatePr || !!busy}
          onClick={onCreatePr}
          className={secondary}
        >
          {busy === "pr" ? (
            <Loader
              className="size-3.5 shrink-0 animate-spin"
              strokeWidth={1.75}
            />
          ) : (
            <GitPullRequest className="size-3.5 shrink-0" strokeWidth={1.75} />
          )}
          Create PR
        </button>
      ) : null}
      {showViewPr ? (
        <button
          type="button"
          title={viewTitle}
          disabled={!canViewPr || !!busy}
          onClick={onViewPr}
          className={secondary}
        >
          <ExternalLink className="size-3.5 shrink-0" strokeWidth={1.75} />
          <span className="min-w-0 truncate">
            {pr?.number ? `View PR #${pr.number}` : "View PR"}
          </span>
        </button>
      ) : null}
    </div>
  );
}

export function FileSection({
  title,
  count,
  open,
  onToggle,
  view,
  onToggleView,
  headerActions,
  children,
}: {
  title: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  view: ChangesView;
  onToggleView: () => void;
  headerActions: { title: string; icon: ReactNode; onClick: () => void }[];
  children: ReactNode;
}) {
  return (
    <div>
      <div className="flex h-7 items-center gap-1 px-1.5">
        <button
          type="button"
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-1 text-left"
        >
          {open ? (
            <ChevronDown
              className="size-3.5 shrink-0 text-content/50"
              strokeWidth={1.75}
            />
          ) : (
            <ChevronRight
              className="size-3.5 shrink-0 text-content/50"
              strokeWidth={1.75}
            />
          )}
          <span className="min-w-0 truncate text-[10px] font-semibold tracking-[0.04em] text-content/55 uppercase">
            {title}
          </span>
          <span className="ml-1 grid h-4 min-w-4 shrink-0 place-items-center rounded-full bg-accent/80 px-1 text-[8px] text-white">
            {count}
          </span>
        </button>
        <IconAction
          title={view === "tree" ? "View as List" : "View as Tree"}
          onClick={onToggleView}
        >
          {view === "tree" ? (
            <ListBullet className="size-3.5" strokeWidth={1.75} />
          ) : (
            <FolderTree className="size-3.5" strokeWidth={1.75} />
          )}
        </IconAction>
        {headerActions.map((action) => (
          <IconAction
            key={action.title}
            title={action.title}
            onClick={action.onClick}
          >
            {action.icon}
          </IconAction>
        ))}
      </div>
      {open ? <ul>{children}</ul> : null}
    </div>
  );
}

async function remotePrContent(cwd: string) {
  const range = await gitRangeContext(cwd);
  const commits = range.commitSummary.trim();
  const firstCommit = commits
    .split(/\r?\n/, 1)[0]
    ?.replace(/^[0-9a-f]+\s+/i, "")
    .trim();
  const title = firstCommit || `Changes on ${range.head}`;
  const body = [
    commits && `## Commits\n\n${commits}`,
    range.diffSummary.trim() && `## Changes\n\n${range.diffSummary.trim()}`,
  ]
    .filter(Boolean)
    .join("\n\n");
  return { title, body: body || title, base: range.base, head: range.head };
}

type ChangeRowProps = {
  files: GitChangedFile[];
  view: ChangesView;
  kind: GitFileDiffKind;
  selected?: string;
  selectedKind?: GitFileDiffKind;
  isBusy: (relative: string) => boolean;
  /** Whether a row is in the multi-selection. */
  isSelected: (relative: string, kind: GitFileDiffKind) => boolean;
  onRowClick: SelectionClick;
  /** A tree folder collapsed, hiding its rows. */
  onCollapse: () => void;
  onOpenFile: (path: string, kind: GitFileDiffKind, pin?: boolean) => void;
  onAction: (
    file: GitChangedFile,
    action: "stage" | "unstage" | "discard",
  ) => void;
};

type SelectionClick = (
  event: MouseEvent,
  relative: string,
  kind: GitFileDiffKind,
) => SelectMode;

export function ChangeList({ files, view, ...rest }: ChangeRowProps) {
  const tree = useMemo(() => buildChangeTree(files), [files]);
  if (view === "tree") {
    return <ChangeDirChildren dir={tree} depth={0} {...rest} />;
  }
  return (
    <>
      {files.map((file) => (
        <ChangeRow
          key={`${rest.kind}:${file.relative}`}
          file={file}
          active={isActive(file, rest.selected, rest.selectedKind, rest.kind)}
          inSelection={rest.isSelected(file.relative, rest.kind)}
          busy={rest.isBusy(file.relative)}
          kind={rest.kind}
          onRowClick={rest.onRowClick}
          onOpenFile={rest.onOpenFile}
          onAction={rest.onAction}
        />
      ))}
    </>
  );
}

function ChangeDirChildren({
  dir,
  depth,
  kind,
  selected,
  selectedKind,
  isBusy,
  isSelected,
  onRowClick,
  onCollapse,
  onOpenFile,
  onAction,
}: Omit<ChangeRowProps, "files" | "view"> & {
  dir: ChangeDir;
  depth: number;
}) {
  return (
    <>
      {dir.dirs.map((child) => (
        <ChangeDirRow
          key={child.path}
          dir={child}
          depth={depth}
          kind={kind}
          selected={selected}
          selectedKind={selectedKind}
          isBusy={isBusy}
          isSelected={isSelected}
          onRowClick={onRowClick}
          onCollapse={onCollapse}
          onOpenFile={onOpenFile}
          onAction={onAction}
        />
      ))}
      {dir.files.map((file) => (
        <ChangeRow
          key={`${kind}:${file.relative}`}
          file={file}
          active={isActive(file, selected, selectedKind, kind)}
          inSelection={isSelected(file.relative, kind)}
          busy={isBusy(file.relative)}
          kind={kind}
          depth={depth}
          onRowClick={onRowClick}
          onOpenFile={onOpenFile}
          onAction={onAction}
        />
      ))}
    </>
  );
}

function ChangeDirRow({
  dir,
  depth,
  kind,
  ...rest
}: Omit<ChangeRowProps, "files" | "view"> & {
  dir: ChangeDir;
  depth: number;
}) {
  const key = `${kind}:${dir.path}`;
  const [open, setOpen] = useState(() => !collapsedDirs.has(key));
  const toggle = () => {
    if (open) collapsedDirs.add(key);
    else collapsedDirs.delete(key);
    setOpen(!open);
    if (open) rest.onCollapse();
  };
  return (
    <li>
      <button
        type="button"
        title={dir.path}
        aria-expanded={open}
        onClick={toggle}
        style={{ paddingLeft: 8 + depth * 12 }}
        className="flex h-7 w-full items-center gap-1.5 pr-2 text-left leading-none text-content hover:bg-content/5"
      >
        <span className="grid size-4 shrink-0 place-items-center text-content/50">
          {open ? (
            <ChevronDown className="size-3.5" strokeWidth={1.75} />
          ) : (
            <ChevronRight className="size-3.5" strokeWidth={1.75} />
          )}
        </span>
        <FileTypeIcon name={dir.name} isDir isOpen={open} size={16} />
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium">
          {dir.name}
        </span>
        <span
          className={`grid w-3.5 shrink-0 place-items-center ${
            dir.status ? statusColor(dir.status) : "text-content/40"
          }`}
          aria-hidden
        >
          <span className="size-1.5 rounded-full bg-current" />
        </span>
      </button>
      {open ? (
        <ul>
          <ChangeDirChildren
            dir={dir}
            depth={depth + 1}
            kind={kind}
            {...rest}
          />
        </ul>
      ) : null}
    </li>
  );
}

function isActive(
  file: GitChangedFile,
  selected: string | undefined,
  selectedKind: GitFileDiffKind | undefined,
  kind: GitFileDiffKind,
): boolean {
  return selected === file.relative && (!selectedKind || selectedKind === kind);
}

function ChangeRow({
  file,
  active,
  inSelection,
  busy,
  kind,
  depth,
  onRowClick,
  onOpenFile,
  onAction,
}: {
  file: GitChangedFile;
  active: boolean;
  inSelection: boolean;
  busy: boolean;
  kind: GitFileDiffKind;
  /** Set in tree view: nesting level, and the folder path moves to the tree. */
  depth?: number;
  onRowClick: SelectionClick;
  onOpenFile: (path: string, kind: GitFileDiffKind, pin?: boolean) => void;
  onAction: (
    file: GitChangedFile,
    action: "stage" | "unstage" | "discard",
  ) => void;
}) {
  const name = basename(file.relative);
  const tree = depth !== undefined;
  const dir = tree ? "" : dirname(file.relative);
  const canOpen = file.status !== "deleted";
  const highlighted = active || inSelection;
  return (
    <li data-selected={inSelection || undefined}>
      <div
        style={tree ? { paddingLeft: 8 + depth * 12 } : undefined}
        className={`group flex h-7 w-full items-center gap-1 pr-2 leading-none ${
          tree ? "" : "pl-2"
        } ${
          highlighted
            ? "bg-selection text-content"
            : "text-content hover:bg-content/5"
        }`}
      >
        <button
          type="button"
          title={file.relative}
          // Keep Shift-click from selecting the row text.
          onMouseDown={(event) => {
            if (event.shiftKey) event.preventDefault();
          }}
          onClick={(event) => {
            const mode = onRowClick(event, file.relative, kind);
            if (mode === "single" && canOpen) onOpenFile(file.path, kind);
          }}
          onDoubleClick={() => {
            if (canOpen) onOpenFile(file.path, kind, true);
          }}
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
        >
          {tree ? <span className="size-4 shrink-0" /> : null}
          <FileTypeIcon name={name} isDir={false} size={16} />
          <span className="min-w-0 flex-1 truncate">
            <span className="text-[13px] font-medium">{name}</span>
            {dir ? (
              <span className="ml-1.5 text-[11px] text-content/40">{dir}</span>
            ) : null}
          </span>
        </button>
        <div
          className={` shrink-0 items-center ${
            highlighted
              ? "flex"
              : "hidden group-focus-within:flex group-hover:flex"
          }`}
        >
          {kind === "unstaged" ? (
            <IconAction
              title="Discard Changes"
              disabled={busy}
              onClick={() => onAction(file, "discard")}
            >
              <Undo2 className="size-3.5" strokeWidth={1.75} />
            </IconAction>
          ) : null}
          {kind === "staged" ? (
            <IconAction
              title="Unstage Changes"
              disabled={busy}
              onClick={() => onAction(file, "unstage")}
            >
              <Minus className="size-3.5" strokeWidth={1.75} />
            </IconAction>
          ) : (
            <IconAction
              title="Stage Changes"
              disabled={busy}
              onClick={() => onAction(file, "stage")}
            >
              <Plus className="size-3.5" strokeWidth={1.75} />
            </IconAction>
          )}
        </div>
        <span
          className={`w-3.5 shrink-0 text-right font-mono text-[11px] font-semibold ${statusColor(file.status)}`}
        >
          {statusLetter(file.status)}
        </span>
      </div>
    </li>
  );
}

function IconAction({
  title,
  disabled,
  onClick,
  children,
}: {
  title: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
      className="grid size-5 place-items-center rounded text-content/55 hover:bg-content/10 hover:text-content disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function dirname(relative: string): string {
  const i = relative.lastIndexOf("/");
  return i > 0 ? relative.slice(0, i) : "";
}

function statusLetter(status: string): string {
  if (status === "untracked") return "U";
  if (status === "added") return "A";
  if (status === "deleted") return "D";
  return "M";
}

function statusColor(status: string): string {
  if (status === "untracked") return "text-sky-400";
  if (status === "added") return "text-emerald-400";
  if (status === "deleted") return "text-red-400";
  return "text-amber-400";
}

function useDiffIndex(
  cwd: string,
  enabled: boolean,
): {
  index: GitDiffIndex | null;
  reload: () => void;
} {
  const [index, setIndex] = useState<GitDiffIndex | null>(() =>
    cachedIndex(cwd),
  );
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((value) => value + 1), []);
  const indexRef = useRef(index);
  indexRef.current = index;

  useEffect(() => {
    if (!enabled || !cwd || cwd === "~") {
      return;
    }
    const cached = cachedIndex(cwd);
    if (cached && !sameIndex(indexRef.current, cached)) {
      indexRef.current = cached;
      setIndex(cached);
    }
    let cancelled = false;
    let inFlight = false;
    let pending = false;

    const load = async () => {
      if (inFlight) {
        pending = true;
        return;
      }
      if (document.hidden && nonce === 0) return;
      inFlight = true;
      try {
        const next = await gitDiffIndex(cwd);
        if (cancelled) return;
        const prev = indexRef.current;
        if (sameIndex(prev, next)) return;
        indexByCwd.set(cwd, next);
        indexRef.current = next;
        setIndex(next);
        applyProjectDiffStats(cwd, {
          files: next.files.length,
          additions: next.additions,
          deletions: next.deletions,
        });
        if (prev) {
          const paths = changedFilePaths(prev, next);
          invalidateWatchedFiles(paths);
          notifyGitChanged();
        }
      } catch {
        if (!cancelled) {
          indexByCwd.delete(cwd);
          setIndex(null);
        }
      } finally {
        inFlight = false;
        if (pending && !cancelled) {
          pending = false;
          void load();
        }
      }
    };

    void load();
    const onResume = () => {
      if (!document.hidden) void load();
    };
    const timer = window.setInterval(onResume, GIT_POLL_MS);
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    const unsubGit = subscribeGitChanged(onResume);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
      unsubGit();
    };
  }, [cwd, enabled, nonce]);

  return { index, reload };
}

function cachedIndex(cwd: string | undefined): GitDiffIndex | null {
  if (!cwd || cwd === "~") return null;
  return indexByCwd.get(cwd) ?? null;
}

function changedFilePaths(prev: GitDiffIndex, next: GitDiffIndex): string[] {
  const paths: string[] = [];
  const seen = new Set<string>();
  const previous = new Map(prev.files.map((file) => [file.relative, file]));
  const current = new Set(next.files.map((file) => file.relative));
  for (const file of next.files) {
    const before = previous.get(file.relative);
    if (
      !before ||
      before.status !== file.status ||
      before.additions !== file.additions ||
      before.deletions !== file.deletions ||
      before.staged !== file.staged ||
      before.unstaged !== file.unstaged
    ) {
      paths.push(file.path);
      seen.add(file.path);
    }
  }
  for (const file of prev.files) {
    if (!current.has(file.relative) && !seen.has(file.path)) {
      paths.push(file.path);
    }
  }
  return paths;
}

function sameIndex(prev: GitDiffIndex | null, next: GitDiffIndex): boolean {
  if (!prev) return false;
  if (
    prev.branch !== next.branch ||
    prev.head !== next.head ||
    prev.additions !== next.additions ||
    prev.deletions !== next.deletions ||
    prev.files.length !== next.files.length ||
    prev.remote !== next.remote ||
    prev.upstream !== next.upstream ||
    prev.defaultBranch !== next.defaultBranch ||
    prev.ahead !== next.ahead ||
    prev.behind !== next.behind ||
    prev.aheadOfDefault !== next.aheadOfDefault ||
    prev.headPushed !== next.headPushed
  ) {
    return false;
  }
  return prev.files.every((file, i) => {
    const other = next.files[i];
    return (
      other &&
      file.relative === other.relative &&
      file.status === other.status &&
      file.additions === other.additions &&
      file.deletions === other.deletions &&
      file.staged === other.staged &&
      file.unstaged === other.unstaged
    );
  });
}
