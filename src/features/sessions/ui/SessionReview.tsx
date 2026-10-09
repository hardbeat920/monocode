import { ChevronDown, ChevronRight, FileDiff } from "../../../shared/ui/icons";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  keepSessionChanges,
  sessionCheckpointStatus,
  subscribeReviewChanged,
  undoSessionChanges,
  type CheckpointFile,
} from "../model/checkpoint";
import { invalidateProjectFiles } from "../../files/model/fileIndex";
import { invalidateWatchedFiles } from "../../files/model/fileWatch";
import {
  basename,
  notifyGitChanged,
  subscribeGitChanged,
} from "../../../platform/tauri/fs";
import { formatInteger } from "../../../shared/lib/numbers";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";

type Props = {
  sessionId: string;
  cwd: string;
  enabled?: boolean;
  busy?: boolean;
  undoLocked?: boolean;
  onOpenDiff: (
    path?: string,
    session?: { sessionId: string; cwd: string },
  ) => void;
  onCommit?: (session: { sessionId: string; cwd: string }) => void;
};

export function SessionReview({
  sessionId,
  cwd,
  enabled = true,
  busy = false,
  undoLocked = false,
  onOpenDiff,
  onCommit,
}: Props) {
  const [files, setFiles] = useState<CheckpointFile[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [acting, setActing] = useState<"keep" | "undo" | null>(null);
  const filesRef = useRef(files);
  const loadGeneration = useRef(0);
  filesRef.current = files;

  const load = useCallback(() => {
    const generation = ++loadGeneration.current;
    if (!cwd || cwd === "~") {
      setFiles([]);
      setError(null);
      return;
    }
    void sessionCheckpointStatus(sessionId, cwd)
      .then((status) => {
        if (generation !== loadGeneration.current) return;
        setFiles(status.files);
        setError(null);
      })
      .catch((caught: unknown) => {
        if (generation !== loadGeneration.current) return;
        setFiles([]);
        setError(caught instanceof Error ? caught.message : String(caught));
      });
  }, [sessionId, cwd]);

  useEffect(() => {
    if (!enabled || busy) return;
    setFiles([]);
    setError(null);
    load();
    let timer: number | null = null;
    const schedule = () => {
      if (timer != null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = null;
        load();
      }, 200);
    };
    const unsubReview = subscribeReviewChanged((id) => {
      if (!id || id === sessionId) schedule();
    });
    const unsubGit = subscribeGitChanged(schedule);
    const onResume = schedule;
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    return () => {
      loadGeneration.current++;
      if (timer != null) window.clearTimeout(timer);
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
      unsubReview();
      unsubGit();
    };
  }, [enabled, load, sessionId, busy]);

  useEffect(() => {
    if (files.length <= 3) setExpanded(false);
  }, [files.length]);

  useEffect(() => {
    if (busy) {
      loadGeneration.current++;
      setFiles([]);
      setError(null);
    }
  }, [busy]);

  // The card represents the result of a turn. Keep it out of the live turn,
  // then refresh and reveal it once the turn has settled.
  if (busy) return null;
  if (error) {
    return (
      <div className="px-4 pt-1 pb-2 font-sans" data-session-review-shell>
        <div
          role="status"
          className="rounded-xl border border-content/12 bg-content/3 px-3 py-2.5"
        >
          <p className="text-[12px] font-medium text-content/80">
            Couldn’t record changes
          </p>
          <p className="mt-1 text-[11px] text-content/50">{error}</p>
        </div>
      </div>
    );
  }
  if (files.length === 0) return null;

  const disabled = acting != null;
  const canUndoAll = !undoLocked && files.every((file) => file.undoable);
  const visibleFiles = expanded ? files : files.slice(0, 3);
  const hiddenFileCount = files.length - visibleFiles.length;
  const totals = files.reduce(
    (sum, file) => ({
      additions: sum.additions + file.additions,
      deletions: sum.deletions + file.deletions,
    }),
    { additions: 0, deletions: 0 },
  );

  const run = (action: "keep" | "undo") => {
    if (disabled) return;
    setActing(action);
    const op =
      action === "keep"
        ? keepSessionChanges(sessionId, cwd)
        : undoSessionChanges(sessionId, cwd);
    const previous = filesRef.current.map((file) => file.path);
    void op
      .then((status) => {
        setFiles(status.files);
        notifyGitChanged();
        invalidateWatchedFiles(previous);
        invalidateProjectFiles(cwd);
      })
      .catch(() => load())
      .finally(() => setActing(null));
  };

  return (
    <div className="px-4 pt-1 pb-2 font-sans" data-session-review-shell>
      <div
        className="overflow-hidden rounded-xl border border-content/12 bg-content/3"
        data-session-review
      >
        <div className="flex min-w-0 items-center gap-2.5 px-3 py-2.5">
          <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-content/8 text-content/55">
            <FileDiff className="size-4" strokeWidth={1.75} />
          </span>
          <div className="min-w-0 flex-1">
            <div className="truncate text-[12px] font-medium text-content/80">
              <span title="Workspace changes recorded during the latest turn">
                Changed {files.length} {files.length === 1 ? "file" : "files"}
              </span>
            </div>
            <div className="flex items-center gap-1.5 font-sans text-[11px] font-semibold tabular-nums -mt-0.5">
              <span className="text-diff-add-fg">
                +{formatInteger(totals.additions)}
              </span>
              <span className="text-diff-del-fg">
                -{formatInteger(totals.deletions)}
              </span>
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-0.5">
            <button
              type="button"
              title={
                canUndoAll
                  ? "Undo the recorded changes"
                  : undoLocked
                    ? "Undo is unavailable while another session is running in this project"
                    : "Undo is unavailable because the workspace was shared, the branch moved, or a recorded file changed afterward"
              }
              disabled={disabled || !canUndoAll}
              onClick={() => run("undo")}
              className="h-7 rounded-md px-2.5 text-[11px] text-content/50 hover:bg-content/8 hover:text-content disabled:opacity-35"
            >
              Undo
            </button>
            <button
              type="button"
              title="Keep the recorded changes and dismiss this card"
              disabled={disabled}
              onClick={() => run("keep")}
              className="h-7 rounded-md px-2.5 text-[11px] text-content/50 hover:bg-content/8 hover:text-content disabled:opacity-35"
            >
              Keep
            </button>
            {onCommit ? (
              <button
                type="button"
                title="Commit these changes"
                onClick={() => onCommit({ sessionId, cwd })}
                className="h-7 rounded-md px-2.5 text-[11px] text-content/50 hover:bg-content/8 hover:text-content"
              >
                Commit
              </button>
            ) : null}
            <button
              type="button"
              title="Review changes"
              onClick={() => onOpenDiff(undefined, { sessionId, cwd })}
              className="h-7 rounded-md border border-content/12 bg-content/8 px-2.5 text-[11px] font-medium text-content/75 hover:bg-content/12 hover:text-content"
            >
              Review
            </button>
          </div>
        </div>
        <ul
          className={`scrollbar-none border-t border-stroke py-1 ${
            expanded ? "max-h-64 overflow-y-auto" : ""
          }`}
        >
          {visibleFiles.map((file) => (
            <li key={file.relative}>
              <FileRow
                file={file}
                sessionId={sessionId}
                cwd={cwd}
                onOpenDiff={onOpenDiff}
              />
            </li>
          ))}
        </ul>
        {files.length > 3 ? (
          <button
            type="button"
            aria-expanded={expanded}
            onClick={() => setExpanded((open) => !open)}
            className="flex h-8 w-full items-center gap-1.5 border-t border-stroke px-3 text-left text-[11px] text-content/45 hover:bg-content/5 hover:text-content/70"
          >
            {expanded ? (
              <ChevronDown className="size-3.5" strokeWidth={1.75} />
            ) : (
              <ChevronRight className="size-3.5" strokeWidth={1.75} />
            )}
            <span>
              {expanded
                ? "Show fewer files"
                : `Show ${hiddenFileCount} more ${hiddenFileCount === 1 ? "file" : "files"}`}
            </span>
          </button>
        ) : null}
      </div>
    </div>
  );
}

function FileRow({
  file,
  sessionId,
  cwd,
  onOpenDiff,
}: {
  file: CheckpointFile;
  sessionId: string;
  cwd: string;
  onOpenDiff: (
    path?: string,
    session?: { sessionId: string; cwd: string },
  ) => void;
}) {
  const name = basename(file.relative);
  return (
    <button
      type="button"
      title={file.relative}
      onClick={() => onOpenDiff(file.path, { sessionId, cwd })}
      className="flex h-8 w-full min-w-0 items-center gap-2 px-3 text-left text-content/65 hover:bg-content/5 hover:text-content"
    >
      <FileTypeIcon name={name} isDir={false} size={15} />
      <span className="min-w-0 flex-1 truncate font-mono text-[12px]">
        {file.relative}
      </span>
      <DiffCounts file={file} />
    </button>
  );
}

function DiffCounts({ file }: { file: CheckpointFile }) {
  if (!file.exact) {
    return (
      <span
        title="This older checkpoint has no exact before/after diff"
        className="shrink-0 text-[11px] font-medium text-content/45"
      >
        Diff unavailable
      </span>
    );
  }
  return (
    <span className="flex shrink-0 gap-2 font-sans text-[11px] font-semibold tabular-nums">
      <span className="text-diff-add-fg">+{formatInteger(file.additions)}</span>
      <span className="text-diff-del-fg">-{formatInteger(file.deletions)}</span>
    </span>
  );
}
