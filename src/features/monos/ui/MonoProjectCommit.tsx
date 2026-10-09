import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Loader,
  Minus,
  Plus,
  WandSparkles,
  X,
} from "../../../shared/ui/icons";
import {
  basename,
  gitCommit,
  gitDiffIndex,
  gitDiscardFile,
  gitPrCreate,
  gitPush,
  gitSync,
  notifyGitChanged,
  subscribeGitChanged,
  type GitChangedFile,
  type GitDiffIndex,
} from "../../../platform/tauri/fs";
import {
  generateCommitMessage,
  generatePrContent,
} from "../../../integrations/harness";
import { MOD } from "../../../platform/tauri/platform";
import { invalidateWatchedFiles } from "../../files/model/fileWatch";
import { recordInboxSelfActivity } from "../../inbox/model/inboxSelfActivity";
import {
  keepSessionChanges,
  notifyReviewChanged,
  type CheckpointFile,
} from "../../sessions/model/checkpoint";
import type { HarnessId } from "../../sessions/model/session";
import {
  loadChangesView,
  saveChangesView,
  type ChangesView,
} from "../../settings/model/appearance";
import {
  ChangeList,
  FileSection,
  GitSyncActions,
  usePrStatus,
} from "../../source-control/ui/GitChangesPanel";

/** One session file, with its path inside the repository that holds it. */
export type MonoProjectFile = { file: CheckpointFile; relative: string };

/** Live Git status for a project a Mono touched. */
export function useProjectIndex(root: string | undefined): {
  index: GitDiffIndex | null;
  reload: () => void;
} {
  const [loaded, setLoaded] = useState<{ root: string; index: GitDiffIndex } | null>(null);
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((value) => value + 1), []);

  useEffect(() => {
    if (!root) {
      setLoaded(null);
      return;
    }
    let cancelled = false;
    let generation = 0;
    const load = () => {
      const current = ++generation;
      void gitDiffIndex(root)
        .then((next) => {
          if (!cancelled && current === generation) setLoaded({ root, index: next });
        })
        .catch(() => {
          if (!cancelled && current === generation) setLoaded(null);
        });
    };
    load();
    const unsubscribe = subscribeGitChanged(load);
    window.addEventListener("focus", load);
    return () => {
      cancelled = true;
      unsubscribe();
      window.removeEventListener("focus", load);
    };
  }, [root, nonce]);

  return { index: loaded && loaded.root === root ? loaded.index : null, reload };
}

function confirmNative(message: string): Promise<boolean> {
  return ask(message, { title: "MonoCode", kind: "warning" });
}

function fail(error: unknown) {
  window.alert(error instanceof Error ? error.message : String(error));
}

/**
 * Commit a Mono's work in one project. The session's files start selected;
 * anything else pending in the checkout is listed but left out unless chosen.
 */
export function MonoProjectCommit({
  sessionId,
  cwd,
  root,
  files,
  index,
  reloadIndex,
  textHarness,
  onOpenFile,
}: {
  sessionId: string;
  /** The session's checkpoint folder, which owns `file.relative`. */
  cwd: string;
  root: string;
  files: MonoProjectFile[];
  index: GitDiffIndex | null;
  reloadIndex: () => void;
  textHarness?: HarnessId;
  onOpenFile: (path: string) => void;
}) {
  const { pr, reload: reloadPr } = usePrStatus(root, index?.branch);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  // Session files are in by default and other changes out; keep exceptions.
  const [excluded, setExcluded] = useState<ReadonlySet<string>>(new Set());
  const [included, setIncluded] = useState<ReadonlySet<string>>(new Set());
  const [view, setView] = useState<ChangesView>(loadChangesView);
  const [commitOpen, setCommitOpen] = useState(true);
  const [restOpen, setRestOpen] = useState(true);
  const menuRef = useRef<HTMLDivElement>(null);
  const messageRef = useRef<HTMLTextAreaElement>(null);
  const generateAbortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    if (!status) return;
    const timer = window.setTimeout(() => setStatus(null), 4000);
    return () => window.clearTimeout(timer);
  }, [status]);

  useEffect(() => {
    if (!menuOpen) return;
    const onPointer = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("pointerdown", onPointer);
    return () => window.removeEventListener("pointerdown", onPointer);
  }, [menuOpen]);

  useEffect(() => {
    const el = messageRef.current;
    if (!el) return;
    el.style.height = "auto";
    // The tab mounts hidden, where nothing has a height; leave it at its
    // natural size until it can be measured.
    if (el.scrollHeight === 0) return;
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [message]);

  useEffect(() => () => generateAbortRef.current?.abort(), []);

  const own = new Set(files.map((entry) => entry.relative));
  const changed = index?.files ?? [];
  const isSelected = (relative: string) =>
    own.has(relative) ? !excluded.has(relative) : included.has(relative);
  const inCommit = changed.filter((file) => isSelected(file.relative));
  const rest = changed.filter((file) => !isSelected(file.relative));
  const selected = inCommit.map((file) => file.relative);
  const hasRemote = Boolean(index?.remote);
  const hasOpenPr = pr?.state === "open";
  const diverged = (index?.ahead ?? 0) > 0 && (index?.behind ?? 0) > 0;
  const onDefault =
    !!index?.branch &&
    !!index.defaultBranch &&
    index.branch === index.defaultBranch;
  const canCommit = selected.length > 0 && message.trim().length > 0 && !busy;
  const canCommitPush = canCommit && hasRemote && !diverged;
  const canCommitPushPr = canCommitPush && !hasOpenPr && !onDefault;
  const canPublish = hasRemote && !!index && !index.upstream;
  const canSync =
    hasRemote &&
    Boolean(index?.upstream) &&
    ((index?.ahead ?? 0) > 0 || (index?.behind ?? 0) > 0);
  const canCreatePr =
    hasRemote &&
    !!index?.branch &&
    !!index.defaultBranch &&
    !hasOpenPr &&
    !onDefault &&
    !diverged &&
    (index?.aheadOfDefault ?? 0) > 0 &&
    (index?.behind ?? 0) === 0;

  const choose = (relatives: readonly string[], include: boolean) => {
    const apply =
      (keep: (relative: string) => boolean, add: boolean) =>
      (previous: ReadonlySet<string>) => {
        const next = new Set(previous);
        for (const relative of relatives.filter(keep)) {
          if (add) next.add(relative);
          else next.delete(relative);
        }
        return next;
      };
    setExcluded(apply((relative) => own.has(relative), !include));
    setIncluded(apply((relative) => !own.has(relative), include));
  };

  const discard = async (file: GitChangedFile) => {
    if (busy) return;
    const name = basename(file.relative);
    const untracked = file.status === "untracked";
    const ok = await ask(
      untracked
        ? `Delete untracked file ${name}?`
        : `Discard changes in ${name}? This cannot be undone.`,
      {
        title: "MonoCode",
        kind: "warning",
        okLabel: untracked ? "Delete" : "Discard",
      },
    );
    if (!ok) return;
    setBusy(file.relative);
    try {
      await gitDiscardFile(root, file.relative);
      invalidateWatchedFiles([file.path]);
      notifyReviewChanged(sessionId);
    } catch (error) {
      fail(error);
    } finally {
      settle();
      setBusy(null);
    }
  };

  const openFile = (relative: string) => {
    const entry = files.find((item) => item.relative === relative);
    if (entry) onOpenFile(entry.file.path);
  };

  const changeListProps = {
    view,
    busy,
    onOpenFile: (path: string) => {
      const file = changed.find((item) => item.path === path);
      if (file) openFile(file.relative);
    },
    onAction: (
      file: GitChangedFile,
      action: "stage" | "unstage" | "discard",
    ) => {
      if (action === "discard") void discard(file);
      else choose([file.relative], action === "stage");
    },
    onFolderAction: (folder: string, action: "stage" | "unstage") =>
      choose(
        changed
          .filter((file) => file.relative.startsWith(`${folder}/`))
          .map((file) => file.relative),
        action === "stage",
      ),
  };

  const recordPrActivity = (number = pr?.number) => {
    if (!number) return;
    recordInboxSelfActivity({
      provider: "github",
      kind: "pr",
      number,
      projectPath: root,
    });
  };

  const confirmDefault = (kind: "push" | "pr") =>
    !onDefault || !index?.branch
      ? Promise.resolve(true)
      : confirmNative(
          kind === "pr"
            ? `Create a pull request from default branch "${index.branch}"?`
            : `Push to default branch "${index.branch}"?`,
        );

  const openCreatedPr = async () => {
    const content = await generatePrContent(root, textHarness);
    if (!content) throw new Error("Could not prepare pull request content");
    const url = await gitPrCreate(
      root,
      content.title,
      content.body,
      content.base,
      content.head,
    );
    const number = Number(/\/pull\/(\d+)(?:[/?#]|$)/.exec(url)?.[1]);
    if (Number.isInteger(number) && number > 0) recordPrActivity(number);
    await openUrl(url.trim());
  };

  const settle = () => {
    reloadIndex();
    reloadPr();
    notifyGitChanged();
  };

  const generate = async () => {
    if (busy || selected.length === 0 || generateAbortRef.current) return;
    const controller = new AbortController();
    generateAbortRef.current = controller;
    setBusy("generate");
    try {
      const generated = await generateCommitMessage(
        root,
        textHarness,
        controller.signal,
        selected,
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

  const commit = async (push: boolean, createPr = false) => {
    if (!canCommit) return;
    setMenuOpen(false);
    if (
      (push || createPr) &&
      !(await confirmDefault(createPr ? "pr" : "push"))
    ) {
      return;
    }
    setBusy(createPr ? "pr" : "commit");
    const committed = files.filter((entry) => selected.includes(entry.relative));
    try {
      await gitCommit(root, message, false, selected);
      // Committed work is accepted work: clear it from the session's review.
      for (const entry of committed) {
        await keepSessionChanges(sessionId, cwd, entry.file.relative);
      }
      notifyReviewChanged(sessionId);
      invalidateWatchedFiles(committed.map((entry) => entry.file.path));
      setMessage("");
      setIncluded(new Set());
      if (push || createPr) {
        await gitPush(root);
        recordPrActivity();
      }
      if (createPr) await openCreatedPr();
      setStatus(
        createPr
          ? "Pull request created"
          : push
            ? "Committed and pushed"
            : "Committed",
      );
    } catch (error) {
      fail(error);
    } finally {
      settle();
      setBusy(null);
    }
  };

  const sync = async () => {
    if (!index || !(canSync || canPublish)) return;
    const pushesCommits = index.ahead > 0;
    setBusy("sync");
    try {
      await gitSync(root);
      if (pushesCommits) recordPrActivity();
    } catch (error) {
      fail(error);
    } finally {
      settle();
      setBusy(null);
    }
  };

  const createPr = async () => {
    if (!canCreatePr || !(await confirmDefault("pr"))) return;
    setBusy("pr");
    try {
      if ((index?.ahead ?? 0) > 0) await gitPush(root);
      await openCreatedPr();
    } catch (error) {
      fail(error);
    } finally {
      settle();
      setBusy(null);
    }
  };

  const toggleView = () => {
    const next = view === "tree" ? "list" : "tree";
    saveChangesView(next);
    setView(next);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-mono-commit={root}>
      <div className="shrink-0 border-b border-stroke p-2">
        <div className="relative">
          <textarea
            ref={messageRef}
            rows={1}
            value={message}
            placeholder={`Message (${MOD}↩ to commit)`}
            disabled={!!busy && busy !== "generate"}
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
            aria-label={
              busy === "generate"
                ? "Cancel commit message generation"
                : "Generate commit message"
            }
            title={
              busy === "generate"
                ? "Cancel commit message generation"
                : "Generate commit message"
            }
            disabled={
              busy === "generate" ? false : !!busy || selected.length === 0
            }
            onClick={() =>
              busy === "generate" ? cancelGenerate() : void generate()
            }
            className="group absolute top-1 right-1 grid size-5 place-items-center rounded-md bg-content/10 text-content hover:bg-content/20 disabled:opacity-40"
          >
            {busy === "generate" ? (
              <>
                <Loader
                  className="size-3.5 animate-spin group-hover:hidden"
                  strokeWidth={1.75}
                />
                <X
                  className="hidden size-3.5 group-hover:block"
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
            {busy === "commit" || busy === "pr" ? (
              <Loader className="size-3.5 animate-spin" strokeWidth={1.75} />
            ) : (
              <Check className="size-3.5" strokeWidth={2} />
            )}
            Commit
          </button>
          <button
            type="button"
            aria-label="Commit options"
            title="Commit options"
            aria-expanded={menuOpen}
            disabled={!!busy || !index?.branch}
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
            canViewPr={hasOpenPr && !!pr?.url}
            onSync={() => void sync()}
            onCreatePr={() => void createPr()}
            onViewPr={() => {
              if (pr?.url) void openUrl(pr.url);
            }}
          />
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none py-1">
        {changed.length === 0 ? (
          <p role="status" className="px-3 py-2 text-[12px] text-content/45">
            {status ?? (index ? "No uncommitted changes" : "Loading changes…")}
          </p>
        ) : (
          <>
            {status ? (
              <p
                role="status"
                className="px-3 pb-1 text-[11px] text-content/55"
              >
                {status}
              </p>
            ) : null}
            {inCommit.length > 0 ? (
              <FileSection
                title="This Commit"
                count={inCommit.length}
                open={commitOpen}
                onToggle={() => setCommitOpen((open) => !open)}
                view={view}
                onToggleView={toggleView}
                headerActions={[
                  {
                    title: "Leave All Out",
                    icon: <Minus className="size-3.5" strokeWidth={1.75} />,
                    onClick: () => choose(selected, false),
                  },
                ]}
              >
                <ChangeList
                  {...changeListProps}
                  files={inCommit}
                  kind="staged"
                />
              </FileSection>
            ) : null}
            {rest.length > 0 ? (
              <FileSection
                title="Changes"
                count={rest.length}
                open={restOpen}
                onToggle={() => setRestOpen((open) => !open)}
                view={view}
                onToggleView={toggleView}
                headerActions={[
                  {
                    title: "Add All to Commit",
                    icon: <Plus className="size-3.5" strokeWidth={1.75} />,
                    onClick: () =>
                      choose(
                        rest.map((file) => file.relative),
                        true,
                      ),
                  },
                ]}
              >
                <ChangeList {...changeListProps} files={rest} kind="unstaged" />
              </FileSection>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}
