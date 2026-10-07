import { DockPanel } from "../../workspace/ui/DockPanel";
import {
  defaultDockSize,
  type DockSide,
  type ProjectTerminalDock,
} from "../../projects/model/projectTerminal";
import { MOD } from "../../../platform/tauri/platform";
import type { TerminalMetaPatch } from "../model/terminalTab";
import { lazySurface } from "../../../shared/ui/lazySurface";

const TerminalView = lazySurface(async () => {
  const module = await import("./TerminalView");
  return { default: module.TerminalView };
});

type Props = {
  dock: ProjectTerminalDock;
  focused: boolean;
  onFocus: () => void;
  onHide: () => void;
  onSideChange: (side: DockSide) => void;
  onSizePaint: (size: number) => void;
  onSizeCommit: (size: number) => void;
  onAddTerminal: () => void;
  onSelectTerminal: (fileId: string) => void;
  onCloseTerminal: (fileId: string) => void;
  onCloseOtherTerminals: (fileId: string) => void;
  onReorderTerminals: (ids: string[]) => void;
  onTerminalMetaChange?: (fileId: string, patch: TerminalMetaPatch) => void;
};

export function ProjectTerminalDock({
  dock,
  focused,
  onFocus,
  onHide,
  onSideChange,
  onSizePaint,
  onSizeCommit,
  onAddTerminal,
  onSelectTerminal,
  onCloseTerminal,
  onCloseOtherTerminals,
  onReorderTerminals,
  onTerminalMetaChange,
}: Props) {
  return (
    <DockPanel
      dock={dock}
      name="Terminal"
      tabsLabel="Terminals"
      addLabel={`New Terminal (${MOD}\`)`}
      hideLabel={`Hide Terminal (${MOD}J)`}
      defaultSize={defaultDockSize}
      onFocus={onFocus}
      onHide={onHide}
      onSideChange={onSideChange}
      onSizePaint={onSizePaint}
      onSizeCommit={onSizeCommit}
      onAdd={onAddTerminal}
      onSelect={onSelectTerminal}
      onClose={onCloseTerminal}
      onCloseOthers={onCloseOtherTerminals}
      onReorder={onReorderTerminals}
    >
      {dock.pane.files.map((file) => (
        <div
          key={file.id}
          aria-hidden={file.id !== dock.pane.activeFileId}
          className={
            file.id === dock.pane.activeFileId
              ? "absolute inset-0 h-full"
              : "hidden"
          }
        >
          <TerminalView
            id={file.id}
            cwd={file.cwd}
            active={focused && file.id === dock.pane.activeFileId}
            onMetaChange={(patch) => onTerminalMetaChange?.(file.id, patch)}
          />
        </div>
      ))}
    </DockPanel>
  );
}
