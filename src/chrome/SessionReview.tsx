import { ChevronDown, ChevronRight } from "./icons";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  keepSessionChanges,
  sessionCheckpointStatus,
  subscribeReviewChanged,
  undoSessionChanges,
  type CheckpointFile,
} from "../lib/checkpoint";
import { invalidateProjectFiles } from "../lib/fileIndex";
import { invalidateWatchedFiles } from "../lib/fileWatch";
import { basename, notifyGitChanged, subscribeGitChanged } from "../lib/fs";
import { FileTypeIcon } from "./FileTypeIcon";

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
};

export function SessionReview({
  sessionId,
  cwd,
  enabled = true,
  busy = false,
  undoLocked = false,
  onOpenDiff,
}: Props) {
  const [files, setFiles] = useState<CheckpointFile[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [acting, setActing] = useState<"keep" | "undo" | null>(null);
  const filesRef = useRef(files);
  filesRef.current = files;

  const load = useCallback(() => {
    if (!cwd || cwd === "~") {
      setFiles([]);
      return;
    }
    void sessionCheckpointStatus(sessionId, cwd)
      .then((status) => setFiles(status.files))
      .catch(() => setFiles([]));
  }, [sessionId, cwd]);

  useEffect(() => {
    if (!enabled || busy) return;
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
    const unsubGit = subscribeGitChanged(() => {
      if (filesRef.current.length > 0) schedule();
    });
    const onResume = () => {
      if (filesRef.current.length > 0) schedule();
    };
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    return () => {
      if (timer != null) window.clearTimeout(timer);
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
      unsubReview();
      unsubGit();
    };
  }, [enabled, load, sessionId, busy]);

  useEffect(() => {
    if (files.length <= 1) setExpanded(false);
  }, [files.length]);

  if (files.length === 0) return null;

  const disabled = busy || acting != null;
  const many = files.length > 1;
  const first = files[0];
  if (!first) return null;
  const canUndoAll = !undoLocked && files.every((file) => file.undoable);

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
    <div className="px-2" data-session-review-shell>
      <div
        className="relative z-0 rounded-t-[10px] border border-b-0 border-content/10 bg-content/3 px-2 py-1"
        data-session-review
      >
        <div className="flex min-w-0 items-center gap-2">
          {many ? (
            <button
              type="button"
              title={expanded ? "Collapse files" : "Expand files"}
              aria-expanded={expanded}
              onClick={() => setExpanded((open) => !open)}
              className="flex min-w-0 flex-1 items-center gap-1 py-0.5 text-left text-content/70 hover:text-content"
            >
              {expanded ? (
                <ChevronDown className="size-3.5 shrink-0" strokeWidth={1.75} />
              ) : (
                <ChevronRight
                  className="size-3.5 shrink-0"
                  strokeWidth={1.75}
                />
              )}
              <span className="truncate text-[12px]">{files.length} Files</span>
            </button>
          ) : (
            <FileLabel
              file={first}
              sessionId={sessionId}
              cwd={cwd}
              onOpenDiff={onOpenDiff}
            />
          )}
          <div className="flex shrink-0 items-center gap-0.5">
            <button
              type="button"
              title={
                canUndoAll
                  ? "Undo all session changes"
                  : undoLocked
                    ? "Undo is unavailable while another session is running in this project"
                    : "Undo is unavailable because a file changed outside this session"
              }
              disabled={disabled || !canUndoAll}
              onClick={() => run("undo")}
              className="h-6 rounded-md px-1.5  text-[11px] text-content/55 hover:bg-content/10 hover:text-content disabled:opacity-40"
            >
              Undo All
            </button>
            <button
              type="button"
              title="Keep all session changes"
              disabled={disabled}
              onClick={() => run("keep")}
              className="h-6 rounded-md px-1.5  text-[11px] text-content/55 hover:bg-content/10 hover:text-content disabled:opacity-40"
            >
              Keep All
            </button>
            <button
              type="button"
              title="Review changes"
              onClick={() => onOpenDiff(undefined, { sessionId, cwd })}
              className="h-6 rounded-md bg-content/15 px-2 text-[11px] text-content/80 hover:bg-content/20 hover:text-content"
            >
              Review
            </button>
          </div>
        </div>
        {many && expanded ? (
          <ul className="scrollbar-none mt-1 max-h-40 overflow-y-auto">
            {files.map((file) => (
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
        ) : null}
      </div>
    </div>
  );
}

function FileLabel({
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
      className="flex min-w-0 flex-1 items-center gap-1.5 py-0.5 text-left text-content/80 hover:text-content"
    >
      <FileTypeIcon name={name} isDir={false} size={14} />
      <span className="min-w-0 truncate font-mono text-[12px]">{name}</span>
      <DiffCounts file={file} />
    </button>
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
      className="flex h-7 w-full min-w-0 items-center gap-1.5 rounded-md px-1 text-left text-content/80 hover:bg-content/10 hover:text-content"
    >
      <FileTypeIcon name={name} isDir={false} size={16} />
      <span className="min-w-0 flex-1 truncate font-mono text-[12px]">
        {name}
      </span>
      <DiffCounts file={file} />
    </button>
  );
}

function DiffCounts({ file }: { file: CheckpointFile }) {
  if (!file.exact) {
    return (
      <span className="shrink-0 text-[11px] font-medium text-amber-300/80">
        Shared file
      </span>
    );
  }
  if (file.additions <= 0 && file.deletions <= 0) return null;
  return (
    <span className="shrink-0 font-mono text-[11px] font-semibold">
      {file.additions > 0 ? (
        <span className="text-emerald-400">+{file.additions}</span>
      ) : null}
      {file.additions > 0 && file.deletions > 0 ? " " : null}
      {file.deletions > 0 ? (
        <span className="text-red-400">-{file.deletions}</span>
      ) : null}
    </span>
  );
}
