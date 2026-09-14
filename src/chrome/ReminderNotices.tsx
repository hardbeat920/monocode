import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { LAYER } from "../lib/layers";
import {
  loadNotificationsEnabled,
  NOTIFICATIONS_CHANGE_EVENT,
} from "../lib/notifications";
import { projectName } from "../lib/paths";
import { sessionDisplayTitle } from "../lib/session";
import {
  formatReminderTime,
  reminderTime,
  type SessionReminder,
} from "../lib/sessionReminders";
import { ExplorerMenu } from "./ExplorerMenu";
import { Clock } from "./icons";
import { sessionReminderPresets } from "./sessionReminderPresets";

function subscribeNotifications(callback: () => void) {
  window.addEventListener(NOTIFICATIONS_CHANGE_EVENT, callback);
  window.addEventListener("storage", callback);
  return () => {
    window.removeEventListener(NOTIFICATIONS_CHANGE_EVENT, callback);
    window.removeEventListener("storage", callback);
  };
}

export function ReminderNotices({
  reminders,
  error,
  onOpen,
  onSnooze,
  onDismiss,
  onRetry,
  onOpenSettings,
  onHeightChange,
}: {
  reminders: SessionReminder[];
  error: string | null;
  onOpen: (reminder: SessionReminder) => void;
  onSnooze: (ids: readonly string[], dueAt: number) => void;
  onDismiss: (ids: readonly string[], expectedDueAt?: number) => void;
  onRetry: () => void;
  onOpenSettings: () => void;
  onHeightChange?: (height: number) => void;
}) {
  const panelRef = useRef<HTMLElement>(null);
  const [snooze, setSnooze] = useState<{
    reminder: SessionReminder;
    x: number;
    y: number;
  } | null>(null);
  const notifications = useSyncExternalStore(
    subscribeNotifications,
    loadNotificationsEnabled,
  );
  const visible = reminders.length > 0 || error != null;
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel || !onHeightChange) return;
    const measure = () => onHeightChange(panel.offsetHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    return () => {
      observer.disconnect();
      onHeightChange(0);
    };
  }, [visible, onHeightChange]);
  if (!visible) return null;

  return createPortal(
    <>
      <section
        ref={panelRef}
        aria-label="Due reminders"
        style={{ zIndex: LAYER.popover - 1 }}
        className="fixed top-3 right-3 w-[min(320px,calc(100vw-24px))] overflow-hidden rounded-xl border border-content/15 bg-background-base/95 text-content shadow-xl backdrop-blur-xl"
      >
        <div className="flex items-center gap-2 border-b border-content/10 px-3 py-2.5">
          <Clock className="size-3.5 text-amber-400" strokeWidth={1.75} />
          <span className="flex-1 text-[12px] font-semibold">
            Due reminders
          </span>
          <span role="status" className="text-[11px] text-content/50">
            {reminders.length || ""}
          </span>
        </div>
        {error ? (
          <div role="alert" className="px-3 py-2 text-[12px] text-content/70">
            Couldn’t load reminders.{" "}
            <button className="underline" onClick={onRetry}>
              Retry
            </button>
          </div>
        ) : null}
        <div className="max-h-[min(320px,50vh)] overflow-y-auto overscroll-none divide-y divide-content/10">
          {reminders.map((reminder) => (
            <article key={reminder.sessionId} className="px-3 py-2.5">
              <button
                className="block w-full text-left"
                onClick={() => onOpen(reminder)}
              >
                <span className="block truncate text-[13px] font-medium hover:underline">
                  {sessionDisplayTitle(reminder.title, reminder.harness)}
                </span>
                <span
                  className="mt-1 block truncate text-[11px] text-content/50"
                  title={formatReminderTime(reminder.dueAt)}
                >
                  {projectName(reminder.cwd)} ·{" "}
                  {formatReminderTime(reminder.dueAt)}
                </span>
              </button>
              <div className="mt-2 flex items-center gap-1.5 text-[11px]">
                <button
                  className="rounded-md bg-content/10 px-2 py-1 hover:bg-content/15"
                  onClick={() => onOpen(reminder)}
                >
                  Open session
                </button>
                <button
                  aria-haspopup="menu"
                  aria-expanded={
                    snooze?.reminder.sessionId === reminder.sessionId
                  }
                  className="rounded-md px-2 py-1 text-content/65 hover:bg-content/10"
                  onClick={(event) => {
                    const rect = event.currentTarget.getBoundingClientRect();
                    setSnooze({ reminder, x: rect.left, y: rect.bottom + 4 });
                  }}
                >
                  Snooze
                </button>
                <button
                  className="ml-auto rounded-md px-2 py-1 text-content/50 hover:bg-content/10"
                  onClick={() =>
                    onDismiss([reminder.sessionId], reminder.dueAt)
                  }
                >
                  Dismiss
                </button>
              </div>
            </article>
          ))}
        </div>
        {!notifications && reminders.length ? (
          <button
            className="w-full border-t border-content/10 px-3 py-2 text-left text-[11px] text-content/50 hover:text-content"
            onClick={onOpenSettings}
          >
            Desktop alerts are off. Enable in Settings.
          </button>
        ) : null}
      </section>
      {snooze &&
      reminders.some(
        (reminder) =>
          reminder.sessionId === snooze.reminder.sessionId &&
          reminder.dueAt === snooze.reminder.dueAt,
      ) ? (
        <ExplorerMenu
          x={snooze.x}
          y={snooze.y}
          items={sessionReminderPresets()}
          ariaLabel="Snooze reminder"
          onClose={() => setSnooze(null)}
          onPick={(id) => {
            const dueAt = reminderTime(id);
            if (dueAt != null) onSnooze([snooze.reminder.sessionId], dueAt);
            setSnooze(null);
          }}
        />
      ) : null}
    </>,
    document.body,
  );
}
