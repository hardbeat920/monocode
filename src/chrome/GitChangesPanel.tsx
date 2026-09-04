import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Check,
  ChevronDown,
  ChevronRight,
  CloudUpload,
  ExternalLink,
  Folder,
  FolderOpen,
  FolderTree,
  GitBranch,
  GitPullRequest,
  ListView,
  Loader,
  Minus,
  Plus,
  RefreshCw,
  Undo2,
  WandSparkles,
} from "./icons";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { FileTypeIcon } from "./FileTypeIcon";
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
  gitPrCreate,
  gitPrStatus,
  gitPush,
  gitStageAll,
  gitStageFile,
  gitSync,
  gitUnstageAll,
  gitUnstageFile,
  notifyGitChanged,
  subscribeGitChanged,
  type GitChangedFile,
  type GitDiffIndex,
  type GitHistoryCommit,
  type GitPr,
} from "../lib/fs";
import type { HarnessId } from "../lib/session";
import { generateCommitMessage, generatePrContent } from "../lib/harness";
import { invalidateWatchedFiles } from "../lib/fileWatch";
import { MOD } from "../lib/platform";
import { applyProjectDiffStats } from "../hooks/useProjectDiffStats";
import { useLockOverscroll } from "../hooks/useLockOverscroll";

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

type ChangesView = "list" | "tree";

const CHANGES_VIEW_KEY = "monocode.gitChangesView";

let changesView: ChangesView = "list";

function loadChangesView(): ChangesView {
  try {
    const raw = localStorage.getItem(CHANGES_VIEW_KEY);
    if (raw === "list" || raw === "tree") changesView = raw;
  } catch {
    // private mode / no storage: keep the in-memory default
  }
  return changesView;
}

function saveChangesView(next: ChangesView) {
  changesView = next;
  try {
    localStorage.setItem(CHANGES_VIEW_KEY, next);
  } catch {
    // private mode / quota: the in-memory value still applies
  }
}
const indexByCwd = new Map<string, GitDiffIndex>();
const prByCwd = new Map<string, GitPr | null>();

type Props = {
  cwd: string;
  enabled: boolean;
  textHarness?: HarnessId;
  selectedPath?: string;
  selectedSha?: string;
  onOpenFile: (path: string) => void;
  onOpenCommit: (commit: GitHistoryCommit) => void;
};

export function GitChangesPanel({
  cwd,
  enabled,
  textHarness,
  selectedPath,
  selectedSha,
  onOpenFile,
  onOpenCommit,
}: Props) {
  const { index, reload } = useDiffIndex(cwd, enabled);
  const files = index?.files ?? [];
  const paneRef = useRef<HTMLDivElement>(null);
  const [graphHeight, setGraphHeight] = useState(loadGraphPanelHeight);
  const [graphExpanded, setGraphExpanded] = useState(graphOpen);

  useLayoutEffect(() => {
    const pane = paneRef.current;
    if (!pane || pane.clientHeight < GRAPH_PANEL_MIN + 160) return;
    const max = pane.clientHeight - 160;
    if (graphHeight > max) {
      setGraphHeight(max);
      saveGraphPanelHeight(max);
    }
  }, [graphHeight]);
  const [view, setView] = useState<ChangesView>(() => loadChangesView());
  const changeView = (next: ChangesView) => {
    saveChangesView(next);
    setView(next);
  };

  if (!cwd || cwd === "~") {
    return <p className="px-3 py-2 text-[12px] text-content/50">No project folder</p>;
  }

  return (
    <div ref={paneRef} className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-content/10 px-3">
        <ChangesViewToggle view={view} onChange={changeView} />
        {index?.branch ? (
          <span className="ml-auto flex min-w-0 items-center gap-1 text-[11px] text-content/50">
            <GitBranch className="size-3 shrink-0" strokeWidth={1.75} />
            <span className="min-w-0 truncate">{index.branch}</span>
            {index.ahead > 0 ? (
              <span className="shrink-0 tabular-nums text-content/40">↑{index.ahead}</span>
            ) : null}
            {index.behind > 0 ? (
              <span className="shrink-0 tabular-nums text-content/40">↓{index.behind}</span>
            ) : null}
          </span>
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
        enabled={enabled}
        fill
        view={view}
        onOpenFile={onOpenFile}
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
        className={`shrink-0 overflow-hidden border-t border-content/10 ${
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
  enabled,
  fill,
  view,
  onOpenFile,
  onMutated,
}: {
  cwd: string;
  textHarness?: HarnessId;
  index: GitDiffIndex | null;
  files: GitChangedFile[];
  selected?: string;
  enabled: boolean;
  fill: boolean;
  view: ChangesView;
  onOpenFile: (path: string) => void;
  onMutated: (paths?: string[]) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const menuRef = useRef<HTMLDivElement>(null);
  const messageRef = useRef<HTMLTextAreaElement>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [stagedExpanded, setStagedExpanded] = useState(stagedOpen);
  const [changesExpanded, setChangesExpanded] = useState(changesOpen);
  const { pr, reload: reloadPr } = usePrStatus(cwd, index?.branch);
  const staged = files.filter((file) => file.staged);
  const unstaged = files.filter((file) => file.unstaged);
  const hasRemote = Boolean(index?.remote);
  const hasOpenPr = pr?.state === "open";
  const diverged = (index?.ahead ?? 0) > 0 && (index?.behind ?? 0) > 0;
  const onDefault =
    !!index?.branch && !!index.defaultBranch && index.branch === index.defaultBranch;
  const canGenerate = files.length > 0 && !busy;
  const canCommit = staged.length > 0 && message.trim().length > 0 && !busy;
  const canCreatePr =
    hasRemote &&
    !hasOpenPr &&
    !onDefault &&
    !diverged &&
    files.length === 0 &&
    (index?.aheadOfDefault ?? 0) > 0 &&
    (index?.behind ?? 0) === 0;
  const canViewPr = hasOpenPr && !!pr?.url;
  const canPublish = hasRemote && !index?.upstream;
  const canSync =
    hasRemote && Boolean(index?.upstream) && ((index?.ahead ?? 0) > 0 || (index?.behind ?? 0) > 0);
  const canCommitPush = canCommit && hasRemote && !diverged;
  const canCommitPushPr = canCommitPush && !hasOpenPr && !onDefault;
  const canEditMessage = staged.length > 0 && !busy;

  useEffect(() => {
    if (!enabled) return;
    const el = messageRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [message, enabled]);

  useEffect(() => {
    if (!menuOpen) return;
    const onPointer = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("pointerdown", onPointer);
    return () => window.removeEventListener("pointerdown", onPointer);
  }, [menuOpen]);

  const fail = (error: unknown) => {
    window.alert(error instanceof Error ? error.message : String(error));
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

  const run = async (file: GitChangedFile, action: "stage" | "unstage" | "discard") => {
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
      onMutated(action === "discard" ? unstaged.map((file) => file.path) : undefined);
    } catch (error) {
      fail(error);
    } finally {
      setBusy(null);
    }
  };

  const runDir = async (
    dirFiles: GitChangedFile[],
    dirPath: string,
    action: "stage" | "unstage" | "discard",
  ) => {
    if (busy || dirFiles.length === 0) return;
    if (action === "discard") {
      const n = dirFiles.length;
      const only = dirFiles[0];
      const untrackedOnly = n === 1 && only?.status === "untracked";
      const ok = await confirmNative(
        untrackedOnly
          ? `Delete untracked file ${basename(only.relative)}?`
          : n === 1 && only
            ? `Discard changes in ${basename(only.relative)}? This cannot be undone.`
            : `Discard all unstaged changes in ${n} files under ${dirPath}? This cannot be undone.`,
        untrackedOnly ? "Delete" : "Discard",
      );
      if (!ok) return;
    }
    setBusy(dirPath);
    try {
      for (const file of dirFiles) {
        if (action === "stage") await gitStageFile(cwd, file.relative);
        else if (action === "unstage") await gitUnstageFile(cwd, file.relative);
        else await gitDiscardFile(cwd, file.relative);
      }
      onMutated(dirFiles.map((file) => file.path));
    } catch (error) {
      fail(error);
    } finally {
      setBusy(null);
    }
  };

  const generate = async () => {
    if (!canGenerate) return;
    setBusy("generate");
    try {
      setMessage(await generateCommitMessage(cwd, textHarness));
    } catch (error) {
      fail(error);
    } finally {
      setBusy(null);
    }
  };

  const commit = async (push: boolean, createPr = false) => {
    if (!canCommit) return;
    if ((push || createPr) && !(await confirmDefault(createPr ? "pr" : "push"))) {
      return;
    }
    setBusy(createPr ? "pr" : "commit");
    setMenuOpen(false);
    try {
      await gitCommit(cwd, message);
      if (push || createPr) await gitPush(cwd);
      setMessage("");
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
    setBusy("sync");
    try {
      await gitSync(cwd);
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
    const content = await generatePrContent(cwd, textHarness);
    if (!content) throw new Error("Could not prepare pull request content");
    const url = await gitPrCreate(cwd, content.title, content.body, content.base, content.head);
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
    <aside className={`flex min-h-0 min-w-0 flex-col ${fill ? "flex-1" : "shrink-0"}`}>
      <div className="shrink-0 border-b border-content/10 p-2">
        <div className="relative">
          <textarea
            ref={messageRef}
            rows={1}
            value={message}
            placeholder={`Message (${MOD}↩ to commit)`}
            disabled={!canEditMessage}
            onChange={(event) => setMessage(event.target.value)}
            onKeyDown={(event) => {
              if ((event.metaKey || event.ctrlKey) && event.key === "Enter" && canCommit) {
                event.preventDefault();
                void commit(false);
              }
            }}
            className="max-h-40 w-full resize-none overflow-y-auto rounded-md bg-content/10 py-1 pr-8 pl-2 text-[13px] leading-5 text-content outline-none placeholder:text-content/35 disabled:opacity-40"
          />
          <button
            type="button"
            title="Generate commit message"
            aria-label="Generate commit message"
            disabled={!canGenerate}
            onClick={() => void generate()}
            className="absolute top-1 right-1 grid size-5 place-items-center rounded-md text-content bg-content/10 hover:bg-content/20 hover:text-content disabled:opacity-40"
          >
            {busy === "generate" ? (
              <Loader className="size-3.5 animate-spin" strokeWidth={1.75} />
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
            className="flex h-7 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-l-md bg-content text-[12px] font-medium text-background-base disabled:opacity-40"
          >
            <Check className="size-3.5" strokeWidth={2} />
            Commit
          </button>

          <button
            type="button"
            title="Commit options"
            aria-label="Commit options"
            disabled={!canCommit}
            onClick={() => setMenuOpen((open) => !open)}
            className="grid h-7 w-7 shrink-0 place-items-center rounded-r-md border-l border-background-base/10 bg-content text-background-base disabled:opacity-40"
          >
            <ChevronDown className="size-3.5" strokeWidth={2} />
          </button>
          {menuOpen ? (
            <div className="absolute top-full right-0 z-30 mt-1 min-w-48 rounded-md border border-content/10 bg-background-base py-1 shadow-lg">
              <button
                type="button"
                disabled={!canCommitPush}
                onClick={() => void commit(true)}
                className="flex h-7 w-full items-center px-3 text-left text-[12px] text-content hover:bg-content/10 disabled:opacity-40"
              >
                Commit & Push
              </button>
              <button
                type="button"
                disabled={!canCommitPushPr}
                onClick={() => void commit(true, true)}
                className="flex h-7 w-full items-center px-3 text-left text-[12px] text-content hover:bg-content/10 disabled:opacity-40"
              >
                Commit, Push & Create PR
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
      <div ref={lockOverscroll} className="min-h-0 flex-1 overflow-y-auto overscroll-none py-1">
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
                headerActions={[
                  {
                    title: "Unstage All Changes",
                    icon: <Minus className="size-3.5" strokeWidth={1.75} />,
                    onClick: () => void runAll("unstage"),
                  },
                ]}
              >
                {view === "tree" ? (
                  <ChangeFileTree
                    files={staged}
                    kind="staged"
                    selected={selected}
                    busy={busy}
                    onOpenFile={onOpenFile}
                    onAction={run}
                    onActionDir={runDir}
                  />
                ) : (
                  staged.map((file) => (
                    <ChangeRow
                      key={`staged:${file.relative}`}
                      file={file}
                      active={selected === file.relative}
                      busy={busy === file.relative}
                      kind="staged"
                      onOpenFile={onOpenFile}
                      onAction={run}
                    />
                  ))
                )}
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
                headerActions={[
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
                {view === "tree" ? (
                  <ChangeFileTree
                    files={unstaged}
                    kind="unstaged"
                    selected={selected}
                    busy={busy}
                    onOpenFile={onOpenFile}
                    onAction={run}
                    onActionDir={runDir}
                  />
                ) : (
                  unstaged.map((file) => (
                    <ChangeRow
                      key={`unstaged:${file.relative}`}
                      file={file}
                      active={selected === file.relative}
                      busy={busy === file.relative}
                      kind="unstaged"
                      onOpenFile={onOpenFile}
                      onAction={run}
                    />
                  ))
                )}
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

function cachedPr(cwd: string, branch: string | null | undefined): GitPr | null {
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
  const dest = index.upstream ?? `${index.remote ?? "origin"}/${index.branch ?? "HEAD"}`;
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
  const viewTitle = pr?.title ? `View PR #${pr.number}: ${pr.title}` : "View pull request";
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
            <Loader className="size-3.5 shrink-0 animate-spin" strokeWidth={1.75} />
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
            <span className="shrink-0 tabular-nums text-content/55">↓{behind}</span>
          ) : null}
          {ahead > 0 ? (
            <span className="shrink-0 tabular-nums text-content/55">↑{ahead}</span>
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
            <Loader className="size-3.5 shrink-0 animate-spin" strokeWidth={1.75} />
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

function FileSection({
  title,
  count,
  open,
  onToggle,
  headerActions,
  children,
}: {
  title: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  headerActions: { title: string; icon: ReactNode; onClick: () => void }[];
  children: ReactNode;
}) {
  return (
    <div>
      <div className="group flex h-7 items-center gap-1 px-1.5">
        <button
          type="button"
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-1 text-left"
        >
          {open ? (
            <ChevronDown className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
          ) : (
            <ChevronRight className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
          )}
          <span className="min-w-0 truncate text-[10px] font-semibold tracking-[0.04em] text-content/55 uppercase">
            {title}
          </span>
          <span className="ml-1 grid h-4 min-w-4 shrink-0 place-items-center rounded-full bg-accent/80 px-1 text-[8px] text-white">
            {count}
          </span>
        </button>
        <div className="flex opacity-0 group-hover:opacity-100 group-focus-within:opacity-100">
          {headerActions.map((action) => (
            <IconAction key={action.title} title={action.title} onClick={action.onClick}>
              {action.icon}
            </IconAction>
          ))}
        </div>
      </div>
      {open ? <ul>{children}</ul> : null}
    </div>
  );
}

function ChangeRow({
  file,
  active,
  busy,
  kind,
  indent = 0,
  onOpenFile,
  onAction,
}: {
  file: GitChangedFile;
  active: boolean;
  busy: boolean;
  kind: "staged" | "unstaged";
  indent?: number;
  onOpenFile: (path: string) => void;
  onAction: (file: GitChangedFile, action: "stage" | "unstage" | "discard") => void;
}) {
  const name = basename(file.relative);
  const dir = dirname(file.relative);
  const canOpen = file.status !== "deleted";
  return (
    <li>
      <div
        className={`group flex h-7 w-full items-center gap-1 px-2 leading-none ${
          active ? "bg-content/10 text-content" : "text-content hover:bg-content/5"
        }`}
        style={indent > 0 ? { paddingLeft: 8 + indent } : undefined}
      >
        <button
          type="button"
          title={file.relative}
          onClick={() => {
            if (canOpen) onOpenFile(file.path);
          }}
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
        >
          <FileTypeIcon name={name} isDir={false} size={16} />
          <span className="min-w-0 flex-1 truncate">
            <span className="text-[13px] font-medium">{name}</span>
            {dir && indent === 0 ? (
              <span className="ml-1.5 text-[11px] text-content/40">{dir}</span>
            ) : null}
          </span>
        </button>
        <div
          className={` shrink-0 items-center ${
            active ? "flex" : "hidden group-focus-within:flex group-hover:flex"
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

function ChangesViewToggle({
  view,
  onChange,
}: {
  view: ChangesView;
  onChange: (next: ChangesView) => void;
}) {
  const cls = (active: boolean) =>
    `grid size-6 place-items-center rounded-md ${
      active
        ? "bg-content/10 text-content"
        : "text-content/45 hover:bg-content/5 hover:text-content"
    }`;
  return (
    <div
      role="group"
      aria-label="Changes view"
      className="flex shrink-0 items-center gap-px rounded-md border border-content/10 p-px"
    >
      <button
        type="button"
        title="List view"
        aria-label="List view"
        aria-pressed={view === "list"}
        onClick={() => onChange("list")}
        className={cls(view === "list")}
      >
        <ListView className="size-3.5" strokeWidth={1.75} />
      </button>
      <button
        type="button"
        title="Tree view"
        aria-label="Tree view"
        aria-pressed={view === "tree"}
        onClick={() => onChange("tree")}
        className={cls(view === "tree")}
      >
        <FolderTree className="size-3.5" strokeWidth={1.75} />
      </button>
    </div>
  );
}

type ChangeTreeNode = {
  /** Canonical dir path (path-first identity, "" never occurs for nodes). */
  path: string;
  /** Flattened display label, e.g. "src/chrome". */
  label: string;
  depth: number;
  dirs: ChangeTreeNode[];
  files: GitChangedFile[];
  total: number;
};

/**
 * Directory grouping for the tree view, modeled on trees.software concepts:
 * path-first identity over `relative`, one memoizable build per file list
 * (prepared-input style), `flattenEmptyDirectories` chain collapsing, and a
 * Git-status lane via the existing per-file status letters. File rows reuse
 * ChangeRow so stage/open/discard behavior matches the list view.
 */
function buildChangeTree(files: readonly GitChangedFile[]): {
  dirs: ChangeTreeNode[];
  files: GitChangedFile[];
} {
  type Raw = { dirs: Map<string, Raw>; files: GitChangedFile[] };
  const root: Raw = { dirs: new Map(), files: [] };
  for (const file of files) {
    const parts = file.relative.split("/");
    let node = root;
    for (let i = 0; i < parts.length - 1; i += 1) {
      const part = parts[i]!;
      let child = node.dirs.get(part);
      if (!child) {
        child = { dirs: new Map(), files: [] };
        node.dirs.set(part, child);
      }
      node = child;
    }
    node.files.push(file);
  }
  const toNodes = (
    raw: Raw,
    parentPath: string,
    depth: number,
  ): { dirs: ChangeTreeNode[]; files: GitChangedFile[] } => {
    const outFiles = [...raw.files].sort((a, b) => a.relative.localeCompare(b.relative));
    const outDirs = [...raw.dirs.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([name, child]): ChangeTreeNode => {
        const path = parentPath ? `${parentPath}/${name}` : name;
        const inner = toNodes(child, path, depth + 1);
        let node: ChangeTreeNode = {
          path,
          label: name,
          depth,
          dirs: inner.dirs,
          files: inner.files,
          total: 0,
        };
        while (node.files.length === 0 && node.dirs.length === 1) {
          const only = node.dirs[0]!;
          node = {
            path: only.path,
            label: `${node.label}/${only.label}`,
            depth,
            dirs: only.dirs,
            files: only.files,
            total: 0,
          };
        }
        node.total = node.files.length + node.dirs.reduce((sum, dir) => sum + dir.total, 0);
        return node;
      });
    return { dirs: outDirs, files: outFiles };
  };
  return toNodes(root, "", 0);
}

function ChangeFileTree({
  files,
  kind,
  selected,
  busy,
  onOpenFile,
  onAction,
  onActionDir,
}: {
  files: GitChangedFile[];
  kind: "staged" | "unstaged";
  selected?: string;
  busy: string | null;
  onOpenFile: (path: string) => void;
  onAction: (file: GitChangedFile, action: "stage" | "unstage" | "discard") => void;
  onActionDir: (
    dirFiles: GitChangedFile[],
    dirPath: string,
    action: "stage" | "unstage" | "discard",
  ) => void;
}) {
  const { dirs, files: rootFiles } = useMemo(() => buildChangeTree(files), [files]);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const toggle = useCallback((path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);
  return (
    <>
      {dirs.map((node) => (
        <ChangeDirNode
          key={`${kind}:${node.path}`}
          node={node}
          kind={kind}
          selected={selected}
          busy={busy}
          collapsed={collapsed}
          onToggle={toggle}
          onOpenFile={onOpenFile}
          onAction={onAction}
          onActionDir={onActionDir}
        />
      ))}
      {rootFiles.map((file) => (
        <ChangeRow
          key={`${kind}:${file.relative}`}
          file={file}
          active={selected === file.relative}
          busy={busy === file.relative}
          kind={kind}
          onOpenFile={onOpenFile}
          onAction={onAction}
        />
      ))}
    </>
  );
}

function collectDirFiles(node: ChangeTreeNode): GitChangedFile[] {
  const out = [...node.files];
  for (const child of node.dirs) out.push(...collectDirFiles(child));
  return out;
}

function ChangeDirNode({
  node,
  kind,
  selected,
  busy,
  collapsed,
  onToggle,
  onOpenFile,
  onAction,
  onActionDir,
}: {
  node: ChangeTreeNode;
  kind: "staged" | "unstaged";
  selected?: string;
  busy: string | null;
  collapsed: Set<string>;
  onToggle: (path: string) => void;
  onOpenFile: (path: string) => void;
  onAction: (file: GitChangedFile, action: "stage" | "unstage" | "discard") => void;
  onActionDir: (
    dirFiles: GitChangedFile[],
    dirPath: string,
    action: "stage" | "unstage" | "discard",
  ) => void;
}) {
  const open = !collapsed.has(node.path);
  const dirFiles = useMemo(() => collectDirFiles(node), [node]);
  return (
    <li>
      <div className="group flex h-7 w-full items-center gap-1 pr-2 leading-none text-content hover:bg-content/5">
        <button
          type="button"
          title={node.path}
          aria-expanded={open}
          onClick={() => onToggle(node.path)}
          className="flex min-w-0 flex-1 items-center gap-1 text-left"
          style={{ paddingLeft: 8 + node.depth * 12 }}
        >
          {open ? (
            <ChevronDown className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
          ) : (
            <ChevronRight className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
          )}
          {open ? (
            <FolderOpen className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
          ) : (
            <Folder className="size-3.5 shrink-0 text-content/50" strokeWidth={1.75} />
          )}
          <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{node.label}</span>
        </button>
        <div className="hidden shrink-0 items-center group-focus-within:flex group-hover:flex">
          {kind === "unstaged" ? (
            <IconAction
              title={`Discard Changes in ${node.label}`}
              disabled={busy !== null}
              onClick={() => onActionDir(dirFiles, node.path, "discard")}
            >
              <Undo2 className="size-3.5" strokeWidth={1.75} />
            </IconAction>
          ) : null}
          {kind === "staged" ? (
            <IconAction
              title={`Unstage Changes in ${node.label}`}
              disabled={busy !== null}
              onClick={() => onActionDir(dirFiles, node.path, "unstage")}
            >
              <Minus className="size-3.5" strokeWidth={1.75} />
            </IconAction>
          ) : (
            <IconAction
              title={`Stage Changes in ${node.label}`}
              disabled={busy !== null}
              onClick={() => onActionDir(dirFiles, node.path, "stage")}
            >
              <Plus className="size-3.5" strokeWidth={1.75} />
            </IconAction>
          )}
        </div>
        <span className="shrink-0 text-[10px] tabular-nums text-content/40">{node.total}</span>
      </div>
      {open ? (
        <ul>
          {node.dirs.map((child) => (
            <ChangeDirNode
              key={`${kind}:${child.path}`}
              node={child}
              kind={kind}
              selected={selected}
              busy={busy}
              collapsed={collapsed}
              onToggle={onToggle}
              onOpenFile={onOpenFile}
              onAction={onAction}
              onActionDir={onActionDir}
            />
          ))}
          {node.files.map((file) => (
            <ChangeRow
              key={`${kind}:${file.relative}`}
              file={file}
              active={selected === file.relative}
              busy={busy === file.relative}
              kind={kind}
              indent={(node.depth + 1) * 12}
              onOpenFile={onOpenFile}
              onAction={onAction}
            />
          ))}
        </ul>
      ) : null}
    </li>
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
  const [index, setIndex] = useState<GitDiffIndex | null>(() => cachedIndex(cwd));
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
    prev.additions !== next.additions ||
    prev.deletions !== next.deletions ||
    prev.files.length !== next.files.length ||
    prev.remote !== next.remote ||
    prev.upstream !== next.upstream ||
    prev.defaultBranch !== next.defaultBranch ||
    prev.ahead !== next.ahead ||
    prev.behind !== next.behind ||
    prev.aheadOfDefault !== next.aheadOfDefault
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
