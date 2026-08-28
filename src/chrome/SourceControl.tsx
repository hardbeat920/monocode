import type { HarnessId } from "../lib/session";
import { GitChangesPanel } from "./GitChangesPanel";

type Props = {
  cwd: string;
  enabled: boolean;
  textHarness?: HarnessId;
  selectedPath?: string;
  sessionId?: string;
  onOpenFile: (path: string, sessionId?: string) => void;
};

export function SourceControl({
  cwd,
  enabled,
  textHarness,
  selectedPath,
  sessionId,
  onOpenFile,
}: Props) {
  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <GitChangesPanel
        cwd={cwd}
        enabled={enabled}
        textHarness={textHarness}
        selectedPath={selectedPath}
        sessionId={sessionId}
        onOpenFile={onOpenFile}
      />
    </div>
  );
}
