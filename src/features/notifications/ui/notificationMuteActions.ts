import {
  isProjectMuted,
  NOTIFICATION_MUTE_HOURS,
  type ProjectNotificationPreference,
} from "../model/notificationPreferences";
import { getLocaleTag, t } from "../../../shared/i18n/locale";

/** A single status label for the project rail, menus, and mute controls. */
export function notificationMuteStatus(
  preference: ProjectNotificationPreference | undefined,
): string | null {
  if (!preference || !isProjectMuted(preference)) return null;
  return preference.mutedUntil === null
    ? t("settings.notifications.mutedUntilResumed")
    : t("settings.notifications.mutedUntil", {
        date: new Date(preference.mutedUntil!).toLocaleString(getLocaleTag(), {
          dateStyle: "medium",
          timeStyle: "short",
        }),
      });
}

/** The same preset IDs and durations are used by project and Inbox menus. */
const mutePresets = () => [
  ...NOTIFICATION_MUTE_HOURS.map((hours) => ({
    kind: "item" as const,
    id: `mute:${hours}`,
    label: t("settings.notifications.muteHours", { count: hours }),
    milliseconds: hours * 3_600_000,
  })),
  {
    kind: "item" as const,
    id: "mute:indefinite",
    label: t("settings.notifications.untilResumed"),
    milliseconds: null,
  },
  {
    kind: "item" as const,
    id: "mute:custom",
    label: t("settings.notifications.chooseDateTime"),
  },
];

export function notificationMuteActions(now = new Date(Date.now())) {
  return mutePresets().map((action) => {
    if (action.milliseconds == null) return action;
    const until = new Date(now.getTime() + action.milliseconds);
    const time = `${until.getHours()}:${String(until.getMinutes()).padStart(2, "0")}`;
    const day =
      until.toDateString() === now.toDateString()
        ? ""
        : t("settings.notifications.tomorrow");
    return { ...action, label: `${action.label} (${day}${time})` };
  });
}

export function notificationMuteDeadline(
  id: string,
): number | null | undefined {
  const action = mutePresets().find((item) => item.id === id);
  if (!action || action.milliseconds === undefined) return undefined;
  return action.milliseconds === null ? null : Date.now() + action.milliseconds;
}
