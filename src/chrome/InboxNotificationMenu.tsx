import { useEffect, useState } from "react";
import { useProjectNotificationPreferences } from "../hooks/useProjectNotificationPreferences";
import {
  isProjectMuted,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";
import { useNotificationProjects } from "../hooks/useNotificationProjects";
import { ExplorerMenu, type ExplorerMenuItem } from "./ExplorerMenu";
import { NotificationMuteDatePicker } from "./NotificationMuteDatePicker";
import { Popover } from "./Popover";
import {
  notificationMuteActions,
  notificationMuteDeadline,
} from "./notificationMuteActions";

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
  const discovery = useNotificationProjects(projectPaths);
  const selection = discovery.selection;
  const [saveError, setError] = useState<string | null>(null);
  const error = saveError ?? discovery.error;
  const [customOpen, setCustomOpen] = useState(false);
  const preferences = useProjectNotificationPreferences();
  const pathsKey = JSON.stringify(projectPaths);
  useEffect(() => {
    setCustomOpen(false);
    setError(null);
  }, [pathsKey]);

  const allIds = selection?.projects.map((project) => project.id) ?? [];
  const mutedIds = allIds.filter((id) =>
    isProjectMuted(preferences[id] ?? { disabled: [] }),
  );
  const items: ExplorerMenuItem[] = [
    {
      kind: "item",
      id: "mute",
      label: "Mute all projects",
      disabled: !allIds.length,
      submenu: notificationMuteActions(),
    },
    { kind: "sep" },
    {
      kind: "item",
      id: "resume",
      label: "Resume muted projects",
      disabled: !mutedIds.length,
    },
  ];
  if (!selection && error)
    items.push({ kind: "item", id: "retry", label: "Retry loading projects" });
  if (onOpenSettings)
    items.push(
      { kind: "sep" },
      { kind: "item", id: "settings", label: "Notification settings…" },
    );

  if (customOpen)
    return (
      <Popover
        anchor={{ x, y }}
        gap={0}
        width={320}
        role="dialog"
        aria-label="Mute project notifications"
        onDismiss={onClose}
        className="space-y-3 overflow-y-auto p-3"
      >
        <div className="space-y-1">
          <p className="text-sm font-medium text-content">Mute all projects</p>
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
      ariaLabel="Inbox actions"
      width={272}
      items={items}
      onClose={onClose}
      header={
        <div className="space-y-1 px-2 py-1.5">
          <p className="text-xs font-medium text-content">
            Project notifications
          </p>
          <p role="status" className="text-xs text-content/50">
            {selection
              ? `${allIds.length} ${allIds.length === 1 ? "project" : "projects"} · ${mutedIds.length} muted`
              : error
                ? "Projects unavailable"
                : "Loading projects…"}
          </p>
          {error ? (
            <p role="alert" className="text-xs text-red-400">
              {error}
            </p>
          ) : null}
        </div>
      }
      onPick={(id) => {
        if (id === "retry") {
          setError(null);
          discovery.retry();
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
            "Could not save notification preferences. Please try again.",
          );
        }
      }}
    />
  );
}
