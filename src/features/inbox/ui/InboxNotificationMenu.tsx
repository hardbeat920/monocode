import { useEffect, useState } from "react";
import { translate } from "../../../shared/i18n/locale";
import { useLanguage } from "../../../shared/i18n/useTranslation";
import {
  inboxHasUnseenItems,
  knownInboxEntries,
  markInboxItemsSeen,
  useInboxSeenTick,
} from "../model/inboxSeen";
import { useProjectNotificationPreferences } from "../../notifications/hooks/useProjectNotificationPreferences";
import {
  isProjectMuted,
  updateNotificationPreferences,
} from "../../notifications/model/notificationPreferences";
import { useNotificationProjects } from "../../notifications/hooks/useNotificationProjects";
import { ExplorerMenu, type ExplorerMenuItem } from "../../files/ui/ExplorerMenu";
import { NotificationMuteDatePicker } from "../../notifications/ui/NotificationMuteDatePicker";
import { Popover } from "../../../shared/ui/Popover";
import {
  notificationMuteActions,
  notificationMuteDeadline,
} from "../../notifications/ui/notificationMuteActions";

type Props = {
  x: number;
  y: number;
  projectPaths: readonly string[];
  onOpenSettings?: () => void;
  onClose: () => void;
};
export function InboxNotificationMenu({
  x,
  y,
  projectPaths,
  onOpenSettings,
  onClose,
}: Props) {
  useLanguage();
  const notificationProjects = useNotificationProjects(projectPaths);
  const [saveError, setError] = useState<string | null>(null);
  const [customOpen, setCustomOpen] = useState(false);
  useInboxSeenTick();
  const preferences = useProjectNotificationPreferences();
  const pathsKey = JSON.stringify(projectPaths);
  useEffect(() => {
    setCustomOpen(false);
    setError(null);
  }, [pathsKey]);
  const entries = knownInboxEntries(projectPaths);
  const hasUnread = inboxHasUnseenItems(entries);

  const allIds = notificationProjects.projects.map((project) => project.id);
  const mutedIds = allIds.filter((id) =>
    isProjectMuted(preferences[id] ?? { disabled: [] }),
  );
  const items: ExplorerMenuItem[] = [
    {
      kind: "item",
      id: "read-all",
      label: translate("Mark all as read"),
      disabled: !hasUnread,
    },
    { kind: "sep" },
    {
      kind: "item",
      id: "mute",
      label: translate("Mute all projects"),
      disabled: !allIds.length,
      submenu: notificationMuteActions(),
    },
    {
      kind: "item",
      id: "resume",
      label: translate("Resume muted projects"),
      disabled: !mutedIds.length,
    },
  ];
  if (onOpenSettings)
    items.push(
      { kind: "item", id: "settings", label: translate("Notification settings…") },
    );

  if (customOpen)
    return (
      <Popover
        anchor={{ x, y }}
        gap={0}
        width={280}
        role="dialog"
        aria-label={translate("Mute project notifications")}
        onDismiss={onClose}
        className="space-y-1 overflow-y-auto p-3"
      >
        <div className="space-y-1">
          <p className="px-1 text-xs font-medium text-content/85">
            {translate("Mute all projects")}
          </p>
        </div>
        <NotificationMuteDatePicker
          projectIds={allIds}
          onChanged={onClose}
          onCancel={() => setCustomOpen(false)}
        />
      </Popover>
    );

  return (
    <ExplorerMenu
      x={x}
      y={y}
      ariaLabel={translate("Inbox actions")}
      width={272}
      items={items}
      onClose={onClose}
      header={
        <div className="space-y-1 px-2 py-1.5">
          <p className="text-xs font-medium text-content">
            {translate("Inbox")}
          </p>
          <p role="status" className="text-xs text-content/50">
            {allIds.length === 1
              ? translate("{count} project · {muted} muted", {
                  count: allIds.length,
                  muted: mutedIds.length,
                })
              : translate("{count} projects · {muted} muted", {
                  count: allIds.length,
                  muted: mutedIds.length,
                })}
          </p>
          {saveError ? (
            <p role="alert" className="text-xs text-red-400">
              {saveError}
            </p>
          ) : null}
        </div>
      }
      onPick={(id) => {
        if (id === "read-all") {
          if (!hasUnread) return;
          if (!markInboxItemsSeen(entries)) {
            setError(translate("Could not save read status. Please try again."));
            return;
          }
          onClose();
          return;
        }
        if (id === "settings") {
          onClose();
          onOpenSettings?.();
          return;
        }
        const ids = id === "resume" ? mutedIds : allIds;
        if (!ids.length) return;
        if (id === "mute:custom") {
          setCustomOpen(true);
          return;
        }
        const mutedUntil = notificationMuteDeadline(id);
        if (id !== "resume" && mutedUntil === undefined) return;
        try {
          updateNotificationPreferences(ids, {
            mutedUntil,
          });
          onClose();
        } catch {
          setError(
            translate("Could not save notification preferences. Please try again."),
          );
        }
      }}
    />
  );
}
