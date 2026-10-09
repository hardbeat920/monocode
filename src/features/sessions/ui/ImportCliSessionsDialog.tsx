import { useEffect, useMemo, useState } from "react";
import { formatRelativeTime } from "../../inbox/model/githubTasks";
import {
  listCliSessions,
  type CliSession,
} from "../../../platform/tauri/cliSessions";
import { Modal } from "../../../shared/ui/Modal";
import {
  importCliSession,
  notifyCliSessionsImported,
} from "../model/cliImport";
import { HARNESS_TITLE } from "../model/session";
import { HarnessIcon } from "./HarnessIcon";

type Props = {
  /** Project folder whose terminal sessions are listed. */
  cwd: string;
  name: string;
  onClose: () => void;
};

type Progress = { done: number; total: number; failed: number };

/** `updatedAt` comes from files on disk; a bad value must not break render. */
function validDate(ms: number): Date | null {
  if (!Number.isFinite(ms)) return null;
  const date = new Date(ms);
  return Number.isNaN(date.getTime()) ? null : date;
}

const sessionKey = (session: CliSession) =>
  `${session.harness}:${session.providerSessionId}`;

/**
 * Lists the Claude Code, Codex and Grok sessions recorded for a project
 * folder in the terminal and imports the chosen ones into its history.
 */
export function ImportCliSessionsDialog({ cwd, name, onClose }: Props) {
  const [sessions, setSessions] = useState<CliSession[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [progress, setProgress] = useState<Progress | null>(null);

  useEffect(() => {
    let cancelled = false;
    void listCliSessions(cwd)
      .then((rows) => {
        if (cancelled) return;
        setSessions(rows);
        setSelected(new Set(rows.map(sessionKey)));
      })
      .catch((reason: unknown) => {
        if (!cancelled)
          setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [cwd]);

  const chosen = useMemo(
    () => (sessions ?? []).filter((row) => selected.has(sessionKey(row))),
    [selected, sessions],
  );
  const importing = progress != null && progress.done < progress.total;
  const allSelected =
    sessions != null &&
    sessions.length > 0 &&
    selected.size === sessions.length;

  const toggle = (session: CliSession) => {
    const next = new Set(selected);
    const key = sessionKey(session);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    setSelected(next);
  };

  const runImport = async () => {
    const total = chosen.length;
    let done = 0;
    let failed = 0;
    setProgress({ done, total, failed });
    // One at a time: each read can be a large file, and the store writes
    // under one lock anyway.
    for (const session of chosen) {
      try {
        await importCliSession(session, cwd);
      } catch {
        failed += 1;
      }
      done += 1;
      setProgress({ done, total, failed });
    }
    notifyCliSessionsImported(cwd);
    if (failed === 0) onClose();
  };

  return (
    <Modal
      title="Import terminal sessions"
      description={`Claude Code, Codex and Grok sessions started in ${name} outside MonoCode. Sending a message resumes the same session, so it stays available in the CLI too.`}
      onClose={() => {
        if (!importing) onClose();
      }}
      fitViewport
    >
      <div className="flex min-h-0 flex-col gap-3 p-4 text-[12px]">
        {error ? (
          <p role="alert" className="text-red-400">
            {error}
          </p>
        ) : sessions == null ? (
          <p className="text-content/55">Looking for sessions…</p>
        ) : sessions.length === 0 ? (
          <p className="text-content/55">
            No terminal sessions found for this folder that are not already
            in MonoCode.
          </p>
        ) : (
          <>
            <label className="flex items-center gap-2 text-content/70">
              <input
                type="checkbox"
                checked={allSelected}
                disabled={importing}
                onChange={() =>
                  setSelected(
                    allSelected ? new Set() : new Set(sessions.map(sessionKey)),
                  )
                }
                className="accent-accent"
              />
              {sessions.length === 1
                ? "1 session"
                : `${sessions.length} sessions`}
            </label>
            <ul className="-mx-1 max-h-[50vh] overflow-y-auto overscroll-none">
              {sessions.map((session) => (
                <li key={sessionKey(session)}>
                  <label className="flex cursor-default items-center gap-2 rounded-md px-1 py-1.5 hover:bg-content/5">
                    <input
                      type="checkbox"
                      checked={selected.has(sessionKey(session))}
                      disabled={importing}
                      onChange={() => toggle(session)}
                      className="accent-accent"
                    />
                    <HarnessIcon
                      harness={session.harness}
                      className="size-3.5 shrink-0"
                    />
                    <span className="min-w-0 flex-1 truncate" title={session.title}>
                      {session.title}
                    </span>
                    <UpdatedAt session={session} />
                  </label>
                </li>
              ))}
            </ul>
          </>
        )}
        {progress && progress.failed > 0 && !importing ? (
          <p role="alert" className="text-red-400">
            {progress.failed === 1
              ? "1 session could not be imported."
              : `${progress.failed} sessions could not be imported.`}
          </p>
        ) : null}
        <div className="flex items-center justify-end gap-2">
          {importing ? (
            <span className="mr-auto text-content/55">
              Importing {progress.done + 1} of {progress.total}…
            </span>
          ) : null}
          <button
            type="button"
            onClick={onClose}
            disabled={importing}
            className="rounded-md px-3 py-1.5 hover:bg-content/8 active:scale-[0.97] disabled:opacity-50"
          >
            {progress && !importing ? "Close" : "Cancel"}
          </button>
          <button
            type="button"
            onClick={() => void runImport()}
            disabled={importing || chosen.length === 0}
            className="rounded-md bg-accent/20 px-3 py-1.5 font-medium text-accent hover:bg-accent/30 active:scale-[0.97] disabled:opacity-50"
          >
            {chosen.length === 1
              ? "Import 1 session"
              : `Import ${chosen.length} sessions`}
          </button>
        </div>
      </div>
    </Modal>
  );
}

function UpdatedAt({ session }: { session: CliSession }) {
  const date = validDate(session.updatedAt);
  return (
    <span
      className="shrink-0 text-[11px] text-content/45"
      title={
        date
          ? `${HARNESS_TITLE[session.harness]} · ${date.toLocaleString()}`
          : HARNESS_TITLE[session.harness]
      }
    >
      {date ? formatRelativeTime(date.toISOString()) : ""}
    </span>
  );
}
