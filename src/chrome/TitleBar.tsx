import {
  ChevronLeft,
  ChevronRight,
  PanelLeft,
  Plus,
  Search,
  SquareTerminal,
  X,
} from "lucide-react";
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
import type { HarnessId } from "../lib/session";
import {
  canJoinTabGroup,
  canJoinTabOnto,
  loadCollapsedTabGroups,
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLabels,
  loadTabGroupLogos,
  reorderTabSegments,
  resolveTabGroupColor,
  resolveTabGroupColorIndex,
  resolveTabGroupCustomColor,
  resolveTabGroupLabel,
  resolveTabGroupLogo,
  saveCollapsedTabGroups,
  saveTabGroupColor,
  saveTabGroupCustomColor,
  saveTabGroupLabel,
  sharedGroupProject,
  TAB_GROUP_LOGOS_CHANGED,
  segmentTabs,
  type TabGroupSegment,
} from "../lib/tabGroups";
import { TabGroupMenu, type TabGroupMenuAction } from "./TabGroupMenu";
import { ExplorerMenu, type ExplorerMenuItem } from "./ExplorerMenu";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { useLockOverscroll } from "../lib/useLockOverscroll";
import { useProjectDiffStats } from "../lib/useProjectDiffStats";
import { useSegmentDrag } from "../lib/useSegmentDrag";
import { useSortable, type SortableDropTarget } from "../lib/useSortable";
import { FileTypeIcon } from "./FileTypeIcon";
import { HarnessIcon } from "./HarnessIcon";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TerminalSpinner } from "./TerminalSpinner";
import { OpacityControl } from "./OpacityControl";
import { WindowControls } from "./WindowControls";
import { IS_MAC, MOD } from "../lib/platform";

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
  sidebarOpen: boolean;
  diffOpen?: boolean;
  onToggleSidebar: () => void;
  onToggleDiff?: () => void;
  onSelect: (id: string) => void;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onGoBack?: () => void;
  onGoForward?: () => void;
  onNew: () => void;
  onNewTerminal?: () => void;
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
};

function sessionMeta(tab: Tab): string {
  if (tab.more.length === 1) return tab.more[0];
  if (tab.sessionCount > 1) return `${tab.sessionCount} sessions`;
  return "";
}

export function tabCopy(
  tab: Tab,
  options?: { inGroup?: boolean },
): {
  headline: string;
  meta: string;
  tooltip: string;
} {
  const inGroup = options?.inGroup ?? false;
  const project = tab.project.trim() || "~";
  const conversation = tab.title.trim();
  const file = tab.files[0] ?? "";
  const sessions = sessionMeta(tab);

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
      headline = inGroup ? "New chat" : project;
      if (sessions) metaParts.push(sessions);
    }
  } else {
    headline = conversation || file || (inGroup ? "New chat" : project);
    if (!inGroup && headline !== project && project !== "~") {
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
            <TerminalSpinner className="inline-block w-3.5 select-none text-center text-[11px] leading-none" />
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
  showLeftBorder: showLeftBorderProp,
  itemRef,
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
  showLeftBorder?: boolean;
  itemRef?: (el: HTMLDivElement | null) => void;
}) {
  const dragging = canDrag && sortable.draggingId === tab.id;
  const inGroup = groupPosition != null;
  const { headline, meta, tooltip } = tabCopy(tab, { inGroup });
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
  const showLeftBorder =
    showLeftBorderProp ??
    (inGroup
      ? groupPosition !== "first" && groupPosition !== "only"
      : index > 0);

  return (
    <div
      ref={(el) => {
        sortable.setItemRef(tab.id, el);
        itemRef?.(el);
      }}
      className={`group @container relative flex h-full w-56 min-w-28 shrink touch-none self-stretch ${
        showLeftBorder ? "border-l border-content/10" : ""
      } ${dragging ? "opacity-40" : ""} ${
        canDrag ? "cursor-grab active:cursor-grabbing" : ""
      }`}
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
        <div className="pointer-events-none absolute inset-y-0 left-0 z-20 w-0.5 bg-accent" />
      ) : null}
      {showEnd ? (
        <div className="pointer-events-none absolute inset-y-0 right-0 z-20 w-0.5 bg-accent" />
      ) : null}
      {dropTarget ? (
        <div
          className={`pointer-events-none absolute inset-0 z-10 ${dropTargetTint(dropTarget)}`}
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
        className={`relative flex h-full min-w-0 flex-1 items-center gap-1.5 px-2.5 text-left ${
          closable ? "pr-7" : "pr-2.5"
        } ${canDrag ? "cursor-grab active:cursor-grabbing" : ""} ${
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
          <SquareTerminal
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
                meta ? "text-[10px] font-medium" : "text-[13px]"
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
      className={`relative sticky left-0 z-20 flex h-full max-w-36 shrink-0 items-center gap-1.5 border-r border-content/10 px-4 pr-5 text-[11px] font-medium hover:brightness-110 ${
        canDrag ? "cursor-grab touch-none active:cursor-grabbing" : ""
      }`}
      style={{
        background: `color-mix(in srgb, ${color} 20%, var(--color-background-base, #1a1a1a) 80%)`,
      }}
    >
      <span
        className="pointer-events-none absolute inset-x-0 bottom-0 h-px"
        style={{ background: color }}
        aria-hidden
      />
      {dropTarget ? (
        <span
          className={`pointer-events-none absolute inset-0 ${dropTargetTint(dropTarget)}`}
        />
      ) : null}
      {logoPath ? (
        <ProjectLogoIcon
          path={logoPath}
          className="size-3.5 shrink-0 rounded-sm"
          imageClassName="size-3.5"
        />
      ) : (
        <span
          className="size-2 min-w-2 rounded-full mt-px"
          style={{
            background: `color-mix(in srgb, ${color} 70%, transparent)`,
          }}
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
}: {
  segmentIndex: number;
  segment: Extract<TabGroupSegment, { kind: "group" }>;
  displayColor: string;
  displayLabel: string;
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
}) {
  const activeInGroup = segment.tabs.some((tab) => tab.id === activeId);
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
      className={`relative flex h-full shrink-0 items-stretch ${
        segment.startIndex > 0 ? "border-l border-content/10" : ""
      } ${draggingGroup ? "opacity-40" : ""}`}
      data-tauri-drag-region="false"
    >
      {showSegmentStart ? (
        <div className="pointer-events-none absolute inset-y-0 left-0 z-30 w-0.5 bg-accent" />
      ) : null}
      {showSegmentEnd ? (
        <div className="pointer-events-none absolute inset-y-0 right-0 z-30 w-0.5 bg-accent" />
      ) : null}
      <GroupLabel
        color={displayColor}
        collapsed={collapsed}
        project={displayLabel}
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
                showLeftBorder={offset > 0}
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
        className="pointer-events-none absolute inset-x-0 bottom-0 z-20 h-px"
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

function IconButton({
  label,
  active,
  disabled,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      aria-disabled={disabled}
      data-tauri-drag-region="false"
      onClick={() => {
        if (disabled) return;
        onClick?.();
      }}
      className={`grid size-6.5 place-items-center rounded-md ${
        disabled
          ? "text-content/25"
          : active
            ? "text-content hover:bg-content/10"
            : "text-content/50 hover:bg-content/10 hover:text-content"
      }`}
    >
      {children}
    </button>
  );
}

export function TabVisitNav({
  canGoBack = false,
  canGoForward = false,
  onGoBack,
  onGoForward,
}: {
  canGoBack?: boolean;
  canGoForward?: boolean;
  onGoBack?: () => void;
  onGoForward?: () => void;
}) {
  return (
    <div className="flex shrink-0 items-center gap-0.5">
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
    </div>
  );
}

function TitleBarComponent({
  tabs,
  activeId,
  cwd,
  sidebarOpen,
  diffOpen = false,
  onToggleSidebar,
  onToggleDiff,
  onSelect,
  canGoBack = false,
  canGoForward = false,
  onGoBack,
  onGoForward,
  onNew,
  onNewTerminal,
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
}: Props) {
  const tabIds = tabs.map((tab) => tab.id);
  const segments = segmentTabs(tabs);
  const canDragSegments = segments.length > 1;
  const projectOf = (id: string) => tabs.find((tab) => tab.id === id)?.project;
  const sortable = useSortable(tabIds, onReorder, {
    onDropOnItem: onJoinTab,
    onDropOnGroup: onJoinTabToGroup,
    // Tab groups are project-scoped; a foreign drop is flagged, not applied.
    canDropOn: (draggedId, kind, id) =>
      kind === "group"
        ? canJoinTabGroup(tabs, draggedId, id, projectOf)
        : canJoinTabOnto(tabs, draggedId, id, projectOf),
  });
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
      setTabMenu({ x: event.clientX, y: event.clientY, tabId: tab.id });
    },
    [],
  );

  const tabMenuItems: ExplorerMenuItem[] = (() => {
    if (!tabMenu) return [];
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

  const onTitleBarDoubleClick = useCallback((e: ReactMouseEvent) => {
    if (
      e.target instanceof HTMLElement &&
      e.target.hasAttribute("data-tauri-drag-region")
    ) {
      try {
        void getCurrentWindow().toggleMaximize();
      } catch {}
    }
  }, []);

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

  return (
    <header
      className="flex h-10 shrink-0 items-stretch border-b border-content/10"
      data-tauri-drag-region
      onDoubleClick={onTitleBarDoubleClick}
    >
      {sidebarOpen || !IS_MAC ? null : (
        <div className="w-[78px] shrink-0" data-tauri-drag-region />
      )}

      <div className="flex shrink-0 items-center gap-0.5 px-2">
        {sidebarOpen ? null : (
          <TabVisitNav
            canGoBack={canGoBack}
            canGoForward={canGoForward}
            onGoBack={onGoBack}
            onGoForward={onGoForward}
          />
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

      <div className="flex min-w-0 flex-1 items-stretch border-l border-content/10">
        {/*
          Strip sizes to its tabs (w-56 each), sits left; + follows.
          When crowded, tabs shrink to min-w-28 then the strip scrolls.
        */}
        <div
          className="relative h-full min-w-0 shrink overflow-hidden"
          data-tauri-drag-region="false"
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
            className="scrollbar-none flex h-full min-w-0 items-stretch overflow-x-auto overflow-y-hidden overscroll-none"
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
                  className={`relative flex h-full shrink-0 items-stretch ${
                    draggingSegment ? "opacity-40" : ""
                  }`}
                  data-tauri-drag-region="false"
                >
                  {showSegmentStart ? (
                    <div className="pointer-events-none absolute inset-y-0 left-0 z-30 w-0.5 bg-accent" />
                  ) : null}
                  {showSegmentEnd ? (
                    <div className="pointer-events-none absolute inset-y-0 right-0 z-30 w-0.5 bg-accent" />
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
            onRename={onGroupRename}
            onColorChange={onGroupColorChange}
            onCustomColorChange={onGroupCustomColorChange}
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

        <div className="flex shrink-0 items-center gap-0.5 border-l border-content/10 px-1.5">
          <IconButton label={`New Tab (${MOD}T)`} onClick={onNew}>
            <Plus className="size-3.5" strokeWidth={1.75} />
          </IconButton>
          {onNewTerminal ? (
            <IconButton label={`New Terminal (${MOD}\`)`} onClick={onNewTerminal}>
              <SquareTerminal className="size-3.5" strokeWidth={1.75} />
            </IconButton>
          ) : null}
        </div>

        <div
          className="flex min-w-0 flex-1 items-center justify-center px-4"
          data-tauri-drag-region
        >
          {!IS_MAC ? (
            <span
              className="pointer-events-none truncate text-[11.5px] font-medium text-content/40 select-none"
              data-tauri-drag-region
            >
              {systemTitle}
            </span>
          ) : null}
        </div>

        <div className="flex h-full shrink-0 items-stretch">
          <div className="flex items-center gap-1 px-2">
            <ProjectDiffStats
              cwd={cwd}
              active={diffOpen}
              onClick={onToggleDiff}
            />
            {IS_MAC ? <OpacityControl /> : null}
          </div>
          {!IS_MAC ? <WindowControls /> : null}
        </div>
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
  const enabled = Boolean(cwd) && cwd !== "~";
  const stats = useProjectDiffStats(cwd, enabled);
  if (!enabled) return null;

  const additions = stats?.additions ?? 0;
  const deletions = stats?.deletions ?? 0;
  if (additions <= 0 && deletions <= 0) return null;

  return (
    <button
      type="button"
      title={`+${additions} -${deletions} uncommitted — review diffs`}
      aria-label={`+${additions} -${deletions} uncommitted`}
      aria-pressed={active}
      data-tauri-drag-region="false"
      onClick={onClick}
      className={`flex h-6.5 items-center gap-1 rounded-md px-1.5 font-mono text-[11px] font-semibold tabular-nums ${
        active ? "bg-content/10" : "hover:bg-content/10"
      }`}
    >
      {additions > 0 ? (
        <span className="text-emerald-400">+{additions}</span>
      ) : null}
      {deletions > 0 ? (
        <span className="text-red-400">-{deletions}</span>
      ) : null}
    </button>
  );
}
