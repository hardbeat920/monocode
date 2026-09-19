import {
  useEffect,
  useRef,
  useState,
  type ComponentType,
  type FormEvent,
  type ReactNode,
} from "react";
import { prettyCwd, projectName } from "../lib/paths";
import { type Worktree } from "../lib/worktrees";
import { Modal } from "./Modal";
import {
  CircleAlert,
  CloudUpload,
  FileDiff,
  Folder,
  GitBranch,
  Loader,
  MessageSquare,
} from "./icons";

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
  const [busy, setBusy] = useState(false);
  const [deleteSessions, setDeleteSessions] = useState(false);
  const [confirmation, setConfirmation] = useState("");
  const [error, setError] = useState<string>();
  const input = useRef<HTMLInputElement>(null);
  // The modal focuses its close button on mount, so claim focus on the next frame.
  useEffect(() => {
    const frame = requestAnimationFrame(() => input.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, []);
  const folder = projectName(tree.path);
  const confirmed =
    confirmation.trim().toLowerCase() === folder.toLowerCase() && !!folder;
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || !confirmed) return;
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
      title="Delete worktree?"
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
          This permanently deletes the working copy and everything inside it.
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
                {sessionCount} session{sessionCount === 1 ? "" : "s"} using this
                worktree {sessionCount === 1 ? "is" : "are"}{" "}
                {deleteSessions
                  ? "permanently deleted."
                  : "kept. Select a branch or worktree to continue them."}
              </Consequence>
            )}
            {tree.dirty && (
              <Consequence icon={FileDiff} tone="warn">
                All uncommitted and untracked changes here are discarded.
              </Consequence>
            )}
            {tree.dirty == null && (
              <Consequence icon={CircleAlert} tone="warn">
                Changes could not be checked. Anything uncommitted here is
                discarded.
              </Consequence>
            )}
            <Consequence icon={GitBranch}>
              {tree.branch ? (
                <>
                  The{" "}
                  <span className="font-medium text-content">
                    {tree.branch}
                  </span>{" "}
                  branch and its commits are kept.
                </>
              ) : (
                "The branch is kept."
              )}
            </Consequence>
            {!!tree.unpushed && (
              <Consequence icon={CloudUpload}>
                {tree.unpushed} commit{tree.unpushed === 1 ? " is" : "s are"}{" "}
                not on a remote. They stay on the branch.
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
              Also delete associated sessions
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
        <label className="flex flex-col gap-1.5">
          <span className="text-[12.5px] text-content/70">
            Type <span className="font-mono text-content">{folder}</span> to
            confirm
          </span>
          <input
            ref={input}
            aria-label={`Type ${folder} to confirm`}
            className="h-9 rounded-md border border-content/10 bg-background-base px-2.5 font-mono text-[13px] outline-none placeholder:text-content/25 focus:border-content/25 disabled:opacity-50"
            value={confirmation}
            disabled={busy}
            placeholder={folder}
            autoComplete="off"
            autoCorrect="off"
            spellCheck={false}
            onChange={(e) => setConfirmation(e.target.value)}
          />
        </label>
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
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy || !confirmed}
            className="inline-flex items-center gap-1.5 rounded-md bg-red-500/20 px-3 py-1.5 font-medium text-red-400 hover:bg-red-500/30 disabled:opacity-40 disabled:hover:bg-red-500/20 active:scale-[0.97]"
          >
            {busy && <Loader className="size-3.5 animate-spin" />}
            {sessionCount && deleteSessions
              ? `Delete worktree and session${sessionCount === 1 ? "" : "s"}`
              : "Delete worktree"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
