import { loadLocale, translate } from "../../../shared/i18n/locale";
import {
  isProjectMuted,
  NOTIFICATION_MUTE_HOURS,
  type ProjectNotificationPreference,
} from "../model/notificationPreferences";

function formatMuteUntil(when: number): string {
  return new Date(when).toLocaleString(
    loadLocale() === "zh-CN" ? "zh-CN" : undefined,
    { dateStyle: "medium", timeStyle: "short" },
  );
}

/** A single status label for the project rail, menus, and mute controls. */
export function notificationMuteStatus(
  preference: ProjectNotificationPreference | undefined,
): string | null {
  if (!preference || !isProjectMuted(preference)) return null;
  return preference.mutedUntil === null
    ? translate("Muted until resumed")
    : translate("Muted until {when}", {
        when: formatMuteUntil(preference.mutedUntil!),
      });
}

/** The same preset IDs and durations are used by project and Inbox menus. */
const mutePresets = [
  ...NOTIFICATION_MUTE_HOURS.map((hours) => ({
    kind: "item" as const,
    id: `mute:${hours}`,
    hours,
    milliseconds: hours * 3_600_000,
  })),
  {
    kind: "item" as const,
    id: "mute:indefinite",
    hours: null,
    milliseconds: null,
  },
  {
    kind: "item" as const,
    id: "mute:custom",
    hours: null,
    milliseconds: undefined,
  },
];

function mutePresetLabel(hours: number): string {
  return hours === 1
    ? translate("{hours} hour", { hours })
    : translate("{hours} hours", { hours });
}

export function notificationMuteActions(now = new Date(Date.now())) {
  return mutePresets.map((action) => {
    if (action.id === "mute:custom") {
      return {
        kind: "item" as const,
        id: action.id,
        label: translate("Choose date and time"),
      };
    }
    if (action.milliseconds == null) {
      return {
        kind: "item" as const,
        id: action.id,
        label: translate("Until resumed"),
      };
    }
    const until = new Date(now.getTime() + action.milliseconds);
    const time = `${until.getHours()}:${String(until.getMinutes()).padStart(2, "0")}`;
    const day =
      until.toDateString() === now.toDateString() ? "" : translate("Tomorrow, ");
    return {
      kind: "item" as const,
      id: action.id,
      label: `${mutePresetLabel(action.hours!)} (${day}${time})`,
    };
  });
}

export function notificationMuteDeadline(
  id: string,
): number | null | undefined {
  const action = mutePresets.find((item) => item.id === id);
  if (!action || action.milliseconds === undefined) return undefined;
  return action.milliseconds === null ? null : Date.now() + action.milliseconds;
}
