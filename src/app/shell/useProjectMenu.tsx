import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  AppWindow,
  Archive,
  BellOff,
  FolderOpen,
  FolderTree,
  ImagePlus,
  Pin,
  PinOff,
  Settings,
  Trash2,
} from "../../shared/ui/icons";
import {
  basename,
  listExternalEditors,
  openInExternalEditor,
  revealPath,
  type ExternalEditor,
} from "../../platform/tauri/fs";
import { IS_MAC, IS_WIN } from "../../platform/tauri/platform";
import { projectKey, projectName } from "../../shared/lib/paths";
import {
  loadPinnedProjects,
  sameProjectPath,
  subscribeProjectPathsChanged,
  toggleProjectPin,
} from "../../features/projects/model/recents";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLabels,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupColorIndex,
  resolveTabGroupCustomColor,
  resolveTabGroupLabel,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
  saveTabGroupColor,
  saveTabGroupCustomColor,
  saveTabGroupLabel,
  saveTabGroupMascot,
} from "../../features/workspace/model/tabGroups";
import {
  createProjectGroup,
  deleteProjectGroup,
  loadProjectGroupAssignments,
  loadProjectGroups,
  projectGroupColor,
  projectGroupIdForPath,
  saveProjectGroups,
  setProjectGroupAssignment,
  updateProjectGroup,
  type ProjectGroup,
} from "../../features/projects/model/projectGroups";
import { useTabGroupLogos } from "../../features/projects/hooks/useTabGroupLogos";
import { ProjectBackgroundDialog } from "../../features/projects/ui/ProjectBackgroundDialog";
import { RemoveProjectDialog } from "../../features/projects/ui/RemoveProjectDialog";
import {
  TabGroupMenu,
  type TabGroupMenuExtraItem,
} from "../../features/workspace/ui/TabGroupMenu";
import {
  knownNotificationProject,
  type NotificationProject,
} from "../../features/notifications/model/notificationProjects";
import { NotificationMuteDatePicker } from "../../features/notifications/ui/NotificationMuteDatePicker";
import { Popover } from "../../shared/ui/Popover";
import {
  notificationMuteActions,
  notificationMuteDeadline,
  notificationMuteStatus,
} from "../../features/notifications/ui/notificationMuteActions";
import { useProjectNotificationPreferences } from "../../features/notifications/hooks/useProjectNotificationPreferences";
import { useNotificationProjects } from "../../features/notifications/hooks/useNotificationProjects";
import { updateNotificationPreferences } from "../../features/notifications/model/notificationPreferences";
import type { ExplorerMenuItem } from "../../features/files/ui/ExplorerMenu";

const REVEAL_LABEL = IS_MAC
  ? "Reveal in Finder"
  : IS_WIN
    ? "Reveal in File Explorer"
    : "Open Containing Folder";

function projectMenuExtraItems(
  pinned: boolean,
  canRemove: boolean,
  canConfigureNotifications: boolean,
  notificationReady: boolean,
  externalEditors: ExternalEditor[] | null,
  projectGroups: ProjectGroup[],
  currentProjectGroupId?: string,
): TabGroupMenuExtraItem[] {
  const groupSubmenu: ExplorerMenuItem[] = [
    { kind: "item", id: "project-group:new", label: "New group…" },
    ...(projectGroups.length > 0 ? [{ kind: "sep" } as const] : []),
    ...projectGroups.map((group) => ({
      kind: "item" as const,
      id: `project-group:${group.id}`,
      label: group.name,
      checked: group.id === currentProjectGroupId,
    })),
    ...(projectGroups.length > 0 ? [{ kind: "sep" } as const] : []),
    {
      kind: "item",
      id: "project-group:none",
      label: "Ungrouped",
      checked: currentProjectGroupId == null,
    },
  ];
  const items: TabGroupMenuExtraItem[] = [
    {
      id: "background",
      label: "Background image",
      icon: ImagePlus,
    },
    {
      id: "project-group",
      label: "Move to group",
      icon: FolderTree,
      submenu: groupSubmenu,
    },
    pinned
      ? { id: "unpin", label: "Unpin project", icon: PinOff }
      : { id: "pin", label: "Pin project", icon: Pin },
    { id: "reveal", label: REVEAL_LABEL, icon: FolderOpen },
    {
      id: "external-editor",
      label: "Open in editor",
      icon: AppWindow,
      disabled: externalEditors === null,
      submenu:
        externalEditors === null
          ? [
              {
                kind: "item",
                id: "external-editor:loading",
                label: "Looking for editors…",
                disabled: true,
              },
            ]
          : externalEditors.length > 0
            ? externalEditors.map((editor) => ({
                kind: "item" as const,
                id: `external-editor:${editor.id}`,
                label: editor.name,
              }))
            : [
                {
                  kind: "item",
                  id: "external-editor:none",
                  label: "No supported editors found",
                  disabled: true,
                },
              ],
    },
    {
      id: "notifications-mute",
      label: "Mute notifications",
      icon: BellOff,
      sepBefore: true,
      disabled: !notificationReady,
      submenu: notificationMuteActions(),
    },
  ];
  if (canConfigureNotifications) {
    items.push({
      id: "notifications-settings",
      label: "Notification settings…",
      icon: Settings,
    });
  }
  if (canRemove) {
    items.push(
      { id: "archive", label: "Archive", icon: Archive, sepBefore: true },
      { id: "delete", label: "Delete", icon: Trash2, danger: true },
    );
  }
  return items;
}

type Point = { x: number; y: number };

type Options = {
  onRemoveProject?: (path: string, options: { purgeData: boolean }) => void;
  onOpenNotificationSettings?: (projectPath?: string) => void;
  /** Called when a project or group menu opens, so hosts can close sibling menus. */
  onOpen?: () => void;
};

/**
 * Right-click menu for a project: appearance, groups, pinning, editors,
 * notifications, and removal. Shared by the project rail and the project
 * pickers shown while the rail is hidden. Every change lands in storage,
 * which announces it through `subscribeProjectPathsChanged`.
 */
export function useProjectMenu({
  onRemoveProject,
  onOpenNotificationSettings,
  onOpen,
}: Options) {
  const [projectMenu, setProjectMenu] = useState<
    (Point & { path: string; projectKey: string }) | null
  >(null);
  const [groupMenu, setGroupMenu] = useState<(Point & { id: string }) | null>(
    null,
  );
  const [notificationMenu, setNotificationMenu] = useState<
    (Point & { path: string; project: NotificationProject }) | null
  >(null);
  const [removing, setRemoving] = useState<{
    path: string;
    name: string;
  } | null>(null);
  const [backgroundProject, setBackgroundProject] = useState<{
    project: string;
    name: string;
  } | null>(null);
  const [menuError, setMenuError] = useState<string | null>(null);
  const [externalEditors, setExternalEditors] = useState<
    ExternalEditor[] | null
  >(null);
  const trigger = useRef<HTMLElement | null>(null);
  // The menus render straight from storage; re-render when it changes.
  const [, setStorageRevision] = useState(0);
  useEffect(
    () =>
      subscribeProjectPathsChanged(() =>
        setStorageRevision((revision) => revision + 1),
      ),
    [],
  );
  const groupLogos = useTabGroupLogos();
  const notificationPreferences = useProjectNotificationPreferences();
  const menuPath = projectMenu?.path;
  useNotificationProjects(menuPath ? [menuPath] : []);
  const readyNotificationProject = menuPath
    ? knownNotificationProject(menuPath)
    : undefined;
  const menuMuteStatus = readyNotificationProject
    ? notificationMuteStatus(
        notificationPreferences[readyNotificationProject.id],
      )
    : null;

  useEffect(() => {
    setMenuError(null);
  }, [menuPath]);

  useEffect(() => {
    let active = true;
    void listExternalEditors()
      .then((installed) => {
        if (active)
          setExternalEditors(Array.isArray(installed) ? installed : []);
      })
      .catch(() => {
        if (active) setExternalEditors([]);
      });
    return () => {
      active = false;
    };
  }, []);

  const captureTrigger = (element?: HTMLElement | null) => {
    trigger.current =
      element ??
      (document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null);
  };

  const restoreFocus = () => trigger.current?.focus();

  const closeNotificationMenu = () => {
    setNotificationMenu(null);
    restoreFocus();
  };

  const open = (
    path: string,
    x: number,
    y: number,
    triggerElement?: HTMLElement | null,
  ) => {
    captureTrigger(triggerElement);
    onOpen?.();
    setGroupMenu(null);
    setNotificationMenu(null);
    setProjectMenu({ x, y, path, projectKey: projectKey(path) });
  };

  const openGroupMenu = (id: string, x: number, y: number) => {
    captureTrigger();
    onOpen?.();
    setProjectMenu(null);
    setNotificationMenu(null);
    setGroupMenu({ x, y, id });
  };

  const createGroup = (x: number, y: number, projectPath?: string) => {
    const current = loadProjectGroups();
    const group = createProjectGroup(current);
    if (!saveProjectGroups([...current, group])) return;
    if (projectPath) setProjectGroupAssignment(projectPath, group.id);
    else captureTrigger();
    setGroupMenu({ x, y, id: group.id });
  };

  const close = () => {
    setProjectMenu(null);
    setGroupMenu(null);
    setNotificationMenu(null);
  };

  const groupLabels = projectMenu ? loadTabGroupLabels() : {};

  const onPick = (action: string) => {
    if (!projectMenu) return;
    const { path, projectKey: key } = projectMenu;
    if (action === "project-group:new") {
      createGroup(projectMenu.x, projectMenu.y, path);
    } else if (action === "project-group:none") {
      setProjectGroupAssignment(path, null);
    } else if (action.startsWith("project-group:")) {
      setProjectGroupAssignment(path, action.slice("project-group:".length));
    } else if (action === "mute:custom") {
      if (!readyNotificationProject) return false;
      setNotificationMenu({
        ...projectMenu,
        project: readyNotificationProject,
      });
    } else if (
      action.startsWith("mute:") ||
      action === "notifications-resume"
    ) {
      if (!readyNotificationProject) return false;
      const mutedUntil = notificationMuteDeadline(action);
      if (action !== "notifications-resume" && mutedUntil === undefined)
        return false;
      try {
        updateNotificationPreferences([readyNotificationProject.id], {
          mutedUntil,
        });
      } catch {
        setMenuError(
          "Could not save notification preferences. Please try again.",
        );
        return false;
      }
    } else if (action.startsWith("external-editor:")) {
      const editorId = action.slice("external-editor:".length);
      if (!externalEditors?.some((editor) => editor.id === editorId))
        return false;
      void openInExternalEditor(editorId, path)
        .then(() => {
          setProjectMenu(null);
          restoreFocus();
        })
        .catch((error: unknown) => {
          setMenuError(error instanceof Error ? error.message : String(error));
        });
      return false;
    } else if (action === "notifications-settings") {
      onOpenNotificationSettings?.(path);
    } else if (action === "pin" || action === "unpin") {
      toggleProjectPin(path);
    } else if (action === "background") {
      setBackgroundProject({
        project: key,
        name: resolveTabGroupLabel(key, groupLabels, basename(path)),
      });
    } else if (action === "reveal") void revealPath(path);
    else if (action === "archive") {
      onRemoveProject?.(path, { purgeData: false });
    } else if (action === "delete") {
      setRemoving({
        path,
        name: resolveTabGroupLabel(key, groupLabels, basename(path)),
      });
    }
  };

  const renderProjectMenu = () => {
    if (!projectMenu) return null;
    const key = projectMenu.projectKey;
    const colors = loadTabGroupColors();
    const customColors = loadTabGroupCustomColors();
    return (
      <TabGroupMenu
        x={projectMenu.x}
        y={projectMenu.y}
        groupId={key}
        label={resolveTabGroupLabel(
          key,
          groupLabels,
          basename(projectMenu.path),
        )}
        colorIndex={resolveTabGroupColorIndex(key, colors, customColors)}
        customColor={resolveTabGroupCustomColor(key, customColors)}
        currentColor={resolveTabGroupColor(
          key,
          colors,
          customColors,
          projectName(projectMenu.path),
        )}
        logoPath={resolveTabGroupLogo(key, groupLogos)}
        logoProject={projectMenu.path}
        mascotName={resolveTabGroupMascot(key, loadTabGroupMascots())}
        mascotProject={projectName(projectMenu.path)}
        onRename={saveTabGroupLabel}
        onColorChange={saveTabGroupColor}
        onCustomColorChange={saveTabGroupCustomColor}
        onMascotChange={saveTabGroupMascot}
        onLogoChange={() => {}}
        onPick={() => {}}
        onClose={() => {
          setProjectMenu(null);
          restoreFocus();
        }}
        showActions={false}
        leadingAction={
          menuMuteStatus
            ? {
                id: "notifications-resume",
                label: "Resume notifications",
                description: menuMuteStatus,
                icon: BellOff,
              }
            : undefined
        }
        extraItems={projectMenuExtraItems(
          loadPinnedProjects().some((pinned) =>
            sameProjectPath(pinned, projectMenu.path),
          ),
          Boolean(onRemoveProject),
          Boolean(onOpenNotificationSettings),
          Boolean(readyNotificationProject),
          externalEditors,
          loadProjectGroups(),
          projectGroupIdForPath(
            projectMenu.path,
            loadProjectGroupAssignments(),
          ),
        )}
        footer={
          menuError ? (
            <p role="alert" className="px-2 py-1 text-xs text-red-400">
              {menuError}
            </p>
          ) : null
        }
        onExtraPick={onPick}
      />
    );
  };

  const renderGroupMenu = () => {
    if (!groupMenu) return null;
    const group = loadProjectGroups().find((item) => item.id === groupMenu.id);
    if (!group) return null;
    return (
      <TabGroupMenu
        x={groupMenu.x}
        y={groupMenu.y}
        groupId={group.id}
        label={group.name}
        colorIndex={group.colorIndex ?? null}
        customColor={group.customColor ?? null}
        currentColor={projectGroupColor(group)}
        logoPath={null}
        mascotName={group.mascot ?? null}
        mascotProject={group.id}
        onRename={(_, name) =>
          updateProjectGroup(group.id, (current) => ({
            ...current,
            name: name.trim() || current.name,
          }))
        }
        onColorChange={(_, colorIndex) =>
          updateProjectGroup(group.id, (current) => ({
            ...current,
            colorIndex: colorIndex ?? undefined,
            customColor: undefined,
          }))
        }
        onCustomColorChange={(_, customColor) =>
          updateProjectGroup(group.id, (current) => ({
            ...current,
            colorIndex: undefined,
            customColor,
          }))
        }
        onMascotChange={(_, mascot) =>
          updateProjectGroup(group.id, (current) => ({
            ...current,
            mascot: mascot ?? undefined,
          }))
        }
        onLogoChange={() => {}}
        onPick={() => {}}
        onClose={() => {
          setGroupMenu(null);
          restoreFocus();
        }}
        showActions={false}
        ariaLabel="Project group actions"
        extraItems={[
          {
            id: "delete-project-group",
            label: "Delete group",
            description: "Projects will become ungrouped",
            icon: Trash2,
            danger: true,
          },
        ]}
        onExtraPick={(action) =>
          action === "delete-project-group"
            ? deleteProjectGroup(group.id)
            : undefined
        }
      />
    );
  };

  const element: ReactNode = (
    <>
      {renderProjectMenu()}
      {renderGroupMenu()}
      {notificationMenu ? (
        <Popover
          key={notificationMenu.path}
          anchor={{ x: notificationMenu.x, y: notificationMenu.y }}
          gap={0}
          width={280}
          role="dialog"
          aria-label="Mute project notifications"
          onDismiss={closeNotificationMenu}
          className="space-y-1 overflow-y-auto p-3"
        >
          <p
            className="truncate px-1 text-xs font-medium text-content/85"
            title={notificationMenu.project.name}
          >
            {notificationMenu.project.name}
          </p>
          <NotificationMuteDatePicker
            projectIds={[notificationMenu.project.id]}
            onCancel={closeNotificationMenu}
            onChanged={closeNotificationMenu}
          />
        </Popover>
      ) : null}
      {removing ? (
        <RemoveProjectDialog
          name={removing.name}
          path={removing.path}
          onConfirm={() => {
            setProjectGroupAssignment(removing.path, null);
            onRemoveProject?.(removing.path, { purgeData: true });
            setRemoving(null);
            restoreFocus();
          }}
          onCancel={() => {
            setRemoving(null);
            restoreFocus();
          }}
        />
      ) : null}
      {backgroundProject ? (
        <ProjectBackgroundDialog
          project={backgroundProject.project}
          name={backgroundProject.name}
          onClose={() => {
            setBackgroundProject(null);
            restoreFocus();
          }}
        />
      ) : null}
    </>
  );

  return {
    open,
    openGroupMenu,
    createGroup,
    close,
    isOpen: projectMenu != null,
    /** True while any menu or dialog opened from the project menu is showing. */
    isActive:
      projectMenu != null ||
      groupMenu != null ||
      notificationMenu != null ||
      removing != null ||
      backgroundProject != null,
    element,
  };
}
