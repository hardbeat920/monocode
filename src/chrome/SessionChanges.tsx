import { Check, RefreshCw, Undo2 } from "lucide-react";
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
import { notifyGitChanged, subscribeGitChanged } from "../lib/fs";
import { FileTypeIcon } from "./FileTypeIcon";
import {
  DiffCounts,
  dirname,
  IconAction,
  statusColor,
  statusLetter,
} from "./changeParts";

type Props = {
  sessionId: string;
  cwd: string;
  enabled: boolean;
  selectedPath?: string;
  onOpenFile: (path: string) => void;
};

/**
 * Files this session changed, measured against the snapshot taken when it
 * started rather than HEAD, so pre-existing dirt stays out of the list.
 */
export function SessionChanges({
  sessionId,
  cwd,
  enabled,
  selectedPath,
  onOpenFile,
}: Props) {
  const [files, setFiles] = useState<CheckpointFile[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
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
  }, [cwd, sessionId]);

  useEffect(() => {
    if (!enabled) return;
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
    const onResume = () => {
      if (!document.hidden) schedule();
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
  }, [enabled, load, sessionId]);

  const run = useCallback(
    (action: "keep" | "undo", relative?: string) => {
      setBusy(relative ?? "*");
      const touched = relative
        ? filesRef.current
            .filter((file) => file.relative === relative)
            .map((file) => file.path)
        : filesRef.current.map((file) => file.path);
      const op =
        action === "keep"
          ? keepSessionChanges(sessionId, cwd, relative)
          : undoSessionChanges(sessionId, cwd, relative);
      void op
        .then((status) => {
          setFiles(status.files);
          notifyGitChanged();
          invalidateWatchedFiles(touched);
          invalidateProjectFiles(cwd);
        })
        .catch(() => load())
        .finally(() => setBusy(null));
    },
    [cwd, load, sessionId],
  );

  const additions = files.reduce((sum, file) => sum + file.additions, 0);
  const deletions = files.reduce((sum, file) => sum + file.deletions, 0);

  if (files.length === 0) {
    return (
      <p className="px-3 py-2 text-[12px] text-content/50">
        No changes from this session
      </p>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="group flex h-7 shrink-0 items-center gap-1 px-2">
        <span className="min-w-0 truncate text-[10px] font-semibold tracking-[0.04em] text-content/55 uppercase">
          {files.length} {files.length === 1 ? "File" : "Files"}
        </span>
        <span className="ml-auto flex items-center gap-1">
          <DiffCounts additions={additions} deletions={deletions} />
          <span className="flex opacity-0 group-hover:opacity-100 group-focus-within:opacity-100">
            <IconAction
              title="Undo all session changes"
              disabled={busy != null}
              onClick={() => run("undo")}
            >
              <Undo2 className="size-3.5" strokeWidth={1.75} />
            </IconAction>
            <IconAction
              title="Keep all session changes"
              disabled={busy != null}
              onClick={() => run("keep")}
            >
              <Check className="size-3.5" strokeWidth={1.75} />
            </IconAction>
            <IconAction title="Refresh" onClick={load}>
              <RefreshCw className="size-3.5" strokeWidth={1.75} />
            </IconAction>
          </span>
        </span>
      </div>
      <ul className="scrollbar-thin min-h-0 flex-1 overflow-y-auto">
        {files.map((file) => (
          <SessionRow
            key={file.relative}
            file={file}
            active={file.path === selectedPath}
            busy={busy != null}
            onOpenFile={onOpenFile}
            onAction={run}
          />
        ))}
      </ul>
    </div>
  );
}

function SessionRow({
  file,
  active,
  busy,
  onOpenFile,
  onAction,
}: {
  file: CheckpointFile;
  active: boolean;
  busy: boolean;
  onOpenFile: (path: string) => void;
  onAction: (action: "keep" | "undo", relative: string) => void;
}) {
  const name = file.relative.slice(file.relative.lastIndexOf("/") + 1);
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
          className={`shrink-0 items-center ${
            active ? "flex" : "hidden group-focus-within:flex group-hover:flex"
          }`}
        >
          <IconAction
            title="Undo this file"
            disabled={busy}
            onClick={() => onAction("undo", file.relative)}
          >
            <Undo2 className="size-3.5" strokeWidth={1.75} />
          </IconAction>
          <IconAction
            title="Keep this file"
            disabled={busy}
            onClick={() => onAction("keep", file.relative)}
          >
            <Check className="size-3.5" strokeWidth={1.75} />
          </IconAction>
        </div>
        <DiffCounts additions={file.additions} deletions={file.deletions} />
        <span
          className={`w-3.5 shrink-0 text-right font-mono text-[11px] font-semibold ${statusColor(file.status)}`}
        >
          {statusLetter(file.status)}
        </span>
      </div>
    </li>
  );
}
