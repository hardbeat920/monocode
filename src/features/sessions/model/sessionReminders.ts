import { invoke } from "@tauri-apps/api/core";
import type { HarnessId } from "./session";

export const REMINDERS_CHANGED = "monocode:reminders-changed";
export const REMINDER_OPEN = "monocode:reminder-open";

export type SessionReminder = {
  sessionId: string;
  dueAt: number;
  firedAt: number | null;
  title: string;
  harness: HarnessId;
  cwd: string;
};

export type ReminderTarget = Pick<SessionReminder, "sessionId" | "dueAt">;

export function reminderTime(preset: string, now = new Date()): number | null {
  if (preset === "reminder:1h") return now.getTime() + 60 * 60 * 1000;
  if (preset === "reminder:3h") return now.getTime() + 3 * 60 * 60 * 1000;
  const date = new Date(now);
  if (preset === "reminder:evening") {
    date.setHours(18, 0, 0, 0);
  } else if (preset === "reminder:tomorrow") {
    date.setDate(date.getDate() + 1);
    date.setHours(9, 0, 0, 0);
  } else if (preset === "reminder:next-week") {
    date.setDate(date.getDate() + ((8 - date.getDay()) % 7 || 7));
    date.setHours(9, 0, 0, 0);
  } else {
    return null;
  }
  return date.getTime() > now.getTime() ? date.getTime() : null;
}

export function formatReminderTime(dueAt: number): string {
  return new Date(dueAt).toLocaleString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

export function listReminders(): Promise<SessionReminder[]> {
  return invoke("reminder_list");
}

export function setReminders(
  sessionIds: readonly string[],
  dueAt: number,
): Promise<void> {
  return invoke("reminder_set", { sessionIds, dueAt });
}

export function clearReminders(
  sessionIds: readonly string[],
  expectedDueAt?: number,
): Promise<void> {
  return invoke("reminder_clear", {
    sessionIds,
    expectedDueAt: expectedDueAt ?? null,
  });
}
