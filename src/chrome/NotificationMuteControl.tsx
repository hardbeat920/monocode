import { useRef, useState } from "react";
import { useProjectNotificationPreferences } from "../hooks/useProjectNotificationPreferences";
import {
  notificationMuteActions,
  notificationMuteDeadline,
  notificationMuteStatus,
} from "./notificationMuteActions";
import { NotificationMuteDatePicker } from "./NotificationMuteDatePicker";
import { ExplorerMenu } from "./ExplorerMenu";
import { Popover } from "./Popover";
import { SecondaryButton } from "./SecondaryButton";
import { BellOff, ChevronDown } from "./icons";
import {
  isProjectMuted,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";

type Props = {
  projectIds: readonly string[];
  onChanged?: () => void;
};

export function NotificationMuteControl({ projectIds, onChanged }: Props) {
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<"menu" | "custom" | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const preferences = useProjectNotificationPreferences();
  const muted = projectIds.filter((id) =>
    isProjectMuted(preferences[id] ?? { disabled: [] }),
  );
  const status =
    muted.length === 0
      ? null
      : projectIds.length > 1
        ? `${muted.length} of ${projectIds.length} projects muted`
        : notificationMuteStatus(preferences[projectIds[0]]);

  const close = (restoreFocus = true) => {
    setOpen(null);
    if (restoreFocus) trigger.current?.focus();
  };
  const change = (mutedUntil: number | null | undefined) => {
    if (!projectIds.length) return;
    try {
      updateNotificationPreferences(projectIds, { mutedUntil });
      setError(null);
      close();
      onChanged?.();
    } catch {
      setError("Could not save notification preferences. Please try again.");
    }
  };

  return (
    <div className="flex max-w-full flex-wrap items-center justify-end gap-x-3 gap-y-2">
      {status ? (
        <span role="status" className="text-[11px] text-content/50">
          {status}
        </span>
      ) : null}
      {muted.length ? (
        <button
          type="button"
          className="rounded-md px-2 py-1.5 text-xs text-content/70 hover:bg-content/5 hover:text-content focus-visible:outline-2 focus-visible:outline-accent"
          onClick={() => change(undefined)}
        >
          Resume notifications
        </button>
      ) : null}
      <SecondaryButton
        ref={trigger}
        type="button"
        aria-label={
          muted.length ? "Change mute duration" : "Mute notifications"
        }
        aria-haspopup={open === "custom" ? "dialog" : "menu"}
        aria-expanded={open !== null}
        title="Mute pauses all project notifications without changing your category choices."
        disabled={!projectIds.length}
        onClick={() => setOpen(open ? null : "menu")}
      >
        <BellOff className="size-3.5" aria-hidden="true" />
        {muted.length ? "Muted" : "Mute"}
        <ChevronDown className="size-3 text-content/40" aria-hidden="true" />
      </SecondaryButton>
      {error ? (
        <p role="alert" className="w-full text-xs text-red-400">
          {error}
        </p>
      ) : null}
      {open === "menu" && trigger.current ? (
        <ExplorerMenu
          x={trigger.current.getBoundingClientRect().right - 244}
          y={trigger.current.getBoundingClientRect().bottom + 4}
          ariaLabel="Mute notifications"
          width={244}
          header={
            <p className="px-2 py-1.5 text-[11px] text-content/45">
              Mute all notifications for
            </p>
          }
          items={notificationMuteActions()}
          onClose={() =>
            close(Boolean(document.activeElement?.closest('[role="menu"]')))
          }
          onPick={(id) => {
            if (id === "mute:custom") {
              setOpen("custom");
              return;
            }
            const deadline = notificationMuteDeadline(id);
            if (deadline !== undefined) change(deadline);
          }}
        />
      ) : null}
      {open === "custom" ? (
        <Popover
          anchor={trigger}
          align="end"
          width={280}
          role="dialog"
          aria-label="Mute project notifications"
          onDismiss={(reason) => close(reason === "escape")}
          className="overflow-y-auto p-3"
        >
          <NotificationMuteDatePicker
            projectIds={projectIds}
            onCancel={() => close()}
            onChanged={() => {
              close();
              onChanged?.();
            }}
          />
        </Popover>
      ) : null}
    </div>
  );
}
