import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Check,
  ChevronDown,
  ChevronRight,
  CloudUpload,
  ExternalLink,
  GitBranch,
  GitPullRequest,
  Loader,
  Minus,
  Plus,
  RefreshCw,
  Undo2,
  WandSparkles,
  X,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { FileTypeIcon } from "../chrome/FileTypeIcon";
import {
  basename,
  gitCommit,
  gitDiffIndex,
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
  type GitPr,
} from "../lib/fs";
import type { HarnessId } from "../lib/session";
import { generateCommitMessage, generatePrContent } from "../lib/harness";
import { useLockOverscroll } from "../lib/useLockOverscroll";
import { MOD } from "../lib/platform";

const MIN_WIDTH = 280;
const DEFAULT_WIDTH = 300;

let rememberedWidth = DEFAULT_WIDTH;
let stagedOpen = true;
let changesOpen = true;

function clampDiffWidth(value: number) {
  const max = Math.min(
    Math.floor(window.innerWidth * 0.8),
    Math.max(MIN_WIDTH, window.innerWidth - 240),
  );
  return Math.min(max, Math.max(MIN_WIDTH, Math.round(value)));
}

type Props = {
  cwd: string;
  textHarness?: HarnessId;
  selectedPath?: string;
  focused: boolean;
  onFocus: () => void;
  onClose: () => void;
  onOpenFile: (path: string) => void;
};

export function DiffPane({
  cwd,
  textHarness,
  selectedPath,
  focused,
  onFocus,
  onClose,
  onOpenFile,
}: Props) {
  const [width, setWidth] = useState(() => rememberedWidth);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ startX: number; startW: number } | null>(null);
  const paneRef = useRef<HTMLElement>(null);
  const widthRef = useRef(width);
  const pendingWidth = useRef(width);
  const resizeFrame = useRef<number | null>(null);
  const { index, reload } = useDiffIndex(cwd);
  const files = index?.files ?? [];

  useEffect(() => {
    if (!dragging) return;
    const previous = document.body.style.cursor;
    document.body.style.cursor = "col-resize";
    return () => {
      document.body.style.cursor = previous;
    };
  }, [dragging]);

  useEffect(
    () => () => {
      if (resizeFrame.current != null) {
        cancelAnimationFrame(resizeFrame.current);
      }
    },
    [],
  );

  const applyWidth = (next: number, remember: boolean) => {
    pendingWidth.current = next;
    widthRef.current = next;
    if (remember) rememberedWidth = next;
    if (paneRef.current) paneRef.current.style.width = `${next}px`;
    setWidth(next);
  };

  const paintWidth = (next: number) => {
    pendingWidth.current = next;
    if (resizeFrame.current != null) return;
    resizeFrame.current = requestAnimationFrame(() => {
      resizeFrame.current = null;
      rememberedWidth = pendingWidth.current;
      if (paneRef.current) {
        paneRef.current.style.width = `${pendingWidth.current}px`;
      }
    });
  };

  const commitWidth = () => {
    if (resizeFrame.current != null) {
      cancelAnimationFrame(resizeFrame.current);
      resizeFrame.current = null;
    }
    applyWidth(pendingWidth.current, true);
  };

  const onResizePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { startX: e.clientX, startW: widthRef.current };
    pendingWidth.current = widthRef.current;
    setDragging(true);
  };

  const onResizePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    const next = clampDiffWidth(
      drag.current.startW - (e.clientX - drag.current.startX),
    );
    paintWidth(next);
  };

  const onResizePointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    drag.current = null;
    commitWidth();
    setDragging(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  return (
    <section
      ref={paneRef}
      data-diff-pane=""
      style={{ width }}
      className={`relative flex h-full min-h-0 shrink-0 flex-col border-l border-content/10 ${
        focused ? "bg-content/3" : "bg-content/2"
      }`}
      onMouseDown={onFocus}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize changes pane"
        aria-valuenow={width}
        aria-valuemin={MIN_WIDTH}
        className={`absolute inset-y-0 -left-px z-10 w-1.5 cursor-col-resize touch-none ${
          dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={onResizePointerDown}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
        onPointerCancel={onResizePointerUp}
        onDoubleClick={() => {
          pendingWidth.current = DEFAULT_WIDTH;
          commitWidth();
        }}
      />
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-content/10 px-3">
        <span className="text-[12px] font-medium text-content">Changes</span>
        <DiffCounts
          additions={index?.additions ?? 0}
          deletions={index?.deletions ?? 0}
        />
        {index?.branch ? (
          <span className="ml-auto flex min-w-0 items-center gap-1 text-[11px] text-content/50">
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
        ) : (
          <span className="ml-auto" />
        )}
        <button
          type="button"
          title="Close changes"
          aria-label="Close changes"
          onClick={onClose}
          className="grid size-6 shrink-0 place-items-center rounded text-content/45 hover:bg-content/10 hover:text-content"
        >
          <X className="size-3.5" strokeWidth={1.75} />
        </button>
      </header>
      <ChangedFiles
        cwd={cwd}
        textHarness={textHarness}
        index={index}
        files={files}
        selected={selectedPath}
        onOpenFile={onOpenFile}
        onMutated={() => {
          reload();
          notifyGitChanged();
        }}
      />
    </section>
  );
}

function ChangedFiles({
  cwd,
  textHarness,
  index,
  files,
  selected,
  onOpenFile,
  onMutated,
}: {
  cwd: string;
  textHarness?: HarnessId;
  index: GitDiffIndex | null;
  files: GitChangedFile[];
  selected?: string;
  onOpenFile: (path: string) => void;
  onMutated: () => void;
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
    !!index?.branch &&
    !!index.defaultBranch &&
    index.branch === index.defaultBranch;
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
    hasRemote &&
    Boolean(index?.upstream) &&
    ((index?.ahead ?? 0) > 0 || (index?.behind ?? 0) > 0);
  const canCommitPush = canCommit && hasRemote && !diverged;
  const canCommitPushPr = canCommitPush && !hasOpenPr && !onDefault;
  const canEditMessage = staged.length > 0 && !busy;

  useEffect(() => {
    const el = messageRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [message]);

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

  const confirmDefault = (kind: "push" | "pr") => {
    if (!onDefault || !index?.branch) return true;
    const branch = index.branch;
    return window.confirm(
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
      const ok = window.confirm(
        file.status === "untracked"
          ? `Delete untracked file ${name}?`
          : `Discard changes in ${name}? This cannot be undone.`,
      );
      if (!ok) return;
    }
    setBusy(file.relative);
    try {
      if (action === "stage") await gitStageFile(cwd, file.relative);
      else if (action === "unstage") await gitUnstageFile(cwd, file.relative);
      else await gitDiscardFile(cwd, file.relative);
      onMutated();
    } catch (error) {
      fail(error);
    } finally {
      setBusy(null);
    }
  };

  const runAll = async (action: "stage" | "unstage") => {
    if (busy) return;
    setBusy(action);
    try {
      if (action === "stage") await gitStageAll(cwd);
      else await gitUnstageAll(cwd);
      onMutated();
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
    if ((push || createPr) && !confirmDefault(createPr ? "pr" : "push")) return;
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
    const willPush = !index.upstream || index.ahead > 0;
    if (willPush && !confirmDefault("push")) return;
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
    const url = await gitPrCreate(
      cwd,
      content.title,
      content.body,
      content.base,
      content.head,
    );
    await openUrl(url.trim());
  };

  const createPr = async () => {
    if (!canCreatePr) return;
    if (!confirmDefault("pr")) return;
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
    <aside className="flex min-h-0 min-w-0 flex-1 flex-col">
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
      <div
        ref={lockOverscroll}
        className="min-h-0 flex-1 overflow-y-auto overscroll-none py-1"
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
                headerAction={{
                  title: "Unstage All Changes",
                  icon: <Minus className="size-3.5" strokeWidth={1.75} />,
                  onClick: () => void runAll("unstage"),
                }}
              >
                {staged.map((file) => (
                  <ChangeRow
                    key={`staged:${file.relative}`}
                    file={file}
                    active={selected === file.relative}
                    busy={busy === file.relative}
                    kind="staged"
                    onOpenFile={onOpenFile}
                    onAction={run}
                  />
                ))}
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
                headerAction={{
                  title: "Stage All Changes",
                  icon: <Plus className="size-3.5" strokeWidth={1.75} />,
                  onClick: () => void runAll("stage"),
                }}
              >
                {unstaged.map((file) => (
                  <ChangeRow
                    key={`unstaged:${file.relative}`}
                    file={file}
                    active={selected === file.relative}
                    busy={busy === file.relative}
                    kind="unstaged"
                    onOpenFile={onOpenFile}
                    onAction={run}
                  />
                ))}
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
  const [pr, setPr] = useState<GitPr | null>(null);
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
          if (!cancelled) setPr(next);
        })
        .catch(() => {
          if (!cancelled) setPr(null);
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

function FileSection({
  title,
  count,
  open,
  onToggle,
  headerAction,
  children,
}: {
  title: string;
  count: number;
  open: boolean;
  onToggle: () => void;
  headerAction: { title: string; icon: ReactNode; onClick: () => void };
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
        <div className="opacity-0 group-hover:opacity-100 group-focus-within:opacity-100">
          <IconAction title={headerAction.title} onClick={headerAction.onClick}>
            {headerAction.icon}
          </IconAction>
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
  onOpenFile,
  onAction,
}: {
  file: GitChangedFile;
  active: boolean;
  busy: boolean;
  kind: "staged" | "unstaged";
  onOpenFile: (path: string) => void;
  onAction: (
    file: GitChangedFile,
    action: "stage" | "unstage" | "discard",
  ) => void;
}) {
  const name = basename(file.relative);
  const dir = dirname(file.relative);
  const canOpen = file.status !== "deleted";
  return (
    <li>
      <div
        className={`group flex h-7 w-full items-center gap-1 px-2 leading-none ${
          active
            ? "bg-content/10 text-content"
            : "text-content hover:bg-content/5"
        }`}
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
            {dir ? (
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

function DiffCounts({
  additions,
  deletions,
}: {
  additions: number;
  deletions: number;
}) {
  if (additions <= 0 && deletions <= 0) return null;
  return (
    <span className="flex shrink-0 items-center gap-1.5 font-mono text-[11px] font-semibold tabular-nums">
      {additions > 0 ? (
        <span className="text-emerald-400">+{additions}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-red-400">-{deletions}</span>
      ) : null}
    </span>
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

function useDiffIndex(cwd: string): {
  index: GitDiffIndex | null;
  reload: () => void;
} {
  const [index, setIndex] = useState<GitDiffIndex | null>(null);
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((value) => value + 1), []);

  useEffect(() => {
    if (!cwd || cwd === "~") {
      setIndex(null);
      return;
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
        setIndex((prev) => (sameIndex(prev, next) ? prev : next));
      } catch {
        if (!cancelled) setIndex(null);
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
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    const unsubGit = subscribeGitChanged(onResume);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
      unsubGit();
    };
  }, [cwd, nonce]);

  return { index, reload };
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
