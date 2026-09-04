import { useState, type MouseEvent as ReactMouseEvent } from "react";
import { copyText } from "../lib/clipboard";
import { revealPath } from "../lib/fs";
import { displayPath } from "../lib/paths";
import { IS_MAC } from "../lib/platform";
import { ExplorerMenu } from "./ExplorerMenu";

const REVEAL_LABEL = IS_MAC
  ? "Reveal in Finder"
  : typeof navigator !== "undefined" && /Win/.test(navigator.platform)
    ? "Reveal in File Explorer"
    : "Open Containing Folder";

/** Right-click menu for a resolved workspace file: open, reveal, copy. */
export function useFileContextMenu({
  filePath,
  cwd,
  onOpenFile,
}: {
  filePath: string | undefined;
  cwd?: string;
  onOpenFile?: (path: string) => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  // Right-press would otherwise select the word under the cursor, and the
  // transcript selection menu would pop in next to ours. Kill the default
  // selection; left-press selection (Add to chat) keeps working.
  const onMouseDown = (event: ReactMouseEvent) => {
    if (filePath && event.button === 2) event.preventDefault();
  };
  const onContextMenu = (event: ReactMouseEvent) => {
    if (!filePath) return;
    event.preventDefault();
    event.stopPropagation();
    // Drop a stale selection (e.g. selected elsewhere before right-clicking
    // the chip) so its menu never stacks under ours.
    window.getSelection()?.removeAllRanges();
    setMenu({ x: event.clientX, y: event.clientY });
  };
  const node =
    menu && filePath ? (
      <ExplorerMenu
        x={menu.x}
        y={menu.y}
        ariaLabel="File actions"
        items={[
          ...(onOpenFile
            ? [{ kind: "item" as const, id: "open", label: "Open" }]
            : []),
          { kind: "item" as const, id: "reveal", label: REVEAL_LABEL },
          { kind: "sep" as const },
          { kind: "item" as const, id: "copy-path", label: "Copy Path" },
          {
            kind: "item" as const,
            id: "copy-relative",
            label: "Copy Relative Path",
          },
        ]}
        onPick={(id) => {
          setMenu(null);
          if (id === "open") onOpenFile?.(filePath);
          else if (id === "reveal") void revealPath(filePath).catch(() => {});
          else if (id === "copy-path") void copyText(filePath).catch(() => {});
          else if (id === "copy-relative")
            void copyText(displayPath(filePath, cwd)).catch(() => {});
        }}
        onClose={() => setMenu(null)}
      />
    ) : null;
  return { onContextMenu, onMouseDown, menu: node };
}
