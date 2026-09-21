import {
  isProjectMuted,
  NOTIFICATION_MUTE_HOURS,
  type ProjectNotificationPreference,
} from "../model/notificationPreferences";

/** A single status label for the project rail, menus, and mute controls. */
export function notificationMuteStatus(
  preference: ProjectNotificationPreference | undefined,
): string | null {
  if (!preference || !isProjectMuted(preference)) return null;
  return preference.mutedUntil === null
    ? "Muted until resumed"
    : `Muted until ${new Date(preference.mutedUntil!).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}`;
}

/** The same preset IDs and durations are used by project and Inbox menus. */
const mutePresets = [
  ...NOTIFICATION_MUTE_HOURS.map((hours) => ({
    kind: "item" as const,
    id: `mute:${hours}`,
    label: `${hours} ${hours === 1 ? "hour" : "hours"}`,
    milliseconds: hours * 3_600_000,
  })),
  {
    kind: "item" as const,
    id: "mute:indefinite",
    label: "Until resumed",
    milliseconds: null,
  },
  { kind: "item" as const, id: "mute:custom", label: "Choose date and time" },
];

export function notificationMuteActions(now = new Date(Date.now())) {
  return mutePresets.map((action) => {
    if (action.milliseconds == null) return action;
    const until = new Date(now.getTime() + action.milliseconds);
    const time = `${until.getHours()}:${String(until.getMinutes()).padStart(2, "0")}`;
    const day = until.toDateString() === now.toDateString() ? "" : "Tomorrow, ";
    return { ...action, label: `${action.label} (${day}${time})` };
  });
}

export function notificationMuteDeadline(
  id: string,
): number | null | undefined {
  const action = mutePresets.find((item) => item.id === id);
  if (!action || action.milliseconds === undefined) return undefined;
  return action.milliseconds === null ? null : Date.now() + action.milliseconds;
}
