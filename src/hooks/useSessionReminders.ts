import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { message } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  loadNotificationsEnabled,
  NOTIFICATIONS_CHANGE_EVENT,
} from "../lib/notifications";
import { loadSoundsEnabled, SOUNDS_CHANGE_EVENT } from "../lib/sounds";
import {
  getProjectNotificationRule,
  subscribeNotificationPreferences,
} from "../lib/notificationPreferences";
import {
  knownNotificationProject,
} from "../lib/notificationProjects";
import {
  clearReminders,
  listReminders,
  REMINDER_OPEN,
  REMINDERS_CHANGED,
  setReminders,
  type ReminderTarget,
  type SessionReminder,
} from "../lib/sessionReminders";

export function useSessionReminders(
  onOpenSession: (sessionId: string) => Promise<void>,
  ensureSaved: (sessionIds: readonly string[]) => Promise<void>,
  openSessionIds: readonly string[],
) {
  const [reminders, setItems] = useState<SessionReminder[]>([]);
  const [now, setNow] = useState(Date.now);
  const [error, setError] = useState<string | null>(null);
  const [, refreshPolicy] = useReducer((revision: number) => revision + 1, 0);
  const revision = useRef(0);
  const configurationRevision = useRef(0);
  const configurationQueue = useRef<Promise<void>>(Promise.resolve());
  const remindersRef = useRef(reminders);
  remindersRef.current = reminders;
  const callbacks = useRef({ onOpenSession, ensureSaved });
  callbacks.current = { onOpenSession, ensureSaved };

  const configure = useCallback(async () => {
    const request = ++configurationRevision.current;
    // Native preferences are global state. Keep writes ordered and skip queued
    // snapshots superseded by another window event or a newer project policy.
    const pending = configurationQueue.current
      .catch(() => {})
      .then(async () => {
        if (request !== configurationRevision.current) return;
        await invoke("reminder_configure", {
          preferences: {
            notificationsEnabled: loadNotificationsEnabled(),
            sound: loadSoundsEnabled(),
            projectRules: Object.fromEntries(
              remindersRef.current.flatMap((reminder) => {
                const project = knownNotificationProject(reminder.cwd);
                return project
                  ? [
                      [
                        reminder.sessionId,
                        getProjectNotificationRule(project.id, "reminders"),
                      ],
                    ]
                  : [];
              }),
            ),
          },
        });
      });
    configurationQueue.current = pending;
    try {
      await pending;
    } catch (error) {
      if (request === configurationRevision.current) throw error;
    }
  }, []);

  const refresh = useCallback(async () => {
    const request = ++revision.current;
    try {
      const items = await listReminders();
      if (request !== revision.current) return;
      remindersRef.current = items;
      setItems(items);
      setNow(Date.now());
      setError(null);
      await configure();
    } catch (error) {
      if (request === revision.current) setError(String(error));
    }
  }, [configure]);

  const report = (error: unknown) => {
    void message(String(error), { title: "Reminder", kind: "error" });
  };

  const schedule = useCallback(
    async (ids: readonly string[], dueAt: number) => {
      try {
        await callbacks.current.ensureSaved(ids);
        await setReminders(ids, dueAt);
        await refresh();
      } catch (error) {
        report(error);
      }
    },
    [refresh],
  );

  const cancel = useCallback(
    async (ids: readonly string[], expectedDueAt?: number) => {
      try {
        await clearReminders(ids, expectedDueAt);
        await refresh();
      } catch (error) {
        report(error);
      }
    },
    [refresh],
  );

  const dismissDue = useCallback(
    async (sessionId: string) => {
      const reminder = remindersRef.current.find(
        (item) => item.sessionId === sessionId && item.dueAt <= Date.now(),
      );
      if (!reminder) return;
      await cancel([sessionId], reminder.dueAt);
    },
    [cancel],
  );

  const openHere = useCallback(
    async (reminder: ReminderTarget) => {
      try {
        await callbacks.current.onOpenSession(reminder.sessionId);
        // An old notification must never clear a newer reminder for this session.
        await clearReminders([reminder.sessionId], reminder.dueAt);
        await refresh();
      } catch (error) {
        report(error);
      }
    },
    [refresh],
  );

  useEffect(() => {
    let disposed = false;
    const subscriptions: Array<() => void> = [];
    const configureCurrent = () => {
      refreshPolicy();
      void configure().catch((error) => {
        if (!disposed) setError(String(error));
      });
    };
    const takeOpen = async () => {
      if (disposed) return;
      try {
        const request = await invoke<ReminderTarget | null>(
          "reminder_take_open",
        );
        if (request && !disposed) await openHere(request);
      } catch (error) {
        if (!disposed) setError(String(error));
      }
    };
    const subscribe = async (event: string, handler: () => void) => {
      const unlisten = await listen(event, handler);
      if (disposed) unlisten();
      else subscriptions.push(unlisten);
    };
    void (async () => {
      try {
        await Promise.all([
          subscribe(REMINDERS_CHANGED, () => {
            void refresh();
          }),
          subscribe(REMINDER_OPEN, () => {
            void takeOpen();
          }),
        ]);
        if (disposed) return;
        await refresh();
        await takeOpen();
      } catch (error) {
        if (!disposed) setError(String(error));
      }
    })();
    const onFocus = () => {
      void refresh();
      void takeOpen();
    };
    const onVisible = () => {
      if (!document.hidden) onFocus();
    };
    // The backend owns firing. This refresh catches renamed/deleted sessions,
    // missed events, and a machine waking after its scheduled time.
    const timer = window.setInterval(() => {
      void refresh();
    }, 30_000);
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener(NOTIFICATIONS_CHANGE_EVENT, configureCurrent);
    window.addEventListener(SOUNDS_CHANGE_EVENT, configureCurrent);
    window.addEventListener("storage", configureCurrent);
    const unsubscribePreferences =
      subscribeNotificationPreferences(configureCurrent);
    return () => {
      disposed = true;
      unsubscribePreferences();
      revision.current++;
      configurationRevision.current++;
      subscriptions.forEach((unlisten) => unlisten());
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener(NOTIFICATIONS_CHANGE_EVENT, configureCurrent);
      window.removeEventListener(SOUNDS_CHANGE_EVENT, configureCurrent);
      window.removeEventListener("storage", configureCurrent);
    };
  }, [openHere, refresh, configure]);

  const sessionIdsKey = JSON.stringify(openSessionIds);
  useEffect(() => {
    void invoke("reminder_register_window", {
      sessionIds: JSON.parse(sessionIdsKey),
    }).catch((error) => setError(String(error)));
  }, [sessionIdsKey]);

  const open = useCallback(async (reminder: ReminderTarget) => {
    try {
      await invoke("reminder_open", {
        sessionId: reminder.sessionId,
        dueAt: reminder.dueAt,
      });
    } catch (error) {
      report(error);
    }
  }, []);

  return {
    reminders,
    due: reminders.filter((reminder) => {
      const project = knownNotificationProject(reminder.cwd);
      if (!project || reminder.dueAt > now) return false;
      const rule = getProjectNotificationRule(project.id, "reminders");
      return rule.enabled && reminder.dueAt > rule.after;
    }),
    error,
    refresh,
    schedule,
    cancel,
    dismissDue,
    open,
  };
}
