import {
  useState,
  type ComponentType,
  type FormEvent,
  type ReactNode,
} from "react";
import { prettyCwd } from "../../../shared/lib/paths";
import { type Worktree } from "../model/worktrees";
import { Modal } from "../../../shared/ui/Modal";
import {
  CircleAlert,
  CloudUpload,
  FileDiff,
  Folder,
  GitBranch,
  Loader,
  MessageSquare,
} from "../../../shared/ui/icons";
import { useTranslation } from "../../i18n/model/i18n";

const TONE = {
  danger: "text-red-400",
  warn: "text-amber-400",
  muted: "text-content/35",
};

function Consequence({
  icon: Icon,
  tone = "muted",
  children,
}: {
  icon: ComponentType<{ className?: string }>;
  tone?: keyof typeof TONE;
  children: ReactNode;
}) {
  return (
    <li className="flex items-start gap-2.5">
      <Icon className={`mt-px size-3.5 shrink-0 ${TONE[tone]}`} />
      <span className="min-w-0 flex-1">{children}</span>
    </li>
  );
}

export function DeleteWorktreeDialog({
  cwd,
  tree,
  sessionCount = 0,
  onRemove,
  onClose,
  onDeleted,
}: {
  cwd: string;
  tree: Worktree;
  sessionCount?: number;
  onRemove: (
    cwd: string,
    path: string,
    force: boolean,
    deleteSessions: boolean,
  ) => Promise<void>;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [deleteSessions, setDeleteSessions] = useState(false);
  const [error, setError] = useState<string>();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      // Confirmation covers the complete destructive action, including any
      // local changes that appeared after the last status refresh.
      await onRemove(cwd, tree.path, true, deleteSessions);
      onDeleted();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={t("dialogs.deleteWorktreeTitle", "Delete worktree?")}
      size="sm"
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form
        className="flex flex-col gap-3.5 p-4 text-[13px] leading-[1.5]"
        onSubmit={(e) => void submit(e)}
      >
        <p className="text-content/75">
          {t(
            "dialogs.deleteWorktreeDescription",
            "This permanently deletes the working copy and everything inside it.",
          )}
        </p>
        <div className="rounded-lg border border-content/10 bg-content/5 p-3">
          <p className="flex items-start gap-2.5 text-[12px] text-content/55">
            <Folder className="mt-px size-3.5 shrink-0 text-content/35" />
            <span className="min-w-0 flex-1 break-all font-mono">
              {prettyCwd(tree.path)}
            </span>
          </p>
          <ul className="mt-2.5 flex flex-col gap-2 border-t border-content/8 pt-2.5 text-[12.5px] text-content/75">
            {sessionCount > 0 && (
              <Consequence
                icon={MessageSquare}
                tone={deleteSessions ? "danger" : "muted"}
              >
                {sessionCount === 1
                  ? t(
                      "dialogs.deleteWorktreeSessionCount_one",
                      "1 session using this worktree {status}",
                      {
                        status: deleteSessions
                          ? t(
                              "dialogs.deleteWorktreeSessionsPermanentlyDeleted_one",
                              "is permanently deleted.",
                            )
                          : t(
                              "dialogs.deleteWorktreeSessionsKept_one",
                              "is kept. Select a branch or worktree to continue them.",
                            ),
                      },
                    )
                  : t(
                      "dialogs.deleteWorktreeSessionCount_other",
                      "{count} sessions using this worktree {status}",
                      {
                        count: sessionCount,
                        status: deleteSessions
                          ? t(
                              "dialogs.deleteWorktreeSessionsPermanentlyDeleted_other",
                              "are permanently deleted.",
                            )
                          : t(
                              "dialogs.deleteWorktreeSessionsKept_other",
                              "are kept. Select a branch or worktree to continue them.",
                            ),
                      },
                    )}
              </Consequence>
            )}
            {tree.dirty && (
              <Consequence icon={FileDiff} tone="warn">
                {t(
                  "dialogs.deleteWorktreeDirtyWarning",
                  "All uncommitted and untracked changes here are discarded.",
                )}
              </Consequence>
            )}
            {tree.dirty == null && (
              <Consequence icon={CircleAlert} tone="warn">
                {t(
                  "dialogs.deleteWorktreeDirtyCheckFailed",
                  "Changes could not be checked. Anything uncommitted here is discarded.",
                )}
              </Consequence>
            )}
            <Consequence icon={GitBranch}>
              {tree.branch ? (
                <>
                  {t(
                    "dialogs.deleteWorktreeBranchKeptNamed",
                    "The {branch} branch and its commits are kept.",
                    { branch: tree.branch },
                  )}
                </>
              ) : (
                t("dialogs.deleteWorktreeBranchKept", "The branch is kept.")
              )}
            </Consequence>
            {!!tree.unpushed && (
              <Consequence icon={CloudUpload}>
                {tree.unpushed === 1
                  ? t(
                      "dialogs.deleteWorktreeUnpushed_one",
                      "1 commit is not on a remote. It stays on the branch.",
                    )
                  : t(
                      "dialogs.deleteWorktreeUnpushed_other",
                      "{count} commits are not on a remote. They stay on the branch.",
                      { count: tree.unpushed },
                    )}
              </Consequence>
            )}
          </ul>
        </div>
        {sessionCount > 0 && (
          <div className="flex items-center justify-between gap-3 rounded-lg border border-content/10 p-3">
            <span
              id="delete-worktree-sessions-label"
              className="text-[12.5px] text-content/75"
            >
              {t(
                "dialogs.deleteWorktreeAlsoDeleteSessions",
                "Also delete associated sessions",
              )}
            </span>
            <button
              type="button"
              role="switch"
              aria-labelledby="delete-worktree-sessions-label"
              aria-checked={deleteSessions}
              disabled={busy}
              onClick={() => setDeleteSessions(!deleteSessions)}
              className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:opacity-40 ${deleteSessions ? "bg-red-500" : "bg-content/20"}`}
            >
              <span
                className={`absolute top-0.5 size-4 rounded-full bg-white transition-[left] ${deleteSessions ? "left-4.5" : "left-0.5"}`}
              />
            </button>
          </div>
        )}
        {error && (
          <p role="alert" className="break-words text-[12.5px] text-red-400">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={onClose}
            className="rounded-md px-3 py-1.5 hover:bg-content/8 active:scale-[0.97]"
          >
            {t("common.cancel", "Cancel")}
          </button>
          <button
            type="submit"
            disabled={busy}
            className="inline-flex items-center gap-1.5 rounded-md bg-red-500/20 px-3 py-1.5 font-medium text-red-400 hover:bg-red-500/30 disabled:opacity-40 disabled:hover:bg-red-500/20 active:scale-[0.97]"
          >
            {busy && <Loader className="size-3.5 animate-spin" />}
            {sessionCount && deleteSessions
              ? sessionCount === 1
                ? t("dialogs.deleteWorktreeAndSession_one", "Delete worktree and session")
                : t("dialogs.deleteWorktreeAndSession_other", "Delete worktree and sessions")
              : t("dialogs.deleteWorktreeAction", "Delete worktree")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
