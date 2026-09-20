import { openPath } from "@tauri-apps/plugin-opener";
import { GitCompare, GripVertical, Terminal, X } from "../../../shared/ui/icons";
import type { PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { useLayoutEffect, useRef, useState } from "react";
import { copyText } from "../../../platform/tauri/clipboard";
import { basename, revealPath } from "../../../platform/tauri/fs";
import {
  isAgentTab,
  isChangesTab,
  isCommitTab,
  isFilesystemTab,
  isPlanTab,
  isReleaseNotesTab,
  isReviewTab,
  isSessionChangesTab,
  isTerminalTab,
  type FilePaneTab,
} from "../model/layout";
import { displayPath } from "../../../shared/lib/paths";
import { IS_MAC, IS_WIN } from "../../../platform/tauri/platform";
import { releaseNotesTitle } from "../../../app/model/releaseNotes";
import { terminalTabLabel } from "../../terminal/model/terminalTab";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { useAnimatedReorder } from "../../../shared/hooks/useAnimatedReorder";
import { useTabCloseMotion } from "../hooks/useTabCloseMotion";
import { TabWidthMotion } from "../../../app/shell/ClosingTab";
import { ExplorerMenu, type ExplorerMenuItem } from "../../files/ui/ExplorerMenu";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";

type Props = {
  files: FilePaneTab[];
  activeFileId: string;
  dirtyFileIds: Set<string>;
  fileErrorCounts: Map<string, number>;
  onSelectFile: (fileId: string) => void;
  onCloseFile: (fileId: string) => void;
  onCloseOtherFiles: (fileId: string) => void;
  onReorder: (ids: string[]) => void;
  onPaneDragStart?: (event: ReactPointerEvent<HTMLElement>) => void;
  label?: string;
  trailing?: ReactNode;
};

export type SurfaceTabPresentation = {
  name: string;
  label: string;
  iconName: string;
  tooltip: string;
};

type SurfaceTabMenu = {
  x: number;
  y: number;
  fileId: string;
};

const REVEAL_LABEL = IS_MAC
  ? "Reveal in Finder"
  : IS_WIN
    ? "Reveal in File Explorer"
    : "Open Containing Folder";

export function surfaceTabMenuItems(
  file: FilePaneTab,
  canCloseOthers = true,
): ExplorerMenuItem[] {
  const close: ExplorerMenuItem = {
    kind: "item",
    id: "close",
    label: "Close",
  };
  const closeOthers: ExplorerMenuItem = {
    kind: "item",
    id: "close-others",
    label: "Close Others",
    disabled: !canCloseOthers,
  };
  if (!isFilesystemTab(file) || isChangesTab(file)) {
    return [close, closeOthers];
  }

  return [
    { kind: "item", id: "open-default", label: "Open in Default App" },
    { kind: "item", id: "reveal", label: REVEAL_LABEL },
    { kind: "sep" },
    { kind: "item", id: "copy-path", label: "Copy Path" },
    {
      kind: "item",
      id: "copy-relative-path",
      label: "Copy Relative Path",
    },
    { kind: "item", id: "copy-name", label: "Copy File Name" },
    { kind: "sep" },
    close,
    closeOthers,
  ];
}

export function surfaceTabPresentation(
  file: FilePaneTab,
): SurfaceTabPresentation {
  if (isReleaseNotesTab(file)) {
    const title = releaseNotesTitle(file.releaseNotes.version);
    return {
      name: title,
      label: title,
      iconName: "CHANGELOG.md",
      tooltip: title,
    };
  }

  if (isChangesTab(file)) {
    return {
      name: "Changes",
      label: "Changes",
      iconName: "CHANGES",
      tooltip: "Working tree changes",
    };
  }

  if (isSessionChangesTab(file)) {
    return {
      name: "Session Changes",
      label: "Session Changes",
      iconName: "CHANGES",
      tooltip: "Changes captured for this session only",
    };
  }

  if (isAgentTab(file)) {
    const name = file.path.trim() || "Agent";
    return {
      name,
      label: name,
      iconName: "AGENT",
      tooltip: `${name} — orchestration agent`,
    };
  }

  if (isCommitTab(file)) {
    const name = file.commit.subject.trim() || file.commit.shortSha;
    return {
      name,
      label: name,
      iconName: "CHANGES",
      tooltip: `${file.commit.shortSha} — ${file.commit.subject}`,
    };
  }

  const review = isReviewTab(file);
  const terminal = isTerminalTab(file);
  const name = isPlanTab(file)
    ? file.plan.title.trim() || "Plan"
    : terminal
      ? terminalTabLabel(file)
      : basename(file.path);
  return {
    name,
    label: review ? `${name} (Working Tree)` : name,
    iconName: isPlanTab(file) ? "plan.md" : name,
    tooltip: isPlanTab(file)
      ? name
      : terminal
        ? `${name} — ${file.cwd}`
        : review
          ? `${file.path} (Working Tree)`
          : file.path,
  };
}

/** Tab tooltip: the path, then what is wrong with it. */
export function appendProblems(title: string, errors: number): string {
  if (!errors) return title;
  return `${title} — ${errors} ${errors === 1 ? "problem" : "problems"}`;
}

export function SurfaceTabs({
  files,
  activeFileId,
  dirtyFileIds,
  fileErrorCounts,
  onSelectFile,
  onCloseFile,
  onCloseOtherFiles,
  onReorder,
  onPaneDragStart,
  label = "Open files",
  trailing,
}: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const activeTabRef = useRef<HTMLDivElement | null>(null);
  const [menu, setMenu] = useState<SurfaceTabMenu | null>(null);
  const fileIds = files.map((file) => file.id);
  const sortable = useAnimatedReorder(fileIds, onReorder);
  const {
    displayed,
    setTabNode,
    finishMotion,
  } = useTabCloseMotion(files);
  const menuFile = menu
    ? files.find((file) => file.id === menu.fileId)
    : undefined;

  const onMenuPick = (id: string) => {
    if (!menuFile) return;
    setMenu(null);
    if (id === "close") {
      onCloseFile(menuFile.id);
      return;
    }
    if (id === "close-others") {
      onCloseOtherFiles(menuFile.id);
      return;
    }
    if (!isFilesystemTab(menuFile) || isChangesTab(menuFile)) return;

    let action: Promise<void>;
    switch (id) {
      case "open-default":
        action = openPath(menuFile.path);
        break;
      case "reveal":
        action = revealPath(menuFile.path);
        break;
      case "copy-path":
        action = copyText(menuFile.path);
        break;
      case "copy-relative-path":
        action = copyText(displayPath(menuFile.path, menuFile.cwd));
        break;
      case "copy-name":
        action = copyText(basename(menuFile.path));
        break;
      default:
        return;
    }
    void action.catch((error) => {
      console.error(`Failed to run file-tab action ${id}:`, error);
    });
  };

  useLayoutEffect(() => {
    if (sortable.draggingId) return;
    activeTabRef.current?.scrollIntoView({
      inline: "nearest",
      block: "nearest",
    });
  }, [activeFileId, sortable.draggingId]);

  return (
    <div className="flex h-9 min-w-0 shrink-0 border-b border-stroke">
      <div
        ref={lockOverscroll}
        role="tablist"
        aria-label={label}
        className="scrollbar-none flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto overscroll-none pl-1.5 pr-2.5"
      >
      {onPaneDragStart ? (
        <div
          role="button"
          title="Drag to reorder pane"
          aria-label="Drag to reorder pane"
          tabIndex={-1}
          className="grid h-7.5 w-5 shrink-0 cursor-grab place-items-center rounded-md text-content/35 hover:bg-content/5 hover:text-content/70 active:cursor-grabbing touch-none"
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            event.stopPropagation();
            onPaneDragStart(event);
          }}
        >
          <GripVertical className="size-3.5" strokeWidth={1.75} />
        </div>
      ) : null}
      {displayed.map((entry) => {
        const file = entry.item;
        const closing = entry.closing;
        const opening = entry.opening;
        const active = !closing && file.id === activeFileId;
        const dirty = dirtyFileIds.has(file.id);
        const errors = fileErrorCounts.get(file.id) ?? 0;
        const changes = isChangesTab(file);
        const commit = isCommitTab(file);
        const review = isReviewTab(file) && !changes;
        const terminal = isTerminalTab(file);
        const agent = isAgentTab(file) ? file.agent : null;
        const { label, iconName, tooltip } = surfaceTabPresentation(file);
        const tab = (
          <div
            ref={(el) => {
              if (closing) return;
              setTabNode(file.id, el);
              sortable.setItemRef(file.id, el);
              if (el && file.id === activeFileId) activeTabRef.current = el;
            }}
            className={
              closing || opening
                ? "tab-motion group relative flex h-full w-full min-w-0 overflow-hidden items-center"
                : "reorder-item tab-motion group relative flex h-full w-56 min-w-28 shrink touch-none items-center"
            }
            data-tab-slot-id={closing ? undefined : file.id}
            onMouseDownCapture={(event) => {
              if (closing) return;
              if (event.button === 1) event.preventDefault();
            }}
            onAuxClick={(event) => {
              if (closing || event.button !== 1) return;
              event.preventDefault();
              event.stopPropagation();
              onCloseFile(file.id);
            }}
            onPointerDown={(event) => {
              if (closing) return;
              if (event.button !== 0) return;
              if (
                (event.target as HTMLElement | null)?.closest(
                  "[data-no-drag]",
                )
              ) {
                return;
              }
              onSelectFile(file.id);
              sortable.onItemPointerDown(file.id, event);
            }}
            onContextMenu={(event) => {
              if (closing) return;
              event.preventDefault();
              event.stopPropagation();
              onSelectFile(file.id);
              setMenu({
                x: event.clientX,
                y: event.clientY,
                fileId: file.id,
              });
            }}
          >
            <button
              type="button"
              role="tab"
              aria-selected={active}
              title={appendProblems(tooltip, errors)}
              onClick={() => {
                if (sortable.consumeClick()) return;
                onSelectFile(file.id);
              }}
              className={`relative flex h-7.5 min-w-0 flex-1 cursor-default items-center gap-1.5 self-center rounded-md px-2 pr-7 text-left text-[13px] ${
                active
                  ? "bg-selection text-content"
                  : "text-content/50 hover:bg-content/5 hover:text-content"
              }`}
            >
              {terminal ? (
                <Terminal className="size-3.5 shrink-0" strokeWidth={1.75} />
              ) : agent ? (
                <HarnessIcon
                  harness={agent.harness}
                  className="size-3.5 shrink-0"
                />
              ) : changes || commit ? (
                <GitCompare
                  className="size-3.5 shrink-0"
                  strokeWidth={1.75}
                />
              ) : (
                <FileTypeIcon name={iconName} isDir={false} size={14} />
              )}
              <span
                className={`min-w-0 flex-1 truncate ${review ? "italic" : ""} ${
                  errors
                    ? active
                      ? "text-red-400"
                      : "text-red-400/75 group-hover:text-red-400"
                    : ""
                }`}
              >
                {label}
              </span>
              {dirty ? (
                <span
                  className="size-1.5 shrink-0 rounded-full bg-content/70"
                  title="Unsaved changes"
                  aria-label="Unsaved changes"
                />
              ) : null}
            </button>
            <button
              type="button"
              title={`Close ${label}`}
              aria-label={`Close ${label}`}
              data-no-drag
              onPointerDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.stopPropagation();
                onCloseFile(file.id);
              }}
              className={`absolute right-1 top-1/2 grid size-5 -translate-y-1/2 place-items-center rounded text-content/50 hover:bg-content/10 hover:text-content ${
                active ? "opacity-100" : "opacity-0 group-hover:opacity-100"
              }`}
            >
              <X className="size-3" strokeWidth={1.75} />
            </button>
          </div>
        );
        return closing || opening ? (
          <TabWidthMotion
            key={file.id}
              phase={closing ? "closing" : "opening"}
              width={entry.width}
              onFinish={() => finishMotion(file.id)}
          >
            {tab}
          </TabWidthMotion>
        ) : (
          <div key={file.id} className="contents">
            {tab}
          </div>
        );
      })}
      {onPaneDragStart ? (
        <div
          className="min-w-4 flex-1 cursor-grab active:cursor-grabbing"
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            onPaneDragStart(event);
          }}
        />
      ) : null}
      </div>
      {trailing}
      {menu && menuFile ? (
        <ExplorerMenu
          x={menu.x}
          y={menu.y}
          items={surfaceTabMenuItems(menuFile, files.length > 1)}
          ariaLabel="File tab actions"
          onPick={onMenuPick}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
}
