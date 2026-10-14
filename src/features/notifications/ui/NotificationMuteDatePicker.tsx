import { useState } from "react";
import { useT } from "../../../shared/i18n/useT";
import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../model/notificationPreferences";
import {
  DateTimePicker,
  parseLocalDateTime,
  toLocalDateTime,
} from "../../../shared/ui/DateTimePicker";

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
  const t = useT();
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
      className="w-full"
      onSubmit={(event) => {
        event.preventDefault();
        if (!projectIds.length) return;
        const date = parseLocalDateTime(value);
        if (!date) {
          setError(t("settings.notifications.invalidDate"));
          return;
        }
        if (date.getTime() <= Date.now()) {
          setError(t("settings.notifications.pastDate"));
          return;
        }
        try {
          updateNotificationPreferences(projectIds, {
            mutedUntil: date.getTime(),
          });
          setError(null);
          onChanged?.();
        } catch {
          setError(t("settings.notifications.saveFailed"));
        }
      }}
    >
      <p className="mb-3 px-1 text-[11px] text-content/45">
        {t("settings.notifications.muteAllUntil")}
      </p>
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
        <p role="alert" className="mt-3 px-1 text-xs text-red-400">
          {error}
        </p>
      ) : null}
      <div className="mt-3 flex items-center justify-between gap-2 border-t border-stroke pt-2.5">
        <button
          type="button"
          onClick={onCancel}
          className="rounded px-2 py-1.5 text-xs text-content/50 hover:bg-content/5 hover:text-content focus-visible:outline-2 focus-visible:outline-accent"
        >
          {t("settings.notifications.cancel")}
        </button>
        <button
          type="submit"
          disabled={!projectIds.length}
          className="primary-action flex shrink-0 items-center rounded-md border border-transparent px-2.5 py-1 text-[12px] focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-default"
        >
          {t("settings.notifications.muteUntilThen")}
        </button>
      </div>
    </form>
  );
}
