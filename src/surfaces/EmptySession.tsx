import { type ReactNode, useSyncExternalStore } from "react";
import { basename } from "../lib/fs";
import { projectKey } from "../lib/paths";
import { looksLikeProject } from "../lib/recents";
import {
  loadTabGroupLabels,
  resolveTabGroupLabel,
  subscribeTabGroupLabels,
} from "../lib/tabGroups";
import {
  loadGridArcadeEnabled,
  subscribeGridArcadeEnabled,
} from "../lib/settings";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { TerminalGridBackground } from "./TerminalGridBackground";

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
      ? resolveTabGroupLabel(projectKey(cwd), loadTabGroupLabels(), basename(cwd))
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
        <div className="pointer-events-none relative z-10 mx-auto flex w-full max-w-3xl flex-1 flex-col justify-center px-6 py-12">
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
