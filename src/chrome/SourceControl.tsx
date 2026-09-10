import type { HarnessId } from "../lib/session";
import type { GitFileDiffKind, GitHistoryCommit } from "../lib/fs";
import { GitChangesPanel } from "./GitChangesPanel";

type Props = {
  cwd: string;
  enabled: boolean;
  textHarness?: HarnessId;
  selectedPath?: string;
  selectedKind?: GitFileDiffKind;
  selectedSha?: string;
  onOpenFile: (path: string, kind: GitFileDiffKind) => void;
  onOpenAllChanges: () => void;
  onOpenCommit: (commit: GitHistoryCommit) => void;
};

export function SourceControl({
  cwd,
  enabled,
  textHarness,
  selectedPath,
  selectedKind,
  selectedSha,
  onOpenFile,
  onOpenAllChanges,
  onOpenCommit,
}: Props) {
  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <GitChangesPanel
        cwd={cwd}
        enabled={enabled}
        textHarness={textHarness}
        selectedPath={selectedPath}
        selectedKind={selectedKind}
        selectedSha={selectedSha}
        onOpenFile={onOpenFile}
        onOpenAllChanges={onOpenAllChanges}
        onOpenCommit={onOpenCommit}
      />
    </div>
  );
}
