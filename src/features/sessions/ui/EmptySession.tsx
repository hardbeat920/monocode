import { type ReactNode, useSyncExternalStore } from "react";
import { basename } from "../../../platform/tauri/fs";
import { projectKey } from "../../../shared/lib/paths";
import { looksLikeProject } from "../../projects/model/recents";
import {
  loadTabGroupLabels,
  resolveTabGroupLabel,
  subscribeTabGroupLabels,
} from "../../workspace/model/tabGroups";
import {
  loadGridArcadeEnabled,
  subscribeGridArcadeEnabled,
} from "../../settings/model/settings";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { TerminalGridBackground } from "../../terminal/ui/TerminalGridBackground";

type Props = {
  cwd: string;
  composer?: ReactNode;
  hasChatBackground?: boolean;
};

export function EmptySession({ cwd, composer, hasChatBackground }: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const arcadeEnabled = useSyncExternalStore(
    subscribeGridArcadeEnabled,
    loadGridArcadeEnabled,
    () => true,
  );
  const getProjectLabel = () =>
    looksLikeProject(cwd)
      ? resolveTabGroupLabel(
          projectKey(cwd),
          loadTabGroupLabels(),
          basename(cwd),
        )
      : null;
  const project = useSyncExternalStore(
    subscribeTabGroupLabels,
    getProjectLabel,
    getProjectLabel,
  );
  const title = project
    ? `What should we work on in ${project}?`
    : "What should we work on?";

  return (
    <div
      ref={lockOverscroll}
      className="relative flex h-full min-h-0 overflow-y-auto overscroll-none"
    >
      {arcadeEnabled && !hasChatBackground ? <TerminalGridBackground /> : null}
      {composer ? (
        // Same box as the docked composer (max-w-4xl, p-1.5), so the input
        // keeps its width when the first message docks it.
        <div className="pointer-events-none relative z-10 mx-auto flex w-full max-w-4xl flex-1 flex-col justify-center px-1.5 py-12">
          <div className="pointer-events-auto mb-4 px-2.5">
            <h1
              className="truncate text-lg text-content"
              title={project ? cwd : undefined}
            >
              {title}
            </h1>
          </div>

          <div className="pointer-events-auto w-full">{composer}</div>
        </div>
      ) : null}
    </div>
  );
}
