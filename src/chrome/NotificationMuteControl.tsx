import { useEffect, useRef, useState } from "react";
import { useProjectNotificationPreferences } from "../hooks/useProjectNotificationPreferences";
import { notificationMuteStatus } from "./notificationMuteActions";
import { NotificationMuteDatePicker } from "./NotificationMuteDatePicker";
import {
  isProjectMuted,
  NOTIFICATION_MUTE_HOURS,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";

type Props = {
  projectIds: readonly string[];
  onChanged?: () => void;
};

export function NotificationMuteControl({ projectIds, onChanged }: Props) {
  const [error, setError] = useState<string | null>(null);
  const [customOpen, setCustomOpen] = useState(false);
  const customTrigger = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  useEffect(() => {
    if (!customOpen && restoreFocus.current) {
      customTrigger.current?.focus();
      restoreFocus.current = false;
    }
  }, [customOpen]);
  const preferences = useProjectNotificationPreferences();
  const muted = projectIds.filter((id) =>
    isProjectMuted(preferences[id] ?? { disabled: [] }),
  );
  const status =
    muted.length === 0
      ? "Choose how long to mute notifications"
      : projectIds.length > 1
        ? `${muted.length} of ${projectIds.length} projects muted`
        : notificationMuteStatus(preferences[projectIds[0]]);
  const change = (mutedUntil: number | null | undefined) => {
    if (!projectIds.length) return;
    try {
      updateNotificationPreferences(projectIds, { mutedUntil });
      setError(null);
      setCustomOpen(false);
      onChanged?.();
    } catch {
      setError("Could not save notification preferences. Please try again.");
    }
  };
  const closeCustom = () => {
    restoreFocus.current = true;
    setCustomOpen(false);
  };
  if (customOpen)
    return (
      <NotificationMuteDatePicker
        projectIds={projectIds}
        onCancel={closeCustom}
        onChanged={() => {
          closeCustom();
          onChanged?.();
        }}
      />
    );
  return (
    <div className="space-y-2">
      <p role="status" className="text-xs text-content/50">
        {status}
      </p>
      <div className="flex flex-wrap gap-2">
        {NOTIFICATION_MUTE_HOURS.map((hours) => (
          <button
            key={hours}
            type="button"
            disabled={!projectIds.length}
            className="rounded-md border border-content/10 px-3 py-1.5 text-left text-xs text-content/70 hover:bg-content/10 hover:text-content focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40"
            onClick={() => change(Date.now() + hours * 60 * 60 * 1000)}
          >
            {hours} {hours === 1 ? "hour" : "hours"}
          </button>
        ))}
        <button
          type="button"
          disabled={!projectIds.length}
          className="rounded-md border border-content/10 px-3 py-1.5 text-left text-xs text-content/70 hover:bg-content/10 hover:text-content focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40"
          onClick={() => change(null)}
        >
          Until resumed
        </button>
        <button
          type="button"
          disabled={!projectIds.length}
          ref={customTrigger}
          aria-expanded={customOpen}
          className="rounded-md border border-content/10 px-3 py-1.5 text-left text-xs text-content/70 hover:bg-content/10 hover:text-content focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40"
          onClick={() => setCustomOpen((open) => !open)}
        >
          Choose date and time
        </button>
        {muted.length ? (
          <button
            type="button"
            className="rounded-md border border-content/10 px-3 py-1.5 text-left text-xs text-content/70 hover:bg-content/10 hover:text-content focus-visible:outline-2 focus-visible:outline-accent"
            onClick={() => change(undefined)}
          >
            Resume notifications
          </button>
        ) : null}
      </div>
      {error ? (
        <p role="alert" className="text-xs text-red-400">
          {error}
        </p>
      ) : null}
    </div>
  );
}
