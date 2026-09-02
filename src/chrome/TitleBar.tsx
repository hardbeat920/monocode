import {
  ChevronLeft,
  ChevronRight,
  GitCompare,
  Inbox,
  PanelLeft,
  Plus,
  Search,
  Settings,
  StickyNote,
  Terminal,
  X,
} from "./icons";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { basename } from "../lib/fs";
import { looksLikeProject } from "../lib/recents";
import type { HarnessId } from "../lib/session";
import {
  canJoinTabGroup,
  canJoinTabOnto,
  loadCollapsedTabGroups,
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLabels,
  loadTabGroupMascots,
  loadTabGroupLogos,
  reorderTabSegments,
  resolveTabGroupColor,
  resolveTabGroupColorIndex,
  resolveTabGroupCustomColor,
  resolveTabGroupLabel,
  resolveTabGroupMascot,
  resolveTabGroupLogo,
  saveCollapsedTabGroups,
  saveTabGroupColor,
  saveTabGroupCustomColor,
  saveTabGroupLabel,
  saveTabGroupMascot,
  sharedGroupProject,
  TAB_GROUP_LOGOS_CHANGED,
  segmentTabs,
  type TabGroupSegment,
} from "../lib/tabGroups";
import { TabGroupMenu, type TabGroupMenuAction } from "./TabGroupMenu";
import { CwdPicker } from "./CwdPicker";
import { ExplorerMenu, type ExplorerMenuItem } from "./ExplorerMenu";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { ProjectMascot } from "./ProjectMascot";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { useProjectDiffStats } from "../hooks/useProjectDiffStats";
import { useSegmentDrag } from "../hooks/useSegmentDrag";
import { useSortable, type SortableDropTarget } from "../hooks/useSortable";
import { FileTypeIcon } from "./FileTypeIcon";
import { HarnessIcon } from "./HarnessIcon";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TerminalSpinner } from "./TerminalSpinner";
import { WindowControls } from "./WindowControls";
import { MAC_WINDOW_CHROME, MOD } from "../lib/platform";
import type { RecentProject } from "../lib/recents";

export type Tab = {
  id: string;
  /** Project folder name, e.g. `agent-terminal`. */
  project: string;
  /** Focused conversation title; empty for a fresh session. */
  title: string;
  /** Other conversation titles in this tab, focused session omitted. */
  more: string[];
  sessionCount: number;
  harnesses: HarnessId[];
  /** Harnesses with an in-flight turn in this tab. */
  busyHarnesses: HarnessId[];
  /** Open file basenames, active files first. */
  files: string[];
  /** Split layout with more than one pane in this tab. */
  multiPane?: boolean;
  /** Focus is on a file/terminal pane rather than a conversation pane. */
  fileFocused?: boolean;
  /** Explicit tab group; absent means ungrouped. */
  groupId?: string;
  dirty?: boolean;
  terminal?: boolean;
};

type Props = {
  tabs: Tab[];
  activeId: string;
  cwd: string;
  gitCwd?: string;
  sidebarOpen: boolean;
  deckLayout?: boolean;
  projectRailOpen?: boolean;
  sourceControlActive?: boolean;
  onToggleSidebar: () => void;
  onShowSourceControl?: () => void;
  onSelect: (id: string) => void;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onGoBack?: () => void;
  onGoForward?: () => void;
  onNew: () => void;
  onNewTerminal?: () => void;
  onShowTerminal?: () => void;
  projectTerminalActive?: boolean;
  onOpenSettings?: () => void;
  onOpenInbox?: () => void;
  onOpenNotes?: () => void;
  onClose: (id: string) => void;
  onReorder: (ids: string[], movedId?: string) => void;
  onGoToFile?: () => void;
  onJoinTab?: (draggedId: string, targetId: string) => void;
  onJoinTabToGroup?: (tabId: string, groupId: string) => void;
  onAddToNewGroup?: (tabId: string) => void;
  onAddToGroup?: (tabId: string, groupId: string) => void;
  onRemoveFromGroup?: (tabId: string) => void;
  onUngroup?: (groupId: string) => void;
  onGroupNewTab?: (groupId: string) => void;
  onGroupClose?: (tabIds: string[]) => void;
  onGroupMoveToNewWindow?: (tabIds: string[]) => void;
  recents?: RecentProject[];
  onSelectProject?: (path: string) => void;
  remoteMobile?: boolean;
};

function sessionMeta(tab: Tab): string {
  if (tab.more.length === 1) return tab.more[0];
  if (tab.sessionCount > 1) return `${tab.sessionCount} sessions`;
  return "";
}

export function tabCopy(
  tab: Tab,
  options?: { inGroup?: boolean; deckLayout?: boolean },
): {
  headline: string;
  meta: string;
  tooltip: string;
} {
  const inGroup = options?.inGroup ?? false;
  const deckLayout = options?.deckLayout ?? false;
  const project = tab.project.trim() || "~";
  const conversation = tab.title.trim();
  const file = tab.files[0] ?? "";
  const sessions = sessionMeta(tab);
  const untitled = deckLayout ? "New session" : inGroup ? "New chat" : project;

  let headline: string;
  const metaParts: string[] = [];

  if (tab.multiPane) {
    if (tab.fileFocused && file) {
      headline = file;
      if (conversation) metaParts.push(conversation);
      else if (sessions) metaParts.push(sessions);
    } else if (conversation) {
      headline = conversation;
      if (file) metaParts.push(file);
      else if (sessions) metaParts.push(sessions);
    } else if (file) {
      headline = file;
      if (sessions) metaParts.push(sessions);
    } else {
      headline = untitled;
      if (sessions) metaParts.push(sessions);
    }
  } else {
    headline = conversation || file || untitled;
    if (!deckLayout && !inGroup && headline !== project && project !== "~") {
      metaParts.push(project);
    }
    if (sessions) metaParts.push(sessions);
  }

  const meta = metaParts.join(" · ");

  const tooltipParts = [project];
  if (conversation) tooltipParts.push(conversation);
  tooltipParts.push(...tab.more);
  if (tab.files.length > 0) tooltipParts.push(tab.files.join(", "));
  if (tab.dirty) tooltipParts.push("Unsaved changes");

  return { headline, meta, tooltip: tooltipParts.join(" · ") };
}

/** Which tab-strip edges still have overflow to scroll toward. */
export function tabStripOverflow(
  scrollLeft: number,
  clientWidth: number,
  scrollWidth: number,
): { left: boolean; right: boolean } {
  const maxScroll = scrollWidth - clientWidth;
  if (maxScroll <= 1) return { left: false, right: false };
  return {
    left: scrollLeft > 1,
    right: scrollLeft < maxScroll - 1,
  };
}

function TabHarnesses({
  harnesses,
  busyHarnesses,
  dimmed,
}: {
  harnesses: HarnessId[];
  busyHarnesses: HarnessId[];
  dimmed: boolean;
}) {
  const shown = harnesses.slice(0, 3);
  const extra = harnesses.length - shown.length;
  const opacity = dimmed ? "opacity-55" : "opacity-100";
  const busy = new Set(busyHarnesses);

  return (
    <span className="flex shrink-0 items-center">
      {shown.map((harness, i) => (
        <span
          key={harness}
          className={`grid size-3.5 shrink-0 place-items-center ${opacity} ${
            i > 0 ? "-ml-0.5" : ""
          }`}
        >
          {busy.has(harness) ? (
            <TerminalSpinner className="inline-block w-3.5 select-none text-center text-[11px] leading-none text-accent" />
          ) : (
            <HarnessIcon harness={harness} className="size-3.5 shrink-0" />
          )}
        </span>
      ))}
      {extra > 0 ? (
        <span
          className={`pl-0.5 text-[10px] leading-none ${dimmed ? "text-content/50" : "text-content"}`}
        >
          +{extra}
        </span>
      ) : null}
    </span>
  );
}

type TabGroupPosition = "first" | "middle" | "last" | "only";

type SortableApi = ReturnType<typeof useSortable>;

/** `blocked` marks a drop the tab bar refuses — groups never span projects. */
type TabDropTarget = "join" | "blocked";

function dropTargetFor(
  target: SortableDropTarget | null,
  kind: "tab" | "group",
  id: string,
): TabDropTarget | null {
  if (!target || target.kind !== kind || target.id !== id) return null;
  return target.allowed ? "join" : "blocked";
}

function dropTargetTint(target: TabDropTarget): string {
  return target === "blocked" ? "bg-rose-500/20" : "bg-accent/20";
}

type SegmentDragApi = ReturnType<typeof useSegmentDrag>;

function TitleTabItem({
  tab,
  index,
  active,
  closable,
  canDrag,
  sortable,
  onSelect,
  onClose,
  onContextMenu,
  dropTarget,
  groupPosition,
  itemRef,
  deckLayout = false,
}: {
  tab: Tab;
  index: number;
  active: boolean;
  closable: boolean;
  canDrag: boolean;
  sortable: SortableApi;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onContextMenu?: (event: ReactMouseEvent<HTMLDivElement>) => void;
  dropTarget?: TabDropTarget | null;
  groupPosition?: TabGroupPosition;
  itemRef?: (el: HTMLDivElement | null) => void;
  deckLayout?: boolean;
}) {
  const dragging = canDrag && sortable.draggingId === tab.id;
  const inGroup = groupPosition != null;
  const { headline, meta, tooltip } = tabCopy(tab, { inGroup, deckLayout });
  const fileIcon = tab.files[0];
  const showStart =
    canDrag &&
    sortable.draggingId &&
    sortable.toIndex === index &&
    sortable.fromIndex !== null &&
    sortable.toIndex < sortable.fromIndex;
  const showEnd =
    canDrag &&
    sortable.draggingId &&
    sortable.toIndex === index &&
    sortable.fromIndex !== null &&
    sortable.toIndex > sortable.fromIndex;

  return (
    <div
      ref={(el) => {
        sortable.setItemRef(tab.id, el);
        itemRef?.(el);
      }}
      className={`group @container relative flex h-full cursor-default touch-none items-center self-stretch ${
        deckLayout ? "min-w-0 w-full" : "w-56 min-w-28 shrink"
      } ${dragging ? "opacity-40" : ""}`}
      data-tauri-drag-region="false"
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        if ((event.target as HTMLElement | null)?.closest("[data-no-drag]")) {
          return;
        }
        onSelect(tab.id);
        if (canDrag) sortable.onItemPointerDown(tab.id, event);
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        onContextMenu?.(event);
      }}
    >
      {showStart ? (
        <div className="pointer-events-none absolute inset-y-1.5 left-0 z-20 w-0.5 rounded-full bg-accent" />
      ) : null}
      {showEnd ? (
        <div className="pointer-events-none absolute inset-y-1.5 right-0 z-20 w-0.5 rounded-full bg-accent" />
      ) : null}
      {dropTarget ? (
        <div
          className={`pointer-events-none absolute inset-x-0 inset-y-1.5 z-10 rounded-md ${dropTargetTint(dropTarget)}`}
        />
      ) : null}
      <button
        type="button"
        title={tooltip}
        aria-label={tooltip}
        data-tauri-drag-region="false"
        onClick={() => {
          if (sortable.consumeClick()) return;
          onSelect(tab.id);
        }}
        className={`relative flex h-7.5 min-w-0 flex-1 cursor-default items-center gap-1.5 self-center rounded-md px-2.5 text-left ${
          closable ? "pr-7" : "pr-2.5"
        } ${
          active
            ? "bg-content/10 text-content"
            : "text-content/50 hover:bg-content/5 hover:text-content"
        }`}
      >
        {tab.harnesses.length > 0 ? (
          <TabHarnesses
            harnesses={tab.harnesses}
            busyHarnesses={tab.busyHarnesses}
            dimmed={!active}
          />
        ) : tab.terminal || !fileIcon ? (
          <Terminal
            className={`size-3.5 shrink-0 ${
              active ? "text-content" : "text-content/55"
            }`}
            strokeWidth={1.75}
          />
        ) : (
          <span className={!active ? "opacity-55" : undefined}>
            <FileTypeIcon name={fileIcon} isDir={false} size={14} />
          </span>
        )}
        <span className="flex min-w-0 flex-1 flex-col justify-center gap-0.5">
          <span className="flex min-w-0 items-center gap-1">
            <span
              className={`min-w-0 truncate leading-none ${
                meta
                  ? "text-[13px] @min-[11rem]:text-[10px] @min-[11rem]:font-medium"
                  : "text-[13px]"
              }`}
            >
              {headline}
            </span>
            {tab.dirty ? (
              <span
                className="size-1.5 shrink-0 rounded-full bg-content/70"
                title="Unsaved changes"
                aria-label="Unsaved changes"
              />
            ) : null}
          </span>
          {meta ? (
            <span className="hidden min-w-0 truncate text-[10px] leading-none text-content/45 @min-[11rem]:block">
              {meta}
            </span>
          ) : null}
        </span>
      </button>
      {closable ? (
        <button
          type="button"
          title="Close Tab"
          aria-label={`Close ${headline}`}
          data-no-drag
          data-tauri-drag-region="false"
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            onClose(tab.id);
          }}
          className="absolute right-1 top-1/2 grid size-5 -translate-y-1/2 place-items-center rounded text-content/50 opacity-0 hover:bg-content/10 hover:text-content group-hover:opacity-100"
        >
          <X className="size-3" strokeWidth={1.75} />
        </button>
      ) : null}
    </div>
  );
}

function GroupLabel({
  color,
  collapsed,
  project,
  projectKey,
  mascotName,
  busy,
  count,
  logoPath,
  canDrag,
  onToggle,
  onContextMenu,
  onPointerDown,
  consumeClick,
  dropTarget,
  labelRef,
}: {
  color: string;
  collapsed: boolean;
  project: string;
  projectKey: string;
  mascotName: string | null;
  busy: boolean;
  count: number;
  logoPath?: string | null;
  canDrag: boolean;
  onToggle: () => void;
  onContextMenu: (event: ReactMouseEvent<HTMLButtonElement>) => void;
  onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  consumeClick: () => boolean;
  dropTarget?: TabDropTarget | null;
  labelRef?: (el: HTMLButtonElement | null) => void;
}) {
  return (
    <button
      ref={labelRef}
      type="button"
      title={
        collapsed
          ? `Expand ${project} · ${count} tabs`
          : `Drag to reorder · Click to collapse ${project}`
      }
      aria-label={collapsed ? `Expand ${project}` : `Collapse ${project}`}
      aria-expanded={!collapsed}
      data-tauri-drag-region="false"
      onClick={() => {
        if (consumeClick()) return;
        onToggle();
      }}
      onContextMenu={onContextMenu}
      onPointerDown={onPointerDown}
      className={`relative sticky left-0 z-20 flex h-7.5 max-w-36 shrink-0 cursor-default items-center gap-1.5 self-center rounded-md px-2.5 pr-3 text-[11px] font-medium hover:brightness-110 ${
        canDrag ? "touch-none" : ""
      }`}
      style={{
        background: `color-mix(in srgb, ${color} 20%, var(--color-background-base, #1a1a1a) 80%)`,
      }}
    >
      {dropTarget ? (
        <span
          className={`pointer-events-none absolute inset-0 rounded-md ${dropTargetTint(dropTarget)}`}
        />
      ) : null}
      {logoPath ? (
        <ProjectLogoIcon
          path={logoPath}
          className="size-3.5 shrink-0 rounded-sm"
          imageClassName="size-3.5"
        />
      ) : (
        <ProjectMascot
          project={projectKey}
          color={color}
          name={mascotName}
          className="size-3 min-w-3 shrink-0"
          active={busy}
        />
      )}
      <span className="text-[12.5px] truncate">{project}</span>
      {collapsed ? (
        <span className="text-content shrink-0 tabular-nums opacity-70 pl-1">
          {count}
        </span>
      ) : null}
    </button>
  );
}

function TabGroupBlock({
  segmentIndex,
  segment,
  displayColor,
  displayLabel,
  projectKey,
  mascotName,
  logoPath,
  activeId,
  closable,
  canDrag,
  canDragGroup,
  sortable,
  segmentDrag,
  collapsed,
  dropTarget,
  onSelect,
  onClose,
  onTabContextMenu,
  onToggleCollapse,
  onGroupContextMenu,
  deckLayout = false,
}: {
  segmentIndex: number;
  segment: Extract<TabGroupSegment, { kind: "group" }>;
  displayColor: string;
  displayLabel: string;
  projectKey: string;
  mascotName: string | null;
  logoPath?: string | null;
  activeId: string;
  closable: boolean;
  canDrag: boolean;
  canDragGroup: boolean;
  sortable: SortableApi;
  segmentDrag: SegmentDragApi;
  collapsed: boolean;
  dropTarget?: TabDropTarget | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onTabContextMenu: (tab: Tab, event: ReactMouseEvent<HTMLDivElement>) => void;
  onToggleCollapse: () => void;
  onGroupContextMenu: (event: ReactMouseEvent<HTMLButtonElement>) => void;
  deckLayout?: boolean;
}) {
  const activeInGroup = segment.tabs.some((tab) => tab.id === activeId);
  const groupBusy = segment.tabs.some((tab) => tab.busyHarnesses.length > 0);
  const activeTabRef = useRef<HTMLDivElement | null>(null);
  const draggingGroup = segmentDrag.draggingFromIndex === segmentIndex;
  const showSegmentStart =
    canDragGroup &&
    segmentDrag.draggingFromIndex != null &&
    segmentDrag.toIndex === segmentIndex &&
    segmentDrag.toIndex < segmentDrag.draggingFromIndex;
  const showSegmentEnd =
    canDragGroup &&
    segmentDrag.draggingFromIndex != null &&
    segmentDrag.toIndex === segmentIndex &&
    segmentDrag.toIndex > segmentDrag.draggingFromIndex;

  useEffect(() => {
    if (
      sortable.draggingId ||
      segmentDrag.draggingFromIndex != null ||
      collapsed
    ) {
      return;
    }
    if (activeInGroup) {
      activeTabRef.current?.scrollIntoView({
        inline: "nearest",
        block: "nearest",
      });
    }
  }, [
    activeId,
    activeInGroup,
    collapsed,
    segmentDrag.draggingFromIndex,
    sortable.draggingId,
  ]);

  return (
    <div
      ref={(el) => segmentDrag.setSegmentRef(segmentIndex, el)}
      className={`relative flex h-full cursor-default items-center gap-0.5 ${
        deckLayout ? "min-w-0 flex-1" : "shrink-0"
      } ${draggingGroup ? "opacity-40" : ""}`}
      data-tauri-drag-region="false"
    >
      {showSegmentStart ? (
        <div className="pointer-events-none absolute inset-y-1.5 left-0 z-30 w-0.5 rounded-full bg-accent" />
      ) : null}
      {showSegmentEnd ? (
        <div className="pointer-events-none absolute inset-y-1.5 right-0 z-30 w-0.5 rounded-full bg-accent" />
      ) : null}
      <GroupLabel
        color={displayColor}
        collapsed={collapsed}
        project={displayLabel}
        projectKey={projectKey}
        mascotName={mascotName}
        busy={groupBusy}
        count={segment.tabs.length}
        logoPath={logoPath}
        canDrag={canDragGroup}
        dropTarget={dropTarget}
        labelRef={(el) => sortable.setGroupDropRef(segment.key, el)}
        onToggle={onToggleCollapse}
        onContextMenu={onGroupContextMenu}
        consumeClick={segmentDrag.consumeClick}
        onPointerDown={(event) => {
          if (!canDragGroup) return;
          segmentDrag.onSegmentPointerDown(segmentIndex, event);
        }}
      />
      {collapsed ? null : (
        <>
          {segment.tabs.map((tab, offset) => {
            const index = segment.startIndex + offset;
            const position: TabGroupPosition =
              segment.tabs.length === 1
                ? "only"
                : offset === 0
                  ? "first"
                  : offset === segment.tabs.length - 1
                    ? "last"
                    : "middle";
            return (
              <TitleTabItem
                key={tab.id}
                tab={tab}
                index={index}
                active={tab.id === activeId}
                closable={closable}
                canDrag={canDrag}
                sortable={sortable}
                onSelect={onSelect}
                onClose={onClose}
                onContextMenu={(event) => onTabContextMenu(tab, event)}
                dropTarget={dropTargetFor(sortable.dropTarget, "tab", tab.id)}
                groupPosition={position}
                deckLayout={deckLayout}
                itemRef={
                  tab.id === activeId
                    ? (el) => {
                        activeTabRef.current = el;
                      }
                    : undefined
                }
              />
            );
          })}
        </>
      )}
      <span
        className="pointer-events-none absolute inset-x-1 bottom-1 z-20 h-0.5 rounded-full"
        style={{ background: displayColor }}
        aria-hidden
      />
    </div>
  );
}

function TabStripChevron({
  side,
  onClick,
}: {
  side: "left" | "right";
  onClick: () => void;
}) {
  const label = side === "left" ? "Scroll tabs left" : "Scroll tabs right";
  const Icon = side === "left" ? ChevronLeft : ChevronRight;
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      data-tauri-drag-region="false"
      onPointerDown={(event) => event.stopPropagation()}
      onClick={onClick}
      className={`absolute top-1/2 z-40 grid size-6.5 -translate-y-1/2 place-items-center rounded-md bg-content/10 backdrop-blur-xl text-content/70 hover:bg-content/15 hover:text-content ${
        side === "left" ? "left-1" : "right-1"
      }`}
    >
      <Icon className="size-3.5" strokeWidth={1.75} />
    </button>
  );
}

export function IconButton({
  label,
  active,
  accent,
  disabled,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  accent?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active || accent}
      aria-disabled={disabled}
      data-tauri-drag-region="false"
      onClick={() => {
        if (disabled) return;
        onClick?.();
      }}
      className={`grid size-6.5 place-items-center rounded-md ${
        disabled
          ? "text-content/25"
          : accent
            ? "text-accent hover:bg-content/10"
            : active
              ? "text-content hover:bg-content/10"
              : "text-content/50 hover:bg-content/10 hover:text-content"
      }`}
    >
      {children}
    </button>
  );
}

export function DevModeLabel() {
  if (!import.meta.env.DEV) return null;
  return (
    <span
      title="Development build"
      className="mr-1 min-w-0 truncate rounded-md bg-skill/15 px-1.5 py-0.5 text-[10px] font-medium tracking-wide text-skill"
    >
      Development
    </span>
  );
}

/** Flex spacer that keeps the Development badge next to the visit arrows. */
export function DevModeSlot() {
  return (
    <div className="flex min-w-0 flex-1 items-center justify-end">
      <DevModeLabel />
    </div>
  );
}

export function TabVisitNav({
  canGoBack = false,
  canGoForward = false,
  onGoBack,
  onGoForward,
  onTogglePanel,
  panelActive = false,
  panelLabel = "Toggle Projects",
}: {
  canGoBack?: boolean;
  canGoForward?: boolean;
  onGoBack?: () => void;
  onGoForward?: () => void;
  onTogglePanel?: () => void;
  panelActive?: boolean;
  panelLabel?: string;
}) {
  return (
    <div className="flex shrink-0 items-center">
      <IconButton
        label={`Back (${MOD}[)`}
        disabled={!canGoBack}
        onClick={onGoBack}
      >
        <ChevronLeft className="size-3.5" strokeWidth={1.75} />
      </IconButton>
      <IconButton
        label={`Forward (${MOD}])`}
        disabled={!canGoForward}
        onClick={onGoForward}
      >
        <ChevronRight className="size-3.5" strokeWidth={1.75} />
      </IconButton>
      {onTogglePanel ? (
        <IconButton
          label={panelLabel}
          active={panelActive}
          onClick={onTogglePanel}
        >
          <PanelLeft className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      ) : null}
    </div>
  );
}

/** Back + rail toggle for overlay surfaces when the project rail is closed. */
export function OverlayNav({
  onBack,
  onToggleSidebar,
}: {
  onBack?: () => void;
  onToggleSidebar?: () => void;
}) {
  if (!onBack && !onToggleSidebar) return null;
  return (
    <div className="flex shrink-0 items-center px-1.5">
      {onBack ? (
        <IconButton label={`Back (${MOD}[)`} onClick={onBack}>
          <ChevronLeft className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      ) : null}
      {onToggleSidebar ? (
        <IconButton
          label={`Toggle Sidebar (${MOD}B)`}
          onClick={onToggleSidebar}
        >
          <PanelLeft className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      ) : null}
    </div>
  );
}

function TitleBarComponent({
  tabs,
  activeId,
  cwd,
  gitCwd,
  sidebarOpen,
  deckLayout = false,
  projectRailOpen = true,
  sourceControlActive = false,
  onToggleSidebar,
  onShowSourceControl,
  onSelect,
  canGoBack = false,
  canGoForward = false,
  onGoBack,
  onGoForward,
  onNew,
  onNewTerminal,
  onShowTerminal,
  projectTerminalActive = false,
  onOpenSettings,
  onOpenInbox,
  onOpenNotes,
  onClose,
  onReorder,
  onGoToFile,
  onJoinTab,
  onJoinTabToGroup,
  onAddToNewGroup,
  onAddToGroup,
  onRemoveFromGroup,
  onUngroup,
  onGroupNewTab,
  onGroupClose,
  onGroupMoveToNewWindow,
  recents = [],
  onSelectProject,
  remoteMobile = false,
}: Props) {
  const tabIds = tabs.map((tab) => tab.id);
  const segments = deckLayout
    ? tabs.map((tab, index) => ({ kind: "single" as const, tab, index }))
    : segmentTabs(tabs);
  const canDragSegments = !deckLayout && segments.length > 1;
  const projectOf = (id: string) => tabs.find((tab) => tab.id === id)?.project;
  const sortable = useSortable(
    tabIds,
    onReorder,
    deckLayout
      ? {}
      : {
          onDropOnItem: onJoinTab,
          onDropOnGroup: onJoinTabToGroup,
          // Tab groups are project-scoped; a foreign drop is flagged, not applied.
          canDropOn: (draggedId, kind, id) =>
            kind === "group"
              ? canJoinTabGroup(tabs, draggedId, id, projectOf)
              : canJoinTabOnto(tabs, draggedId, id, projectOf),
        },
  );
  const segmentDrag = useSegmentDrag(segments.length, (fromIndex, toIndex) => {
    const valid = reorderTabSegments(tabs, fromIndex, toIndex);
    if (valid) onReorder(valid);
  });
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const tabStripRef = useRef<HTMLDivElement | null>(null);
  const setTabStripRef = useCallback(
    (el: HTMLDivElement | null) => {
      tabStripRef.current = el;
      lockOverscroll(el);
    },
    [lockOverscroll],
  );
  const [tabOverflow, setTabOverflow] = useState({ left: false, right: false });
  const syncTabOverflow = useCallback(() => {
    const el = tabStripRef.current;
    const next = el
      ? tabStripOverflow(el.scrollLeft, el.clientWidth, el.scrollWidth)
      : { left: false, right: false };
    setTabOverflow((prev) =>
      prev.left === next.left && prev.right === next.right ? prev : next,
    );
  }, []);
  const scrollTabsBy = useCallback((direction: -1 | 1) => {
    const el = tabStripRef.current;
    if (!el) return;
    const amount = Math.max(el.clientWidth * 0.6, 112);
    el.scrollBy({ left: direction * amount, behavior: "smooth" });
  }, []);
  const activeTabRef = useRef<HTMLDivElement | null>(null);
  const closable = tabs.length > 1;
  const canDrag = tabs.length > 1;
  const [collapsedGroups, setCollapsedGroups] = useState(
    loadCollapsedTabGroups,
  );
  const [groupColors, setGroupColors] = useState(loadTabGroupColors);
  const [groupCustomColors, setGroupCustomColors] = useState(
    loadTabGroupCustomColors,
  );
  const [groupLabels, setGroupLabels] = useState(loadTabGroupLabels);
  const [groupLogos, setGroupLogos] = useState(loadTabGroupLogos);
  const [groupMascots, setGroupMascots] = useState(loadTabGroupMascots);
  const [groupMenu, setGroupMenu] = useState<{
    x: number;
    y: number;
    groupId: string;
    tabIds: string[];
  } | null>(null);
  const [tabMenu, setTabMenu] = useState<{
    x: number;
    y: number;
    tabId: string;
  } | null>(null);

  const groupSummaries = segments.flatMap((segment) => {
    if (segment.kind !== "group") return [];
    const shared = sharedGroupProject(segment.tabs);
    return [
      {
        id: segment.key,
        label: resolveTabGroupLabel(
          segment.key,
          groupLabels,
          shared || "Group",
        ),
      },
    ];
  });

  const groupMenuTabs = groupMenu
    ? tabs.filter((tab) => tab.groupId === groupMenu.groupId)
    : [];
  const groupMenuShared = sharedGroupProject(groupMenuTabs);

  const toggleGroupCollapse = useCallback((project: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(project)) next.delete(project);
      else next.add(project);
      saveCollapsedTabGroups(next);
      return next;
    });
  }, []);

  const onGroupContextMenu = useCallback(
    (
      segment: Extract<TabGroupSegment, { kind: "group" }>,
      event: ReactMouseEvent<HTMLButtonElement>,
    ) => {
      event.preventDefault();
      event.stopPropagation();
      setGroupMenu({
        x: event.clientX,
        y: event.clientY,
        groupId: segment.key,
        tabIds: segment.tabs.map((tab) => tab.id),
      });
    },
    [],
  );

  const onGroupMenuPick = useCallback(
    (action: TabGroupMenuAction) => {
      if (!groupMenu) return;
      const { groupId, tabIds } = groupMenu;
      setGroupMenu(null);
      if (action === "new-tab") onGroupNewTab?.(groupId);
      else if (action === "new-window") onGroupMoveToNewWindow?.(tabIds);
      else if (action === "close-group") onGroupClose?.(tabIds);
      else if (action === "ungroup") onUngroup?.(groupId);
      else if (action === "delete-group") {
        const shared =
          tabs.find((tab) => tab.groupId === groupId)?.project ?? "Group";
        if (
          window.confirm(
            `Delete “${resolveTabGroupLabel(groupId, groupLabels, shared)}” and close ${tabIds.length} tabs?`,
          )
        ) {
          onGroupClose?.(tabIds);
        }
      }
    },
    [
      groupLabels,
      groupMenu,
      onGroupClose,
      onGroupMoveToNewWindow,
      onGroupNewTab,
      onUngroup,
      tabs,
    ],
  );

  const onTabContextMenu = useCallback(
    (tab: Tab, event: ReactMouseEvent<HTMLDivElement>) => {
      if (deckLayout) return;
      setTabMenu({ x: event.clientX, y: event.clientY, tabId: tab.id });
    },
    [deckLayout],
  );

  const tabMenuItems: ExplorerMenuItem[] = (() => {
    if (!tabMenu || deckLayout) return [];
    const tab = tabs.find((entry) => entry.id === tabMenu.tabId);
    if (!tab) return [];
    const items: ExplorerMenuItem[] = [
      { kind: "item", id: "new-group", label: "Add to new group" },
    ];
    const others = groupSummaries.filter(
      (group) =>
        group.id !== tab.groupId &&
        canJoinTabGroup(tabs, tab.id, group.id, projectOf),
    );
    if (others.length > 0) {
      items.push({ kind: "sep" });
      for (const group of others) {
        items.push({
          kind: "item",
          id: `add:${group.id}`,
          label: `Add to ${group.label}`,
        });
      }
    }
    if (tab.groupId) {
      items.push({ kind: "sep" });
      items.push({ kind: "item", id: "remove", label: "Remove from group" });
    }
    return items;
  })();

  const onTabMenuPick = useCallback(
    (id: string) => {
      if (!tabMenu) return;
      const tabId = tabMenu.tabId;
      setTabMenu(null);
      if (id === "new-group") onAddToNewGroup?.(tabId);
      else if (id === "remove") onRemoveFromGroup?.(tabId);
      else if (id.startsWith("add:")) onAddToGroup?.(tabId, id.slice(4));
    },
    [onAddToGroup, onAddToNewGroup, onRemoveFromGroup, tabMenu],
  );

  const onGroupRename = useCallback((projectKey: string, label: string) => {
    saveTabGroupLabel(projectKey, label);
    setGroupLabels(loadTabGroupLabels());
  }, []);

  const onGroupMascotChange = useCallback(
    (projectKey: string, name: string | null) => {
      saveTabGroupMascot(projectKey, name);
      setGroupMascots(loadTabGroupMascots());
    },
    [],
  );

  const onGroupColorChange = useCallback(
    (projectKey: string, colorIndex: number | null) => {
      saveTabGroupColor(projectKey, colorIndex);
      setGroupColors(loadTabGroupColors());
      setGroupCustomColors(loadTabGroupCustomColors());
    },
    [],
  );

  const onGroupCustomColorChange = useCallback(
    (projectKey: string, color: string) => {
      saveTabGroupCustomColor(projectKey, color);
      setGroupColors(loadTabGroupColors());
      setGroupCustomColors(loadTabGroupCustomColors());
    },
    [],
  );

  const onGroupLogoChange = useCallback(() => {
    setGroupLogos(loadTabGroupLogos());
  }, []);

  useEffect(() => {
    const refresh = () => setGroupLogos(loadTabGroupLogos());
    window.addEventListener(TAB_GROUP_LOGOS_CHANGED, refresh);
    return () => window.removeEventListener(TAB_GROUP_LOGOS_CHANGED, refresh);
  }, []);

  useEffect(() => {
    if (sortable.draggingId || segmentDrag.draggingFromIndex != null) return;
    activeTabRef.current?.scrollIntoView({
      inline: "nearest",
      block: "nearest",
    });
  }, [activeId, segmentDrag.draggingFromIndex, sortable.draggingId]);

  useLayoutEffect(() => {
    const el = tabStripRef.current;
    if (!el) return;
    syncTabOverflow();
    el.addEventListener("scroll", syncTabOverflow, { passive: true });
    const ro = new ResizeObserver(syncTabOverflow);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", syncTabOverflow);
      ro.disconnect();
    };
  }, [syncTabOverflow]);

  useLayoutEffect(() => {
    syncTabOverflow();
  }, [activeId, collapsedGroups, syncTabOverflow, tabs]);

  const activeTab = useMemo(
    () => tabs.find((t) => t.id === activeId),
    [activeId, tabs],
  );
  const systemTitle = useMemo(() => {
    const activeName = activeTab
      ? activeTab.files[0]
        ? basename(activeTab.files[0])
        : activeTab.project
      : "";
    const project = cwd ? basename(cwd) : "";
    if (activeName && project && activeName !== project) {
      return `${activeName} — ${project} — MonoCode`;
    }
    if (project) {
      return `${project} — MonoCode`;
    }
    return "MonoCode";
  }, [activeTab, cwd]);

  useEffect(() => {
    document.title = systemTitle;
    try {
      void getCurrentWindow().setTitle(systemTitle);
    } catch {}
  }, [systemTitle]);

  const railClosed = deckLayout && !projectRailOpen;
  const showCurrentProject = looksLikeProject(cwd);
  // Until a project is picked, deck mode hides the rail and the sidebar, so
  // nothing project-scoped is actionable and the window controls need room.
  const projectless = deckLayout && !showCurrentProject;
  // An open project is labeled in the sidebar, above Sessions / Explorer /
  // Changes. Without a project that sidebar is gone, so the picker stays here.
  const showProjectButton =
    railClosed && Boolean(onSelectProject) && !showCurrentProject;
  const trailingControls = (
    <div className="flex h-full shrink-0 items-stretch">
      <div className="flex items-center gap-0.5 px-2">
        {!deckLayout ? (
          <ProjectDiffStats
            cwd={gitCwd || cwd}
            active={sourceControlActive}
            onClick={onShowSourceControl}
          />
        ) : null}
        {projectless && railClosed && onOpenInbox ? (
          <IconButton label="Inbox" onClick={onOpenInbox}>
            <Inbox className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {projectless && railClosed && onOpenNotes ? (
          <IconButton label="Notes" onClick={onOpenNotes}>
            <StickyNote className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {railClosed && !projectless ? (
          <>
            <IconButton label={`Go to File (${MOD}P)`} onClick={onGoToFile}>
              <Search className="size-3.5" strokeWidth={1.75} />
            </IconButton>
            <IconButton label={`New session (${MOD}T)`} onClick={onNew}>
              <Plus className="size-3.5" strokeWidth={1.75} />
            </IconButton>
          </>
        ) : null}
        {deckLayout && !projectless && (onShowTerminal || onNewTerminal) ? (
          <IconButton
            label={
              projectTerminalActive ? "Terminal" : `New Terminal (${MOD}\`)`
            }
            accent={projectTerminalActive}
            onClick={
              projectTerminalActive
                ? (onShowTerminal ?? onNewTerminal)
                : onNewTerminal
            }
          >
            <Terminal className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {deckLayout &&
        !projectRailOpen &&
        !showCurrentProject &&
        onOpenSettings ? (
          <IconButton label={`Settings (${MOD},)`} onClick={onOpenSettings}>
            <Settings className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
      </div>
      {!MAC_WINDOW_CHROME ? <WindowControls /> : null}
    </div>
  );

  // "deep" drags from anywhere in the subtree. The bare attribute only drags
  // on a direct hit, which left every label and spacer dead. Tauri still
  // exempts buttons, links and inputs on its own.
  return (
    <header
      className="flex h-10 shrink-0 select-none items-stretch border-b border-content/10"
      data-tauri-drag-region="deep"
    >
      {/* Both the rail and the sidebar step aside without a project, so the
          title bar takes over the traffic lights and the rail toggle. */}
      {(projectless && railClosed) || (!sidebarOpen && MAC_WINDOW_CHROME) ? (
        <div className="w-[78px] shrink-0" />
      ) : null}
      {projectless && railClosed ? (
        <div className="flex shrink-0 items-center px-1.5">
          <IconButton
            label={`Toggle Sidebar (${MOD}B)`}
            onClick={onToggleSidebar}
          >
            <PanelLeft className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        </div>
      ) : null}
      {deckLayout ? null : (
        <div className="flex shrink-0 items-center gap-0.5 px-2">
          {sidebarOpen ? null : (
            <>
              <DevModeLabel />
              <TabVisitNav
                canGoBack={canGoBack}
                canGoForward={canGoForward}
                onGoBack={onGoBack}
                onGoForward={onGoForward}
              />
            </>
          )}
          <IconButton
            label={`Toggle Sidebar (${MOD}B)`}
            active={sidebarOpen}
            onClick={onToggleSidebar}
          >
            <PanelLeft className="size-3.5" strokeWidth={1.75} />
          </IconButton>
          <IconButton label={`Go to File (${MOD}P)`} onClick={onGoToFile}>
            <Search className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        </div>
      )}
      {remoteMobile && deckLayout && railClosed && !projectless ? (
        <div className="flex shrink-0 items-center px-1.5">
          <IconButton
            label={`Toggle Sidebar (${MOD}B)`}
            active={!railClosed}
            onClick={onToggleSidebar}
          >
            <PanelLeft className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        </div>
      ) : null}
      {showProjectButton && onSelectProject ? (
        <CwdPicker
          cwd={cwd}
          recents={recents}
          placement="below"
          onCwdChange={onSelectProject}
          onNewTerminal={onNewTerminal}
          buttonClassName="flex h-full min-w-0 max-w-64 shrink items-center gap-2 px-6 text-left text-sm font-medium leading-tight"
        >
          <span className="min-w-0 truncate text-content/50">No project</span>
        </CwdPicker>
      ) : null}

      <div
        className={`flex min-w-0 flex-1 items-stretch${
          deckLayout && !showProjectButton ? "" : " border-l border-content/10"
        }`}
      >
        {/*
          Strip sizes to its tabs (w-56 each), sits left; + follows.
          When crowded, tabs shrink to min-w-28 then the strip scrolls.
        */}
        <div
          className={`relative h-full min-w-0 overflow-hidden ${
            deckLayout ? "flex-1" : "shrink"
          }`}
          onWheel={(event) => {
            const el = tabStripRef.current;
            if (!el || el.scrollWidth <= el.clientWidth) return;
            if (event.deltaX === 0 && event.deltaY !== 0) {
              el.scrollLeft += event.deltaY;
            }
          }}
        >
          {tabOverflow.left ? (
            <TabStripChevron side="left" onClick={() => scrollTabsBy(-1)} />
          ) : null}
          {tabOverflow.right ? (
            <TabStripChevron side="right" onClick={() => scrollTabsBy(1)} />
          ) : null}
          <div
            ref={setTabStripRef}
            className="scrollbar-none flex h-full min-w-0 cursor-default items-center gap-0.5 overflow-x-auto overflow-y-hidden overscroll-none px-1.5"
          >
            {segments.map((segment, segmentIndex) => {
              const showSegmentStart =
                canDragSegments &&
                segmentDrag.draggingFromIndex != null &&
                segmentDrag.toIndex === segmentIndex &&
                segmentDrag.toIndex < segmentDrag.draggingFromIndex;
              const showSegmentEnd =
                canDragSegments &&
                segmentDrag.draggingFromIndex != null &&
                segmentDrag.toIndex === segmentIndex &&
                segmentDrag.toIndex > segmentDrag.draggingFromIndex;

              if (segment.kind === "group") {
                const shared = sharedGroupProject(segment.tabs);
                return (
                  <TabGroupBlock
                    key={segment.key}
                    segmentIndex={segmentIndex}
                    segment={segment}
                    displayColor={resolveTabGroupColor(
                      segment.key,
                      groupColors,
                      groupCustomColors,
                      shared || segment.key,
                    )}
                    displayLabel={resolveTabGroupLabel(
                      segment.key,
                      groupLabels,
                      shared || "Group",
                    )}
                    projectKey={shared || segment.key}
                    mascotName={resolveTabGroupMascot(
                      segment.key,
                      groupMascots,
                    )}
                    logoPath={
                      shared ? resolveTabGroupLogo(shared, groupLogos) : null
                    }
                    activeId={activeId}
                    closable={closable}
                    canDrag={canDrag}
                    canDragGroup={canDragSegments}
                    sortable={sortable}
                    segmentDrag={segmentDrag}
                    collapsed={collapsedGroups.has(segment.key)}
                    dropTarget={dropTargetFor(
                      sortable.dropTarget,
                      "group",
                      segment.key,
                    )}
                    onSelect={onSelect}
                    onClose={onClose}
                    onTabContextMenu={onTabContextMenu}
                    onToggleCollapse={() => toggleGroupCollapse(segment.key)}
                    onGroupContextMenu={(event) =>
                      onGroupContextMenu(segment, event)
                    }
                    deckLayout={deckLayout}
                  />
                );
              }

              const tab = segment.tab;
              const active = tab.id === activeId;
              const draggingSegment =
                segmentDrag.draggingFromIndex === segmentIndex;
              return (
                <div
                  key={tab.id}
                  ref={(el) => segmentDrag.setSegmentRef(segmentIndex, el)}
                  className={`relative flex h-full cursor-default items-center ${
                    deckLayout ? "w-56 min-w-28 shrink" : "shrink-0"
                  } ${draggingSegment ? "opacity-40" : ""}`}
                  data-tauri-drag-region="false"
                >
                  {showSegmentStart ? (
                    <div className="pointer-events-none absolute inset-y-1.5 left-0 z-30 w-0.5 rounded-full bg-accent" />
                  ) : null}
                  {showSegmentEnd ? (
                    <div className="pointer-events-none absolute inset-y-1.5 right-0 z-30 w-0.5 rounded-full bg-accent" />
                  ) : null}
                  <TitleTabItem
                    tab={tab}
                    index={segment.index}
                    active={active}
                    closable={closable}
                    canDrag={canDrag}
                    sortable={sortable}
                    onSelect={onSelect}
                    onClose={onClose}
                    onContextMenu={(event) => onTabContextMenu(tab, event)}
                    dropTarget={dropTargetFor(
                      sortable.dropTarget,
                      "tab",
                      tab.id,
                    )}
                    deckLayout={deckLayout}
                    itemRef={
                      tab.id === activeId
                        ? (el) => {
                            activeTabRef.current = el;
                          }
                        : undefined
                    }
                  />
                </div>
              );
            })}
          </div>
        </div>

        {groupMenu ? (
          <TabGroupMenu
            x={groupMenu.x}
            y={groupMenu.y}
            groupId={groupMenu.groupId}
            label={resolveTabGroupLabel(
              groupMenu.groupId,
              groupLabels,
              groupMenuShared || "Group",
            )}
            colorIndex={resolveTabGroupColorIndex(
              groupMenu.groupId,
              groupColors,
              groupCustomColors,
            )}
            customColor={resolveTabGroupCustomColor(
              groupMenu.groupId,
              groupCustomColors,
            )}
            currentColor={resolveTabGroupColor(
              groupMenu.groupId,
              groupColors,
              groupCustomColors,
              groupMenuShared || groupMenu.groupId,
            )}
            logoPath={
              groupMenuShared
                ? resolveTabGroupLogo(groupMenuShared, groupLogos)
                : null
            }
            logoProject={groupMenuShared}
            mascotName={resolveTabGroupMascot(groupMenu.groupId, groupMascots)}
            mascotProject={groupMenuShared || groupMenu.groupId}
            onRename={onGroupRename}
            onColorChange={onGroupColorChange}
            onCustomColorChange={onGroupCustomColorChange}
            onMascotChange={onGroupMascotChange}
            onLogoChange={onGroupLogoChange}
            onPick={onGroupMenuPick}
            onClose={() => setGroupMenu(null)}
          />
        ) : null}

        {tabMenu ? (
          <ExplorerMenu
            x={tabMenu.x}
            y={tabMenu.y}
            items={tabMenuItems}
            ariaLabel="Tab actions"
            onPick={onTabMenuPick}
            onClose={() => setTabMenu(null)}
          />
        ) : null}

        {/* Deck mode keeps New in the sidebar and Terminal in the title bar,
            so the strip carries no trailing actions. */}
        {deckLayout ? null : (
          <div className="flex shrink-0 items-center gap-0.5 border-l border-content/10 px-1.5">
            <IconButton label={`New Tab (${MOD}T)`} onClick={onNew}>
              <Plus className="size-3.5" strokeWidth={1.75} />
            </IconButton>
            {onNewTerminal ? (
              <IconButton
                label={`New Terminal (${MOD}\`)`}
                onClick={onNewTerminal}
              >
                <Terminal className="size-3.5" strokeWidth={1.75} />
              </IconButton>
            ) : null}
          </div>
        )}

        {!remoteMobile && !(deckLayout && MAC_WINDOW_CHROME) ? (
          <div className="flex min-w-0 flex-1 items-center justify-center px-4">
            {!MAC_WINDOW_CHROME ? (
              <span className="pointer-events-none truncate text-[11.5px] font-medium text-content/40 select-none">
                {systemTitle}
              </span>
            ) : null}
          </div>
        ) : null}
        {trailingControls}
      </div>
    </header>
  );
}

export const TitleBar = memo(TitleBarComponent);

function ProjectDiffStats({
  cwd,
  active,
  onClick,
}: {
  cwd: string;
  active: boolean;
  onClick?: () => void;
}) {
  const enabled = Boolean(cwd) && cwd !== "~" && Boolean(onClick);
  const stats = useProjectDiffStats(cwd, enabled);
  if (!enabled) return null;

  const files = stats?.files ?? 0;
  const additions = stats?.additions ?? 0;
  const deletions = stats?.deletions ?? 0;
  const hasStats = additions > 0 || deletions > 0;
  const label = hasStats
    ? [
        `${files} ${files === 1 ? "file" : "files"} changed`,
        additions > 0 ? `+${additions}` : "",
        deletions > 0 ? `-${deletions}` : "",
      ]
        .filter(Boolean)
        .join(" ")
    : active
      ? "Hide changes"
      : "Show changes";

  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      data-tauri-drag-region="false"
      onClick={onClick}
      className={`flex h-6.5 items-center gap-1 rounded-md ${
        hasStats
          ? "px-1.5 font-mono text-[11px] font-semibold tabular-nums"
          : "w-6.5 justify-center"
      } ${
        active
          ? "bg-content/10 text-content"
          : "text-content/50 hover:bg-content/10 hover:text-content"
      }`}
    >
      {hasStats ? null : (
        <GitCompare className="size-3.5 shrink-0" strokeWidth={1.75} />
      )}
      {additions > 0 ? (
        <span className="text-emerald-400">+{additions}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-red-400">-{deletions}</span>
      ) : null}
    </button>
  );
}
