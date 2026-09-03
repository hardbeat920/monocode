import type { HarnessId } from "../lib/session";
import type { GitHistoryCommit } from "../lib/fs";
import { GitChangesPanel } from "./GitChangesPanel";

type Props = {
  cwd: string;
  enabled: boolean;
  textHarness?: HarnessId;
  selectedPath?: string;
  selectedSha?: string;
  onOpenFile: (path: string) => void;
  onOpenCommit: (commit: GitHistoryCommit) => void;
};

export function SourceControl({
  cwd,
  enabled,
  textHarness,
  selectedPath,
  selectedSha,
  onOpenFile,
  onOpenCommit,
}: Props) {
  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <GitChangesPanel
        cwd={cwd}
        enabled={enabled}
        textHarness={textHarness}
        selectedPath={selectedPath}
        selectedSha={selectedSha}
        onOpenFile={onOpenFile}
        onOpenCommit={onOpenCommit}
      />
    </div>
  );
}
