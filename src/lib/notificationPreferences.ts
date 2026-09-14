export const NOTIFICATION_CATEGORIES = [
  { id: "pullRequests", label: "Pull requests / Merge requests" },
  { id: "issues", label: "Issues and Linear tasks" },
  { id: "agentFinished", label: "Agent finished" },
  { id: "agentInput", label: "Agent approvals and questions" },
  { id: "reminders", label: "Reminders" },
] as const;

export type NotificationCategory =
  (typeof NOTIFICATION_CATEGORIES)[number]["id"];
export const NOTIFICATION_MUTE_HOURS = [1, 4, 8] as const;
export type NotificationSubject = {
  projectId: string;
  category: NotificationCategory;
  occurredAt?: number;
};
export type ProjectNotificationPreference = {
  disabled: NotificationCategory[];
  /** null means muted until manually resumed; absent means no override. */
  mutedUntil?: number | null;
  /** Suppress delayed activity from before a manual resume. */
  resumedAt?: number;
  enabledAfter?: Partial<Record<NotificationCategory, number>>;
};
type Preferences = Record<string, ProjectNotificationPreference>;
const KEY = "monocode.projectNotifications.v1";
const CHANGE = "monocode:project-notifications-change";

/** `after` is the last suppressed millisecond, shared with native delivery. */
export type ProjectNotificationRule = { enabled: boolean; after: number };

export function getProjectNotificationRule(
  projectId: string,
  category: NotificationCategory,
): ProjectNotificationRule {
  const preference = loadNotificationPreferences()[projectId];
  if (!preference) return { enabled: true, after: 0 };
  return {
    enabled:
      preference.mutedUntil !== null && !preference.disabled.includes(category),
    after: Math.max(
      preference.resumedAt ?? 0,
      // A scheduled event at the deadline belongs to the resumed interval.
      (preference.mutedUntil ?? 1) - 1,
      preference.enabledAfter?.[category] ?? 0,
    ),
  };
}

export function loadNotificationPreferences(): Preferences {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
      return {};
    const entries = Object.entries(parsed).flatMap(([id, value]) => {
      if (!id || !value || typeof value !== "object" || Array.isArray(value))
        return [];
      const disabled = Array.isArray(value.disabled)
        ? NOTIFICATION_CATEGORIES.map(({ id }) => id).filter((category) =>
            value.disabled.includes(category),
          )
        : [];
      const mutedUntil = value.mutedUntil;
      const validMute =
        mutedUntil === null ||
        (typeof mutedUntil === "number" &&
          Number.isFinite(mutedUntil) &&
          mutedUntil >= 0);
      const resumedAt = value.resumedAt;
      const enabledAfter = Object.fromEntries(
        NOTIFICATION_CATEGORIES.flatMap(({ id: category }) => {
          const timestamp = value.enabledAfter?.[category];
          return typeof timestamp === "number" &&
            Number.isFinite(timestamp) &&
            timestamp >= 0
            ? [[category, timestamp]]
            : [];
        }),
      );
      return [
        [
          id,
          {
            disabled,
            ...(validMute ? { mutedUntil } : {}),
            ...(typeof resumedAt === "number" &&
            Number.isFinite(resumedAt) &&
            resumedAt >= 0
              ? { resumedAt }
              : {}),
            ...(Object.keys(enabledAfter).length ? { enabledAfter } : {}),
          },
        ],
      ];
    });
    return Object.fromEntries(entries);
  } catch {
    return {};
  }
}

export function updateNotificationPreferences(
  projectIds: readonly string[],
  patch: Partial<ProjectNotificationPreference>,
) {
  const preferences = loadNotificationPreferences();
  for (const id of projectIds) {
    const previous = preferences[id];
    const enabled =
      patch.disabled === undefined
        ? []
        : (previous?.disabled ?? []).filter(
            (category) => !patch.disabled!.includes(category),
          );
    const enabledAfter = enabled.length
      ? {
          ...previous?.enabledAfter,
          ...Object.fromEntries(
            enabled.map((category) => [category, Date.now()]),
          ),
        }
      : previous?.enabledAfter;
    const resuming =
      "mutedUntil" in patch &&
      patch.mutedUntil === undefined &&
      previous?.mutedUntil !== undefined;
    preferences[id] = {
      ...(previous ?? { disabled: [] }),
      ...patch,
      ...(resuming ? { resumedAt: Date.now() } : {}),
      ...(enabledAfter ? { enabledAfter } : {}),
    };
  }
  localStorage.setItem(KEY, JSON.stringify(preferences));
  if (typeof window !== "undefined") window.dispatchEvent(new Event(CHANGE));
}

export function allowsProjectNotification(
  subject: NotificationSubject,
  now = Date.now(),
): boolean {
  const rule = getProjectNotificationRule(subject.projectId, subject.category);
  return (
    rule.enabled &&
    now > rule.after &&
    (subject.occurredAt === undefined || subject.occurredAt > rule.after)
  );
}

export function isProjectMuted(
  preference: ProjectNotificationPreference,
  now = Date.now(),
): boolean {
  return preference.mutedUntil === null || (preference.mutedUntil ?? 0) > now;
}

/** Include effective mute state so mounted controls update when a deadline passes. */
export function notificationPreferencesSnapshot(): string {
  const preferences = loadNotificationPreferences();
  return JSON.stringify({
    preferences,
    muted: Object.keys(preferences).filter((id) =>
      isProjectMuted(preferences[id]),
    ),
  });
}

export function subscribeNotificationPreferences(
  listener: () => void,
): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const schedule = () => {
    clearTimeout(timer);
    const now = Date.now();
    const deadlines = Object.values(loadNotificationPreferences())
      .map((preference) => preference.mutedUntil)
      .filter(
        (deadline): deadline is number =>
          typeof deadline === "number" && deadline > now,
      );
    if (deadlines.length)
      timer = setTimeout(
        changed,
        Math.min(Math.min(...deadlines) - now, 2_147_483_647),
      );
  };
  const changed = () => {
    schedule();
    listener();
  };
  const onStorage = (event: StorageEvent) => {
    if (event.key === KEY || event.key === null) changed();
  };
  const onVisible = () => {
    if (!document.hidden) changed();
  };
  window.addEventListener(CHANGE, changed);
  window.addEventListener("storage", onStorage);
  window.addEventListener("focus", changed);
  document.addEventListener("visibilitychange", onVisible);
  schedule();
  return () => {
    clearTimeout(timer);
    window.removeEventListener(CHANGE, changed);
    window.removeEventListener("storage", onStorage);
    window.removeEventListener("focus", changed);
    document.removeEventListener("visibilitychange", onVisible);
  };
}
