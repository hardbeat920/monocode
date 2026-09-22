import {
  AppWindow,
  Archive,
  BellOff,
  ChevronDown,
  ChevronRight,
  FolderOpen,
  FolderPlus,
  FolderTree,
  ImagePlus,
  Inbox,
  MoreHorizontal,
  Pin,
  PinOff,
  File,
  Plus,
  Search,
  Settings,
  Trash2,
  Zap,
} from "../../shared/ui/icons";
import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { useDragResize } from "../../shared/hooks/useDragResize";
import { useLockOverscroll } from "../../shared/hooks/useLockOverscroll";
import { useProjectDiffStats } from "../../features/source-control/hooks/useProjectDiffStats";
import { useAnimatedReorder } from "../../shared/hooks/useAnimatedReorder";
import { useTabGroupLogos } from "../../features/projects/hooks/useTabGroupLogos";
import {
  loadProjectRailWidth,
  PROJECT_RAIL_WIDTH_DEFAULT,
  PROJECT_RAIL_WIDTH_MAX,
  PROJECT_RAIL_WIDTH_MIN,
  saveProjectRailWidth,
} from "../../features/settings/model/appearance";
import {
  basename,
  listExternalEditors,
  openInExternalEditor,
  revealPath,
  type ExternalEditor,
  type GitDiffStats,
} from "../../platform/tauri/fs";
import { IS_MAC, IS_WIN, MOD } from "../../platform/tauri/platform";
import { formatInteger } from "../../shared/lib/numbers";
import { pathKey, projectKey, projectName } from "../../shared/lib/paths";
import {
  collectRailProjects,
  loadPinnedProjects,
  loadProjectRailOrder,
  projectRailSections,
  sameProjectPath,
  savePinnedProjects,
  saveProjectRailOrder,
  subscribeProjectPathsChanged,
  syncProjectRailOrder,
  type RecentProject,
} from "../../features/projects/model/recents";
import {
  TAB_GROUP_COLORS,
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
  tabGroupColor,
} from "../../features/workspace/model/tabGroups";
import {
  createProjectGroup,
  loadProjectGroupAssignments,
  loadProjectGroups,
  projectGroupIdForPath,
  saveProjectGroupAssignments,
  saveProjectGroups,
  type ProjectGroup,
} from "../../features/projects/model/projectGroups";
import type { LiveAgent } from "../../features/sessions/model/liveAgents";
import { LiveAgentsPreview } from "../../features/sessions/ui/LiveAgentsPreview";
import { ProjectLogoIcon } from "../../features/projects/ui/ProjectLogoIcon";
import { ProjectBackgroundDialog } from "../../features/projects/ui/ProjectBackgroundDialog";
import { ProjectMascot } from "../../features/projects/ui/ProjectMascot";
import { RailAction, RailSearch } from "./RailAction";
import { RemoveProjectDialog } from "../../features/projects/ui/RemoveProjectDialog";
import { DevModeSlot, TabVisitNav } from "./TitleBar";
import { SidebarUpdateFooter } from "./SidebarUpdate";
import type { InstalledUpdate } from "../model/updateNotice";
import { SettingsNav } from "./SettingsRail";
import { Shimmer } from "../../shared/ui/Shimmer";
import { TabGroupMenu, type TabGroupMenuExtraItem } from "../../features/workspace/ui/TabGroupMenu";
import type { SettingsSectionId } from "../../features/settings/model/settings";
import {
  knownNotificationProject,
  type NotificationProject,
} from "../../features/notifications/model/notificationProjects";
import { NotificationMuteDatePicker } from "../../features/notifications/ui/NotificationMuteDatePicker";
import { Popover } from "../../shared/ui/Popover";
import { InboxNotificationMenu } from "../../features/inbox/ui/InboxNotificationMenu";
import { notificationMuteActions, notificationMuteDeadline, notificationMuteStatus } from "../../features/notifications/ui/notificationMuteActions";
import { useProjectNotificationPreferences } from "../../features/notifications/hooks/useProjectNotificationPreferences";
import { useNotificationProjects } from "../../features/notifications/hooks/useNotificationProjects";
import { updateNotificationPreferences } from "../../features/notifications/model/notificationPreferences";
import type { ExplorerMenuItem } from "../../features/files/ui/ExplorerMenu";
import { GithubStarPrompt } from "./GithubStarPrompt";

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

type Props = {
  cwd: string;
  recents: RecentProject[];
  inboxUnseen?: boolean;
  busyPaths?: Iterable<string>;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onGoBack?: () => void;
  onGoForward?: () => void;
  onSearch?: () => void;
  searchActive?: boolean;
  onOpenInbox?: () => void;
  inboxActive?: boolean;
  notesEnabled?: boolean;
  onOpenNotes?: () => void;
  notesActive?: boolean;
  onOpenAutomations?: () => void;
  automationsActive?: boolean;
  onTogglePanel?: () => void;
  onSelectProject: (path: string) => void;
  onOpenProject: () => void;
  onRemoveProject?: (path: string, options: { purgeData: boolean }) => void;
  liveAgents?: LiveAgent[];
  activeSessionId?: string;
  onSelectAgent?: (sessionId: string) => void;
  settingsOpen?: boolean;
  settingsSection?: SettingsSectionId;
  onOpenSettings?: () => void;
  onOpenNotificationSettings?: (projectPath?: string) => void;
  onSelectSettingsSection?: (section: SettingsSectionId) => void;
  onCloseSettings?: () => void;
  updateNotice?: InstalledUpdate | null;
  onOpenWhatsNew?: (version: string) => void;
  onDismissUpdate?: () => void;
};

export function ProjectRail({
  cwd,
  recents,
  inboxUnseen = false,
  busyPaths,
  canGoBack = false,
  canGoForward = false,
  onGoBack,
  onGoForward,
  onSearch,
  searchActive = false,
  onOpenInbox,
  inboxActive = false,
  notesEnabled = true,
  onOpenNotes,
  notesActive = false,
  onOpenAutomations,
  automationsActive = false,
  onTogglePanel,
  onSelectProject,
  onOpenProject,
  onRemoveProject,
  liveAgents = [],
  activeSessionId,
  onSelectAgent,
  settingsOpen = false,
  settingsSection = "general",
  onOpenSettings,
  onOpenNotificationSettings,
  onSelectSettingsSection,
  onCloseSettings,
  updateNotice = null,
  onOpenWhatsNew,
  onDismissUpdate,
}: Props) {
  const resize = useDragResize({
    min: PROJECT_RAIL_WIDTH_MIN,
    max: () =>
      Math.min(PROJECT_RAIL_WIDTH_MAX, Math.floor(window.innerWidth * 0.35)),
    defaultWidth: PROJECT_RAIL_WIDTH_DEFAULT,
    initial: loadProjectRailWidth(),
    onCommit: saveProjectRailWidth,
  });
  const [railOrder, setRailOrder] = useState(loadProjectRailOrder);
  const [pinnedPaths, setPinnedPaths] = useState(loadPinnedProjects);
  const [groupLabels, setGroupLabels] = useState(loadTabGroupLabels);
  const [groupColors, setGroupColors] = useState(loadTabGroupColors);
  const [groupMascots, setGroupMascots] = useState(loadTabGroupMascots);
  const [groupCustomColors, setGroupCustomColors] = useState(
    loadTabGroupCustomColors,
  );
  const [projectGroups, setProjectGroups] = useState(loadProjectGroups);
  const [projectGroupAssignments, setProjectGroupAssignments] = useState(
    loadProjectGroupAssignments,
  );
  useEffect(
    () =>
      subscribeProjectPathsChanged(() => {
        setRailOrder(loadProjectRailOrder());
        setPinnedPaths(loadPinnedProjects());
        setGroupLabels(loadTabGroupLabels());
        setGroupColors(loadTabGroupColors());
        setGroupMascots(loadTabGroupMascots());
        setGroupCustomColors(loadTabGroupCustomColors());
        setProjectGroupAssignments(loadProjectGroupAssignments());
      }),
    [],
  );
  const [externalEditors, setExternalEditors] = useState<
    ExternalEditor[] | null
  >(null);
  const [projectMenu, setProjectMenu] = useState<{
    x: number;
    y: number;
    path: string;
    projectKey: string;
  } | null>(null);
  const [projectGroupMenu, setProjectGroupMenu] = useState<{
    x: number;
    y: number;
    id: string;
  } | null>(null);
  const [notificationMenu, setNotificationMenu] = useState<{
    x: number;
    y: number;
    path: string;
    project: NotificationProject;
  } | null>(null);
  const [projectMenuError, setProjectMenuError] = useState<string | null>(null);
  const notificationPreferences = useProjectNotificationPreferences();
  const allProjects = useMemo(
    () => collectRailProjects(recents, cwd),
    [cwd, recents],
  );
  const notificationProjects = useNotificationProjects([...allProjects.keys()]);
  const notificationPath = projectMenu?.path;
  const readyNotificationProject = notificationPath
    ? knownNotificationProject(notificationPath)
    : undefined;
  const menuMuteStatus = readyNotificationProject
    ? notificationMuteStatus(notificationPreferences[readyNotificationProject.id])
    : null;
  useEffect(() => {
    setProjectMenuError(null);
  }, [notificationPath]);
  useEffect(() => {
    let active = true;
    void listExternalEditors()
      .then((installed) => {
        if (active) setExternalEditors(Array.isArray(installed) ? installed : []);
      })
      .catch(() => {
        if (active) setExternalEditors([]);
      });
    return () => {
      active = false;
    };
  }, []);
  const [inboxMenu, setInboxMenu] = useState<{
    x: number;
    y: number;
  } | null>(null);
  const menuTrigger = useRef<HTMLElement | null>(null);
  const [removing, setRemoving] = useState<{
    path: string;
    name: string;
  } | null>(null);
  const [backgroundProject, setBackgroundProject] = useState<{
    project: string;
    name: string;
  } | null>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const scrollRef = useRef<HTMLDivElement>(null);
  const groupLogos = useTabGroupLogos();
  const muteStatuses = new Map<string, string | null>();
  for (const project of notificationProjects.projects) {
    const status = notificationMuteStatus(notificationPreferences[project.id]);
    for (const path of project.paths) muteStatuses.set(pathKey(path), status);
  }
  const sections = useMemo(
    () => projectRailSections(recents, cwd, railOrder, pinnedPaths),
    [cwd, pinnedPaths, railOrder, recents],
  );
  const groupedProjectSections = useMemo(() => {
    const byGroup = new Map<string, RecentProject[]>(
      projectGroups.map((group) => [group.id, []]),
    );
    const ungrouped: RecentProject[] = [];
    for (const project of sections.projects) {
      const groupId = projectGroupIdForPath(
        project.path,
        projectGroupAssignments,
      );
      const items = groupId ? byGroup.get(groupId) : undefined;
      if (items) items.push(project);
      else ungrouped.push(project);
    }
    return {
      ungrouped,
      grouped: projectGroups.map((group) => ({
        group,
        items: byGroup.get(group.id) ?? [],
      })),
    };
  }, [projectGroupAssignments, projectGroups, sections.projects]);
  const busy = useMemo(() => {
    const set = new Set<string>();
    for (const path of busyPaths ?? []) set.add(path);
    return set;
  }, [busyPaths]);

  useEffect(() => {
    setRailOrder((prev) => {
      const synced = syncProjectRailOrder(prev, allProjects);
      if (synced.join("\0") === prev.join("\0")) return prev;
      saveProjectRailOrder(synced);
      return synced;
    });
  }, [allProjects]);

  useEffect(() => {
    setPinnedPaths((prev) => {
      const next = prev.filter((path) => allProjects.has(path));
      if (next.length === prev.length) return prev;
      savePinnedProjects(next);
      return next;
    });
  }, [allProjects]);

  useEffect(() => {
    if (!projectMenu) return;
    const onScroll = () => setProjectMenu(null);
    const scrollParent = scrollRef.current ?? window;
    scrollParent.addEventListener("scroll", onScroll, true);
    return () => scrollParent.removeEventListener("scroll", onScroll, true);
  }, [projectMenu]);

  const openProjectMenu = (path: string, x: number, y: number) => {
    menuTrigger.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setInboxMenu(null);
    setNotificationMenu(null);
    setProjectMenu({
      x,
      y,
      path,
      projectKey: projectKey(path),
    });
  };

  const onProjectContextMenu = (
    path: string,
    event: MouseEvent<HTMLElement>,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.querySelector<HTMLButtonElement>("button")?.focus();
    openProjectMenu(path, event.clientX, event.clientY);
  };

  const onProjectRename = (projectKey: string, label: string) => {
    saveTabGroupLabel(projectKey, label);
    setGroupLabels(loadTabGroupLabels());
  };

  const onProjectColorChange = (
    projectKey: string,
    colorIndex: number | null,
  ) => {
    saveTabGroupColor(projectKey, colorIndex);
    setGroupColors(loadTabGroupColors());
    setGroupCustomColors(loadTabGroupCustomColors());
  };

  const onProjectMascotChange = (projectKey: string, name: string | null) => {
    saveTabGroupMascot(projectKey, name);
    setGroupMascots(loadTabGroupMascots());
  };

  const onProjectCustomColorChange = (projectKey: string, color: string) => {
    saveTabGroupCustomColor(projectKey, color);
    setGroupColors(loadTabGroupColors());
    setGroupCustomColors(loadTabGroupCustomColors());
  };

  const saveProjectGroupList = (next: ProjectGroup[]) => {
    if (!saveProjectGroups(next)) return false;
    setProjectGroups(next);
    return true;
  };

  const updateProjectGroup = (
    id: string,
    update: (group: ProjectGroup) => ProjectGroup,
  ) => {
    const current = loadProjectGroups();
    if (!current.some((group) => group.id === id)) return;
    const next = current.map((group) =>
      group.id === id ? update(group) : group,
    );
    saveProjectGroupList(next);
  };

  const assignProjectGroup = (path: string, groupId: string | null) => {
    const next = { ...loadProjectGroupAssignments() };
    const key = pathKey(path);
    if (groupId == null) delete next[key];
    else next[key] = groupId;
    if (!saveProjectGroupAssignments(next)) return false;
    setProjectGroupAssignments(next);
    return true;
  };

  const createGroup = (x: number, y: number, projectPath?: string) => {
    const current = loadProjectGroups();
    const group = createProjectGroup(current);
    if (!saveProjectGroupList([...current, group])) return;
    if (projectPath) assignProjectGroup(projectPath, group.id);
    if (!projectPath) {
      menuTrigger.current =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;
    }
    setProjectGroupMenu({ x, y, id: group.id });
  };

  const deleteGroup = (id: string) => {
    const nextGroups = loadProjectGroups().filter((group) => group.id !== id);
    if (!saveProjectGroupList(nextGroups)) return false;
    const nextAssignments = loadProjectGroupAssignments(nextGroups);
    saveProjectGroupAssignments(nextAssignments);
    setProjectGroupAssignments(nextAssignments);
    return true;
  };

  const reorderSubset = (
    fullOrder: string[],
    subsetOrder: string[],
    subsetPaths: Set<string>,
  ) => {
    const next: string[] = [];
    let subsetIndex = 0;
    for (const path of fullOrder) {
      if (!subsetPaths.has(path)) {
        next.push(path);
        continue;
      }
      if (subsetIndex < subsetOrder.length) {
        next.push(subsetOrder[subsetIndex++]);
      }
    }
    return next;
  };

  const onReorderPinned = (ids: string[]) => {
    const subset = new Set(sections.pinned.map((item) => item.path));
    const next = reorderSubset(railOrder, ids, subset);
    setRailOrder(next);
    saveProjectRailOrder(next);
  };

  const onReorderProjects = (ids: string[]) => {
    const subset = new Set(ids);
    const next = reorderSubset(railOrder, ids, subset);
    setRailOrder(next);
    saveProjectRailOrder(next);
  };

  const onTogglePin = (path: string) => {
    const isPinned = pinnedPaths.some((pinned) =>
      sameProjectPath(pinned, path),
    );
    const next = isPinned
      ? pinnedPaths.filter((pinned) => !sameProjectPath(pinned, path))
      : [...pinnedPaths, path];
    setPinnedPaths(next);
    savePinnedProjects(next);
  };

  const onProjectMenuPick = (action: string) => {
    if (!projectMenu) return;
    const { path, projectKey } = projectMenu;
    if (action === "project-group:new") {
      createGroup(projectMenu.x, projectMenu.y, path);
    } else if (action === "project-group:none") {
      assignProjectGroup(path, null);
    } else if (action.startsWith("project-group:")) {
      const groupId = action.slice("project-group:".length);
      if (projectGroups.some((group) => group.id === groupId)) {
        assignProjectGroup(path, groupId);
      }
    }
    else if (action === "mute:custom") {
      if (!readyNotificationProject) return false;
      setNotificationMenu({ ...projectMenu, project: readyNotificationProject });
    }
    else if (action.startsWith("mute:") || action === "notifications-resume") {
      if (!readyNotificationProject) return false;
      const mutedUntil = notificationMuteDeadline(action);
      if (action !== "notifications-resume" && mutedUntil === undefined) return false;
      try {
        updateNotificationPreferences([readyNotificationProject.id], { mutedUntil });
      } catch {
        setProjectMenuError("Could not save notification preferences. Please try again.");
        return false;
      }
    }
    else if (action.startsWith("external-editor:")) {
      const editorId = action.slice("external-editor:".length);
      if (!externalEditors?.some((editor) => editor.id === editorId)) return false;
      void openInExternalEditor(editorId, path)
        .then(() => {
          setProjectMenu(null);
          menuTrigger.current?.focus();
        })
        .catch((error: unknown) => {
          setProjectMenuError(
            error instanceof Error ? error.message : String(error),
          );
        });
      return false;
    }
    else if (action === "notifications-settings") {
      onOpenNotificationSettings?.(path);
    }
    else if (action === "pin" || action === "unpin") onTogglePin(path);
    else if (action === "background") {
      setBackgroundProject({
        project: projectKey,
        name: resolveTabGroupLabel(projectKey, groupLabels, basename(path)),
      });
    } else if (action === "reveal") void revealPath(path);
    else if (action === "archive") {
      onRemoveProject?.(path, { purgeData: false });
    } else if (action === "delete") {
      setRemoving({
        path,
        name: resolveTabGroupLabel(projectKey, groupLabels, basename(path)),
      });
    }
  };

  const onConfirmDelete = () => {
    if (!removing) return;
    assignProjectGroup(removing.path, null);
    onRemoveProject?.(removing.path, { purgeData: true });
    setRemoving(null);
  };

  const pinnedIds = sections.pinned.map((item) => item.path);
  const projectIds = groupedProjectSections.ungrouped.map((item) => item.path);
  const pinnedSortable = useAnimatedReorder(pinnedIds, onReorderPinned, "y");
  const projectSortable = useAnimatedReorder(projectIds, onReorderProjects, "y");
  return (
    <nav
      ref={resize.setPaneRef}
      aria-label="Projects"
      className="sidebar-glass relative flex shrink-0 flex-col border-r border-stroke"
    >
      <div
        className="flex h-10 shrink-0 select-none items-center pr-1.5"
        data-tauri-drag-region="deep"
      >
        {IS_MAC ? <div className="w-[78px] shrink-0" /> : null}
        <DevModeSlot />
        <TabVisitNav
          canGoBack={canGoBack}
          canGoForward={canGoForward}
          onGoBack={onGoBack}
          onGoForward={onGoForward}
          onTogglePanel={settingsOpen ? undefined : onTogglePanel}
          panelActive
        />
      </div>

      {settingsOpen ? (
        <SettingsNav
          section={settingsSection}
          onSelect={(next) => onSelectSettingsSection?.(next)}
          onClose={() => onCloseSettings?.()}
        />
      ) : (
        <>
          <div className="flex shrink-0 flex-col gap-px px-2 pb-2 pt-0.5">
            <RailSearch
              label="Search"
              icon={Search}
              onClick={onSearch}
              active={searchActive}
              shortcut={`${MOD}K`}
              ariaLabel={`Search (${MOD}K)`}
            />
            <div className="mt-0.5" />
            <RailAction
              label="Inbox"
              icon={Inbox}
              onClick={onOpenInbox}
              onOpenContextMenu={(x, y) => {
                menuTrigger.current =
                  document.activeElement instanceof HTMLElement
                    ? document.activeElement
                    : null;
                setProjectMenu(null);
                setNotificationMenu(null);
                setInboxMenu({ x, y });
              }}
              active={inboxActive}
              dot={inboxUnseen}
              ariaLabel={inboxUnseen ? "Inbox, new items" : "Inbox"}
            />
            {notesEnabled ? (
              <RailAction
                label="Notes"
                icon={File}
                onClick={onOpenNotes}
                active={notesActive}
                ariaLabel="Notes"
              />
            ) : null}
            <RailAction
              label="Automations"
              icon={Zap}
              onClick={onOpenAutomations}
              active={automationsActive}
              ariaLabel="Automations"
            />
          </div>

          <div
            ref={(el) => {
              lockOverscroll(el);
              scrollRef.current = el;
            }}
            className="flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-none pb-2"
          >
            {sections.pinned.length > 0 ? (
              <ProjectSection
                label="Pinned"
                items={sections.pinned}
                muteStatuses={muteStatuses}
                cwd={cwd}
                busy={busy}
                sortable={pinnedSortable}
                pinned
                searchActive={
                  searchActive ||
                  inboxActive ||
                  notesActive ||
                  automationsActive
                }
                onSelect={onSelectProject}
                onTogglePin={onTogglePin}
                onContextMenu={onProjectContextMenu}
                onOpenMenu={openProjectMenu}
                groupLabels={groupLabels}
                groupColors={groupColors}
                groupCustomColors={groupCustomColors}
                groupLogos={groupLogos}
                groupMascots={groupMascots}
              />
            ) : null}

            {projectGroups.length > 0 ? (
              <div className="mb-2 shrink-0">
                <ProjectSectionHeader label="Groups" onAddGroup={createGroup} />
                <div className="flex flex-col gap-px px-2">
                  {groupedProjectSections.grouped.map(({ group, items }) => (
                    <ProjectGroupSection
                      key={group.id}
                      group={group}
                      items={items}
                      muteStatuses={muteStatuses}
                      cwd={cwd}
                      busy={busy}
                      searchActive={
                        searchActive ||
                        inboxActive ||
                        notesActive ||
                        automationsActive
                      }
                      onSelect={onSelectProject}
                      onTogglePin={onTogglePin}
                      onContextMenu={onProjectContextMenu}
                      onOpenMenu={openProjectMenu}
                      onReorder={onReorderProjects}
                      onToggleCollapsed={() =>
                        updateProjectGroup(group.id, (current) => ({
                          ...current,
                          collapsed: !current.collapsed,
                        }))
                      }
                      onOpenGroupMenu={(x, y) => {
                        menuTrigger.current =
                          document.activeElement instanceof HTMLElement
                            ? document.activeElement
                            : null;
                        setProjectMenu(null);
                        setNotificationMenu(null);
                        setProjectGroupMenu({ x, y, id: group.id });
                      }}
                      groupLabels={groupLabels}
                      groupColors={groupColors}
                      groupCustomColors={groupCustomColors}
                      groupLogos={groupLogos}
                      groupMascots={groupMascots}
                    />
                  ))}
                </div>
              </div>
            ) : null}

            <ProjectSection
              label="Projects"
              items={groupedProjectSections.ungrouped}
              muteStatuses={muteStatuses}
              emptyLabel={
                sections.projects.length === 0 && projectGroups.length === 0
                  ? "No projects yet"
                  : undefined
              }
              onAdd={onOpenProject}
              cwd={cwd}
              busy={busy}
              sortable={projectSortable}
              pinned={false}
              searchActive={
                searchActive || inboxActive || notesActive || automationsActive
              }
              onSelect={onSelectProject}
              onTogglePin={onTogglePin}
              onContextMenu={onProjectContextMenu}
              onOpenMenu={openProjectMenu}
              groupLabels={groupLabels}
              groupColors={groupColors}
              groupCustomColors={groupCustomColors}
              groupLogos={groupLogos}
              groupMascots={groupMascots}
            />
          </div>
          <LiveAgentsPreview
            agents={liveAgents}
            activeSessionId={activeSessionId}
            onSelect={onSelectAgent}
            groupLabels={groupLabels}
            groupColors={groupColors}
            groupCustomColors={groupCustomColors}
            groupMascots={groupMascots}
          />
          <SidebarUpdateFooter
            update={updateNotice}
            onOpenWhatsNew={onOpenWhatsNew}
            onDismissUpdate={onDismissUpdate}
          />
          <div className="flex shrink-0 flex-col gap-px p-2">
            <GithubStarPrompt />
            <RailAction
              label="Settings"
              icon={Settings}
              onClick={onOpenSettings}
              shortcut={`${MOD},`}
              ariaLabel={`Settings (${MOD},)`}
            />
          </div>
        </>
      )}
      {projectMenu ? (
        <TabGroupMenu
          x={projectMenu.x}
          y={projectMenu.y}
          groupId={projectMenu.projectKey}
          label={resolveTabGroupLabel(
            projectMenu.projectKey,
            groupLabels,
            basename(projectMenu.path),
          )}
          colorIndex={resolveTabGroupColorIndex(
            projectMenu.projectKey,
            groupColors,
            groupCustomColors,
          )}
          customColor={resolveTabGroupCustomColor(
            projectMenu.projectKey,
            groupCustomColors,
          )}
          currentColor={resolveTabGroupColor(
            projectMenu.projectKey,
            groupColors,
            groupCustomColors,
            projectName(projectMenu.path),
          )}
          logoPath={resolveTabGroupLogo(projectMenu.projectKey, groupLogos)}
          logoProject={projectMenu.path}
          mascotName={resolveTabGroupMascot(
            projectMenu.projectKey,
            groupMascots,
          )}
          mascotProject={projectName(projectMenu.path)}
          onRename={onProjectRename}
          onColorChange={onProjectColorChange}
          onCustomColorChange={onProjectCustomColorChange}
          onMascotChange={onProjectMascotChange}
          onLogoChange={() => {}}
          onPick={() => {}}
          onClose={() => {
            setProjectMenu(null);
            menuTrigger.current?.focus();
          }}
          showActions={false}
          leadingAction={menuMuteStatus ? {
            id: "notifications-resume",
            label: "Resume notifications",
            description: menuMuteStatus,
            icon: BellOff,
          } : undefined}
          extraItems={projectMenuExtraItems(
            pinnedPaths.some((pinned) =>
              sameProjectPath(pinned, projectMenu.path),
            ),
            Boolean(onRemoveProject),
            Boolean(onOpenNotificationSettings),
            Boolean(readyNotificationProject),
            externalEditors,
            projectGroups,
            projectGroupIdForPath(projectMenu.path, projectGroupAssignments),
          )}
          footer={projectMenuError ? (
            <p role="alert" className="px-2 py-1 text-xs text-red-400">{projectMenuError}</p>
          ) : null}
          onExtraPick={onProjectMenuPick}
        />
      ) : null}
      {projectGroupMenu ? (
        <ProjectGroupAppearanceMenu
          menu={projectGroupMenu}
          groups={projectGroups}
          onRename={(id, name) =>
            updateProjectGroup(id, (group) => ({
              ...group,
              name: name.trim() || group.name,
            }))
          }
          onColorChange={(id, colorIndex) =>
            updateProjectGroup(id, (group) => ({
              ...group,
              colorIndex: colorIndex ?? undefined,
              customColor: undefined,
            }))
          }
          onCustomColorChange={(id, customColor) =>
            updateProjectGroup(id, (group) => ({
              ...group,
              colorIndex: undefined,
              customColor,
            }))
          }
          onMascotChange={(id, mascot) =>
            updateProjectGroup(id, (group) => ({
              ...group,
              mascot: mascot ?? undefined,
            }))
          }
          onDelete={deleteGroup}
          onClose={() => {
            setProjectGroupMenu(null);
            menuTrigger.current?.focus();
          }}
        />
      ) : null}
      {notificationMenu ? (
        <ProjectNotificationDatePicker
          key={notificationMenu.path}
          {...notificationMenu}
          onClose={() => {
            setNotificationMenu(null);
            menuTrigger.current?.focus();
          }}
        />
      ) : null}
      {inboxMenu ? (
        <InboxNotificationMenu
          {...inboxMenu}
          projectPaths={[...allProjects.keys()]}
          onOpenSettings={onOpenNotificationSettings}
          onClose={() => {
            setInboxMenu(null);
            menuTrigger.current?.focus();
          }}
        />
      ) : null}
      {removing ? (
        <RemoveProjectDialog
          name={removing.name}
          path={removing.path}
          onConfirm={onConfirmDelete}
          onCancel={() => setRemoving(null)}
        />
      ) : null}
      {backgroundProject ? (
        <ProjectBackgroundDialog
          project={backgroundProject.project}
          name={backgroundProject.name}
          onClose={() => setBackgroundProject(null)}
        />
      ) : null}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize project sidebar"
        aria-valuenow={resize.width}
        aria-valuemin={PROJECT_RAIL_WIDTH_MIN}
        aria-valuemax={PROJECT_RAIL_WIDTH_MAX}
        className={`absolute inset-y-0 -right-px z-10 w-1.5 cursor-col-resize touch-none ${
          resize.dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
      />
    </nav>
  );
}

type SortableHandle = ReturnType<typeof useAnimatedReorder>;

function ProjectNotificationDatePicker({
  project,
  x,
  y,
  onClose,
}: {
  project: NotificationProject;
  x: number;
  y: number;
  onClose: () => void;
}) {
  return (
    <Popover
      anchor={{ x, y }}
      gap={0}
      width={280}
      role="dialog"
      aria-label="Mute project notifications"
      onDismiss={onClose}
      className="space-y-1 overflow-y-auto p-3"
    >
      <p
        className="truncate px-1 text-xs font-medium text-content/85"
        title={project.name}
      >
        {project.name}
      </p>
      <NotificationMuteDatePicker projectIds={[project.id]} onCancel={onClose} onChanged={onClose} />
    </Popover>
  );
}

function ProjectSection({
  label,
  items,
  muteStatuses,
  emptyLabel,
  onAdd,
  cwd,
  busy,
  sortable,
  pinned,
  searchActive,
  onSelect,
  onTogglePin,
  onContextMenu,
  onOpenMenu,
  groupLabels,
  groupColors,
  groupCustomColors,
  groupLogos,
  groupMascots,
}: {
  label: string;
  items: RecentProject[];
  muteStatuses: ReadonlyMap<string, string | null>;
  emptyLabel?: string;
  onAdd?: () => void;
  cwd: string;
  busy: Set<string>;
  sortable: SortableHandle;
  pinned: boolean;
  searchActive: boolean;
  onSelect: (path: string) => void;
  onTogglePin: (path: string) => void;
  onContextMenu: (path: string, event: MouseEvent<HTMLElement>) => void;
  onOpenMenu: (path: string, x: number, y: number) => void;
  groupLabels: Record<string, string>;
  groupColors: Record<string, number>;
  groupCustomColors: Record<string, string>;
  groupLogos: ReturnType<typeof useTabGroupLogos>;
  groupMascots: Record<string, string>;
}) {
  return (
    <div className="shrink-0 mb-2">
      <ProjectSectionHeader label={label} onAdd={onAdd} />
      {items.length === 0 && emptyLabel ? (
        <p className="px-4 pb-1 text-[11px] leading-tight text-content/40">
          {emptyLabel}
        </p>
      ) : null}
      <div className="flex flex-col gap-px px-2">
        {items.map((item) => (
          <ProjectCard
            key={item.path}
            item={item}
            muteStatus={muteStatuses.get(pathKey(item.path)) ?? undefined}
            selected={!searchActive && sameProjectPath(item.path, cwd)}
            busy={isBusyPath(item.path, busy)}
            pinned={pinned}
            sortable={sortable}
            onSelect={onSelect}
            onTogglePin={onTogglePin}
            onContextMenu={onContextMenu}
            onOpenMenu={onOpenMenu}
            groupLabels={groupLabels}
            groupColors={groupColors}
            groupCustomColors={groupCustomColors}
            groupLogos={groupLogos}
            groupMascots={groupMascots}
          />
        ))}
      </div>
    </div>
  );
}

function ProjectSectionHeader({
  label,
  onAdd,
  onAddGroup,
}: {
  label: string;
  onAdd?: () => void;
  onAddGroup?: (x: number, y: number) => void;
}) {
  return (
    <div className="flex items-center gap-1 px-3 pb-1.5 pt-1">
      <span className="min-w-0 flex-1 truncate px-1 text-xs text-content/50">
        {label}
      </span>
      {onAddGroup ? (
        <button
          type="button"
          title="New project group"
          aria-label="New project group"
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            onAddGroup(rect.left, rect.bottom);
          }}
          className="grid size-5 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content"
        >
          <FolderPlus className="size-3.5" strokeWidth={1.75} />
        </button>
      ) : null}
      {onAdd ? (
        <button
          type="button"
          title="Open project"
          aria-label="Open project"
          onClick={onAdd}
          className="grid size-5 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content"
        >
          <Plus className="size-3.5" strokeWidth={1.75} />
        </button>
      ) : null}
    </div>
  );
}

function projectGroupColor(group: ProjectGroup): string {
  if (group.customColor) return group.customColor;
  if (
    group.colorIndex != null &&
    group.colorIndex >= 0 &&
    group.colorIndex < TAB_GROUP_COLORS.length
  ) {
    return TAB_GROUP_COLORS[group.colorIndex];
  }
  return tabGroupColor(group.id);
}

function ProjectGroupAppearanceMenu({
  menu,
  groups,
  onRename,
  onColorChange,
  onCustomColorChange,
  onMascotChange,
  onDelete,
  onClose,
}: {
  menu: { x: number; y: number; id: string };
  groups: ProjectGroup[];
  onRename: (id: string, name: string) => void;
  onColorChange: (id: string, colorIndex: number | null) => void;
  onCustomColorChange: (id: string, color: string) => void;
  onMascotChange: (id: string, mascot: string | null) => void;
  onDelete: (id: string) => boolean;
  onClose: () => void;
}) {
  const group = groups.find((item) => item.id === menu.id);
  if (!group) return null;
  return (
    <TabGroupMenu
      x={menu.x}
      y={menu.y}
      groupId={group.id}
      label={group.name}
      colorIndex={group.colorIndex ?? null}
      customColor={group.customColor ?? null}
      currentColor={projectGroupColor(group)}
      logoPath={null}
      mascotName={group.mascot ?? null}
      mascotProject={group.id}
      onRename={onRename}
      onColorChange={onColorChange}
      onCustomColorChange={onCustomColorChange}
      onMascotChange={onMascotChange}
      onLogoChange={() => {}}
      onPick={() => {}}
      onClose={onClose}
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
        action === "delete-project-group" ? onDelete(group.id) : undefined
      }
    />
  );
}

function ProjectGroupSection({
  group,
  items,
  muteStatuses,
  cwd,
  busy,
  searchActive,
  onSelect,
  onTogglePin,
  onContextMenu,
  onOpenMenu,
  onReorder,
  onToggleCollapsed,
  onOpenGroupMenu,
  groupLabels,
  groupColors,
  groupCustomColors,
  groupLogos,
  groupMascots,
}: {
  group: ProjectGroup;
  items: RecentProject[];
  muteStatuses: ReadonlyMap<string, string | null>;
  cwd: string;
  busy: Set<string>;
  searchActive: boolean;
  onSelect: (path: string) => void;
  onTogglePin: (path: string) => void;
  onContextMenu: (path: string, event: MouseEvent<HTMLElement>) => void;
  onOpenMenu: (path: string, x: number, y: number) => void;
  onReorder: (ids: string[]) => void;
  onToggleCollapsed: () => void;
  onOpenGroupMenu: (x: number, y: number) => void;
  groupLabels: Record<string, string>;
  groupColors: Record<string, number>;
  groupCustomColors: Record<string, string>;
  groupLogos: ReturnType<typeof useTabGroupLogos>;
  groupMascots: Record<string, string>;
}) {
  const sortable = useAnimatedReorder(
    items.map((item) => item.path),
    onReorder,
    "y",
  );
  const countLabel = `${items.length} ${items.length === 1 ? "project" : "projects"}`;
  const expanded = !group.collapsed;
  const openMenu = (target: HTMLElement, x?: number, y?: number) => {
    const rect = target.getBoundingClientRect();
    onOpenGroupMenu(x ?? rect.left, y ?? rect.bottom);
  };

  return (
    <div
      className={`shrink-0 overflow-hidden rounded-md ${
        expanded ? "mb-1.5 bg-content/5" : ""
      }`}
      data-project-group={group.id}
      role="group"
      aria-label={group.name}
    >
      <div
        className="project-reorder-item group relative flex h-8 items-stretch rounded-md px-2 opacity-65 cursor-default"
        onContextMenu={(event) => {
          event.preventDefault();
          event.currentTarget.querySelector<HTMLButtonElement>("button")?.focus();
          openMenu(event.currentTarget, event.clientX, event.clientY);
        }}
      >
        <button
          type="button"
          aria-expanded={!group.collapsed}
          aria-label={`${group.name}, ${countLabel}`}
          title={`${group.name} · ${countLabel}`}
          onClick={onToggleCollapsed}
          className="flex min-w-0 flex-1 cursor-default items-center gap-2 text-left transition-[padding] duration-150 motion-reduce:transition-none group-hover:pr-6 group-has-[:focus-visible]:pr-6"
        >
          <div className="grid size-4 shrink-0 place-items-center">
            {group.collapsed ? (
              <>
                <span
                  data-group-mascot
                  className="grid size-4 place-items-center group-hover:hidden group-has-[:focus-visible]:hidden"
                >
                  <ProjectMascot
                    project={group.id}
                    color={projectGroupColor(group)}
                    name={group.mascot ?? null}
                    className="size-3"
                  />
                </span>
                <ChevronRight
                  data-group-chevron
                  className="hidden size-3.5 group-hover:block group-has-[:focus-visible]:block"
                  strokeWidth={1.75}
                />
              </>
            ) : (
              <ChevronDown
                data-group-chevron
                className="size-3.5"
                strokeWidth={1.75}
              />
            )}
          </div>
          <span className={nameClassName}>{group.name}</span>
        </button>
        <button
          type="button"
          data-no-drag
          title="Group options"
          aria-label={`${group.name} group options`}
          aria-haspopup="menu"
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            openMenu(event.currentTarget);
          }}
          className="absolute right-1 top-1/2 hidden size-6 -translate-y-1/2 place-items-center rounded-md text-content/55 hover:bg-content/8 hover:text-content group-hover:grid group-has-[:focus-visible]:grid"
        >
          <MoreHorizontal className="size-4" strokeWidth={1.75} />
        </button>
      </div>
      {expanded ? (
        <div data-project-group-items className="flex flex-col gap-px p-1">
          {items.map((item) => (
            <ProjectCard
              key={item.path}
              item={item}
              muteStatus={muteStatuses.get(pathKey(item.path)) ?? undefined}
              selected={!searchActive && sameProjectPath(item.path, cwd)}
              busy={isBusyPath(item.path, busy)}
              pinned={false}
              sortable={sortable}
              onSelect={onSelect}
              onTogglePin={onTogglePin}
              onContextMenu={onContextMenu}
              onOpenMenu={onOpenMenu}
              groupLabels={groupLabels}
              groupColors={groupColors}
              groupCustomColors={groupCustomColors}
              groupLogos={groupLogos}
              groupMascots={groupMascots}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

const nameClassName =
  "min-w-0 flex-1 truncate text-sm font-medium leading-tight";

function ProjectCard({
  item,
  muteStatus,
  selected,
  busy,
  pinned,
  sortable,
  onSelect,
  onTogglePin,
  onContextMenu,
  onOpenMenu,
  groupLabels,
  groupColors,
  groupCustomColors,
  groupLogos,
  groupMascots,
}: {
  item: RecentProject;
  muteStatus?: string;
  selected: boolean;
  busy: boolean;
  pinned: boolean;
  sortable: SortableHandle;
  onSelect: (path: string) => void;
  onTogglePin: (path: string) => void;
  onContextMenu: (path: string, event: MouseEvent<HTMLElement>) => void;
  onOpenMenu: (path: string, x: number, y: number) => void;
  groupLabels: Record<string, string>;
  groupColors: Record<string, number>;
  groupCustomColors: Record<string, string>;
  groupLogos: ReturnType<typeof useTabGroupLogos>;
  groupMascots: Record<string, string>;
}) {
  const fallbackName = basename(item.path);
  const key = projectKey(item.path);
  const seed = projectName(item.path);
  const name = resolveTabGroupLabel(key, groupLabels, fallbackName);
  const logoPath = resolveTabGroupLogo(key, groupLogos);
  const color = resolveTabGroupColor(key, groupColors, groupCustomColors, seed);
  const diffEnabled = Boolean(item.path) && item.path !== "~";
  const stats = useProjectDiffStats(item.path, diffEnabled);
  const files = stats?.files ?? 0;
  const additions = stats?.additions ?? 0;
  const deletions = stats?.deletions ?? 0;
  const hasChanges = files > 0 || additions > 0 || deletions > 0;
  const cardTitle = projectCardTitle(item.path, name, stats, busy);
  const cardAriaLabel = projectCardAriaLabel(name, stats, busy);

  return (
    <div
      ref={(el) => sortable.setItemRef(item.path, el)}
      data-selected={selected || undefined}
      className={`reorder-item project-reorder-item group relative flex touch-none items-stretch rounded-md px-2 h-8 ${
        selected
          ? "bg-selection-strong text-content"
          : "opacity-65"
      } cursor-default`}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        if ((event.target as HTMLElement | null)?.closest("[data-no-drag]")) {
          return;
        }
        sortable.onItemPointerDown(item.path, event);
      }}
      onClick={(event) => {
        if ((event.target as HTMLElement | null)?.closest("[data-no-drag]")) {
          return;
        }
        if (sortable.consumeClick()) return;
        onSelect(item.path);
      }}
      onContextMenu={(event) => onContextMenu(item.path, event)}
      onKeyDown={(event) => {
        if (
          event.key !== "ContextMenu" &&
          !(event.shiftKey && event.key === "F10")
        ) return;
        event.preventDefault();
        event.stopPropagation();
        const rect = event.currentTarget.getBoundingClientRect();
        onOpenMenu(item.path, rect.left, rect.bottom);
      }}
    >
      <button
        type="button"
        title={muteStatus ? `${cardTitle}\n${muteStatus}` : cardTitle}
        aria-label={muteStatus ? `${cardAriaLabel}, ${muteStatus}` : cardAriaLabel}
        aria-current={selected ? "true" : undefined}
        className="flex min-w-0 flex-1 cursor-default items-center gap-2 text-left transition-[padding] duration-150 motion-reduce:transition-none group-hover:pr-6 group-has-[:focus-visible]:pr-6"
      >
        <div className="project-card-logo grid size-4 shrink-0 place-items-center transition-opacity group-hover:opacity-0">
          {logoPath && !busy ? (
            <ProjectLogoIcon
              path={logoPath}
              className="size-4 rounded-sm"
              imageClassName="size-4"
            />
          ) : (
            <ProjectMascot
              project={seed}
              color={color}
              name={resolveTabGroupMascot(key, groupMascots)}
              className="size-3"
              active={busy}
            />
          )}
        </div>
        {busy ? (
          <Shimmer as="span" duration={1.4} className={nameClassName}>
            {name}
          </Shimmer>
        ) : (
          <span className={nameClassName}>{name}</span>
        )}
        {hasChanges ? (
          <span className="project-card-stats shrink-0 group-hover:hidden group-has-[:focus-visible]:hidden">
            <ProjectDiffStat additions={additions} deletions={deletions} />
          </span>
        ) : null}
        {muteStatus ? (
          <span
            role="img"
            aria-label={muteStatus}
            title={muteStatus}
            className="grid size-4 shrink-0 place-items-center text-amber-400"
          >
            <BellOff className="size-3.5" strokeWidth={1.75} aria-hidden="true" />
          </span>
        ) : null}
      </button>
      <button
        type="button"
        data-no-drag
        title="Project options"
        aria-label="Project options"
        aria-haspopup="menu"
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          event.stopPropagation();
          const rect = event.currentTarget.getBoundingClientRect();
          onOpenMenu(
            item.path,
            event.detail === 0 ? rect.left : event.clientX,
            event.detail === 0 ? rect.bottom : event.clientY,
          );
        }}
        className="absolute right-1 top-1/2 hidden size-6 -translate-y-1/2 place-items-center rounded-md text-content/55 hover:bg-content/8 hover:text-content group-hover:grid group-has-[:focus-visible]:grid"
      >
        <MoreHorizontal className="size-4" strokeWidth={1.75} />
      </button>
      <button
        type="button"
        data-no-drag
        title={pinned ? "Unpin project" : "Pin project"}
        aria-label={pinned ? "Unpin project" : "Pin project"}
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          event.stopPropagation();
          onTogglePin(item.path);
        }}
        className="absolute left-2 top-1/2 grid size-4 -translate-y-1/2 place-items-center rounded-sm text-content/55 opacity-0 pointer-events-none transition-opacity hover:text-content group-hover:pointer-events-auto group-hover:opacity-100"
      >
        {pinned ? (
          <PinOff className="size-3.5" strokeWidth={1.75} />
        ) : (
          <Pin className="size-3.5" strokeWidth={1.75} />
        )}
      </button>
    </div>
  );
}

function isBusyPath(path: string, busy: Set<string>): boolean {
  for (const other of busy) {
    if (sameProjectPath(path, other)) return true;
  }
  return false;
}

function ProjectDiffStat({
  additions,
  deletions,
}: {
  additions: number;
  deletions: number;
}) {
  if (additions <= 0 && deletions <= 0) return null;

  const label = [
    additions > 0 ? `+${formatInteger(additions)}` : "",
    deletions > 0 ? `-${formatInteger(deletions)}` : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <span
      title={`${label} uncommitted`}
      className="flex shrink-0 items-center gap-1 font-sans text-[11px] font-semibold tabular-nums"
    >
      {additions > 0 ? (
        <span className="text-emerald-400">+{formatInteger(additions)}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-red-400">-{formatInteger(deletions)}</span>
      ) : null}
    </span>
  );
}

function projectCardTitle(
  path: string,
  name: string,
  stats: GitDiffStats | null,
  busy: boolean,
): string {
  const parts = [name, path];
  if (busy) parts.push("Working");
  const files = stats?.files ?? 0;
  const additions = stats?.additions ?? 0;
  const deletions = stats?.deletions ?? 0;
  if (files > 0 || additions > 0 || deletions > 0) {
    parts.push(
      [
        files > 0 ? `${files} ${files === 1 ? "file" : "files"} changed` : "",
        additions > 0 ? `+${formatInteger(additions)}` : "",
        deletions > 0 ? `-${formatInteger(deletions)}` : "",
      ]
        .filter(Boolean)
        .join(" "),
    );
  }
  return parts.join("\n");
}

function projectCardAriaLabel(
  name: string,
  stats: GitDiffStats | null,
  busy: boolean,
): string {
  const parts = [name];
  if (busy) parts.push("working");
  const files = stats?.files ?? 0;
  const additions = stats?.additions ?? 0;
  const deletions = stats?.deletions ?? 0;
  if (files > 0) {
    parts.push(`${files} ${files === 1 ? "file" : "files"} changed`);
  }
  if (additions > 0) parts.push(`+${formatInteger(additions)}`);
  if (deletions > 0) parts.push(`-${formatInteger(deletions)}`);
  return parts.join(", ");
}
