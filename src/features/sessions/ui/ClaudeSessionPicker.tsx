import { useEffect, useState } from "react";
import { ModalPanel } from "../../../shared/ui/Modal";
import {
  claudeSessions,
  type ClaudeSessionSummary,
} from "../../../platform/tauri/fs";

type Props = {
  cwd: string;
  onPick: (summary: ClaudeSessionSummary) => void;
  onClose: () => void;
};

/** Conversations Claude Code already recorded for this project, newest first. */
export function ClaudeSessionPicker({ cwd, onPick, onClose }: Props) {
  const [sessions, setSessions] = useState<ClaudeSessionSummary[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    claudeSessions(cwd)
      .then((list) => {
        if (!cancelled) setSessions(list);
      })
      .catch(() => {
        // A project Claude Code has never run in simply has nothing to show.
        if (!cancelled) setSessions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [cwd]);

  return (
    <ModalPanel
      onClose={onClose}
      title="Resume a Claude Code conversation"
      description="Picked up from where Claude Code left off in this project."
      fitViewport
    >
      {sessions === null ? (
        <p className="px-1 py-6 text-center text-sm opacity-60">Reading…</p>
      ) : sessions.length === 0 ? (
        <p className="px-1 py-6 text-center text-sm opacity-60">
          Claude Code has not recorded a conversation in this project yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {sessions.map((session) => (
            <li key={session.id}>
              <button
                type="button"
                className="flex w-full flex-col items-start gap-0.5 rounded-lg px-3 py-2 text-left hover:bg-black/5 dark:hover:bg-white/10"
                onClick={() => onPick(session)}
              >
                <span className="line-clamp-2 text-sm">{session.title}</span>
                <span className="text-xs opacity-60">
                  {formatWhen(session.updatedAt)} ·{" "}
                  {session.messageCount === 1
                    ? "1 message"
                    : `${session.messageCount} messages`}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </ModalPanel>
  );
}

function formatWhen(at: number): string {
  if (!at) return "unknown date";
  const date = new Date(at);
  const today = new Date();
  const sameDay = date.toDateString() === today.toDateString();
  return sameDay
    ? date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
}
