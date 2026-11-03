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
      className="@container relative flex h-full min-h-0 overflow-y-auto overscroll-none"
    >
      {arcadeEnabled && !hasChatBackground ? <TerminalGridBackground /> : null}
      {composer ? (
        // Same box as the docked composer (max-w-4xl, p-1.5), so the input
        // keeps its width when the first message docks it. The spacers center
        // it when there's room. Once the pane is narrower than the composer
        // (or too short), the bottom spacer gives way so the input sits where
        // it will dock, and the title drops so it reads as the docked composer.
        // `overflow-clip` keeps the composer's blur bleed (`::before`) from
        // counting as scrollable overflow once the input is flush with the
        // pane edges, which let the pane scroll a few pixels both ways.
        <div className="pointer-events-none relative z-10 mx-auto flex w-full max-w-4xl flex-1 flex-col overflow-clip px-1.5 pt-6 pb-1.5">
          <div aria-hidden className="flex-[1_1_0px]" />
          <div className="pointer-events-auto mb-4 shrink-0 px-2.5 @max-4xl:hidden">
            <h1
              className="truncate text-lg text-content"
              title={project ? cwd : undefined}
            >
              {title}
            </h1>
          </div>

          <div className="pointer-events-auto w-full shrink-0">{composer}</div>
          <div aria-hidden className="flex-[1_1_1.125rem] @max-4xl:hidden" />
        </div>
      ) : null}
    </div>
  );
}
