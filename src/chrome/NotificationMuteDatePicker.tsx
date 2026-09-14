import { useState } from "react";
import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";
import {
  DateTimePicker,
  parseLocalDateTime,
  toLocalDateTime,
} from "./DateTimePicker";

type Props = {
  projectIds: readonly string[];
  onChanged?: () => void;
  onCancel: () => void;
};

/** Custom timing is a complete step, separate from the duration presets. */
export function NotificationMuteDatePicker({
  projectIds,
  onChanged,
  onCancel,
}: Props) {
  const [value, setValue] = useState(() => {
    const until =
      projectIds.length === 1
        ? loadNotificationPreferences()[projectIds[0]]?.mutedUntil
        : undefined;
    const initial =
      typeof until === "number" && until > Date.now()
        ? until
        : Date.now() + 3_600_000;
    return toLocalDateTime(new Date(Math.ceil(initial / 60_000) * 60_000));
  });
  const [error, setError] = useState<string | null>(null);
  return (
    <form
      noValidate
      className="w-full max-w-xs space-y-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (!projectIds.length) return;
        const date = parseLocalDateTime(value);
        if (!date) {
          setError("Choose a valid date and time.");
          return;
        }
        if (date.getTime() <= Date.now()) {
          setError("Choose a date and time in the future.");
          return;
        }
        try {
          updateNotificationPreferences(projectIds, {
            mutedUntil: date.getTime(),
          });
          setError(null);
          onChanged?.();
        } catch {
          setError(
            "Could not save notification preferences. Please try again.",
          );
        }
      }}
    >
      <p className="text-xs text-content/60">Resume notifications on</p>
      <DateTimePicker
        value={value}
        onChange={(next) => {
          setValue(next);
          setError(null);
        }}
        minDate={toLocalDateTime(new Date(Date.now())).slice(0, 10)}
        autoFocus
      />
      {error ? (
        <p role="alert" className="text-xs text-red-400">
          {error}
        </p>
      ) : null}
      <div className="flex items-center justify-between gap-2 border-t border-content/10 pt-3">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md px-3 py-1.5 text-xs text-content/60 hover:bg-content/5 hover:text-content focus-visible:outline-2 focus-visible:outline-accent"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={!projectIds.length}
          className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:opacity-90 focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40"
        >
          Mute until then
        </button>
      </div>
    </form>
  );
}
