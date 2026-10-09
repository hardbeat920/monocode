import { planTitle } from "../../features/sessions/model/plan";
import {
  sessionWorkCwd,
  type Session,
} from "../../features/sessions/model/session";
import {
  newPlanTab,
  openEditorTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";

export function openMonoPlan(options: {
  tab: WorkspaceTab | undefined;
  session: Session | undefined;
  blockId: string;
  closeMonoView: () => void;
  setTabs: (update: (tabs: WorkspaceTab[]) => WorkspaceTab[]) => void;
  setComposerFocused: (focused: boolean) => void;
}): boolean {
  const { tab, session, blockId } = options;
  const block = session?.blocks.find((entry) => entry.id === blockId);
  if (!tab || !session || block?.role !== "plan") return false;
  const file = {
    ...newPlanTab(
      session.id,
      block.id,
      planTitle(block.text),
      sessionWorkCwd(session),
    ),
    ...(session.worktreeCwd ? { projectCwd: session.cwd } : {}),
  };
  options.closeMonoView();
  options.setTabs((prev) =>
    prev.map((entry) =>
      entry.id === tab.id ? openEditorTab(entry, file) : entry,
    ),
  );
  options.setComposerFocused(false);
  return true;
}
