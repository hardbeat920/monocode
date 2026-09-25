import {
  rememberProject,
  type RecentProject,
} from "../../features/projects/model/recents";
import type { QuickLaunch } from "../../features/quick-composer/model/quickComposer";
import { applyQuickWorkspace } from "../../features/quick-composer/model/quickWorkspace";
import { prepareAttachments } from "../../features/sessions/model/attachments";
import {
  mergeModelSettings,
  resolveModel,
} from "../../features/sessions/model/models";
import {
  newSession,
  type Session,
  type Attachment,
} from "../../features/sessions/model/session";
import {
  newTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import type { SubmissionAcceptance } from "./submissionAcceptance";

/** Complete the workspace handoff before the receiver acknowledges the launch. */
export async function acceptQuickLaunch(
  launch: QuickLaunch,
  deliveryId: string,
  workspace: {
    getSessions: () => Session[];
    updateSessions: (update: (sessions: Session[]) => Session[]) => void;
    appendTab: (tab: WorkspaceTab, cwd: string) => void;
    setProjectCwd: (cwd: string) => void;
    setRecents: (recents: RecentProject[]) => void;
    revealTab: (id: string, cwd: string) => void;
    submit: (
      sessionId: string,
      text: string,
      attachments: Attachment[],
    ) => SubmissionAcceptance;
    saveDraft?: (
      sessionId: string,
      text: string,
      attachments: Attachment[],
      requestId: string,
    ) => SubmissionAcceptance;
  },
): Promise<void> {
  // Rehydrate image previews in this webview; the floating panel sends paths.
  const attachments = await prepareAttachments(launch.attachments ?? []);
  const existing = workspace
    .getSessions()
    .find((session) => session.id === deliveryId);
  const previous = existing?.blocks.find(
    (block) => block.appRequestId === deliveryId,
  );
  if (previous) {
    if (previous.text !== launch.prompt || (!launch.draft && !!previous.draft))
      throw new Error("Request ID was already used for another session launch");
    return;
  }
  if (
    existing?.quickLaunchAccepted ||
    existing?.blocks.some((block) => block.role === "user" && !block.draft)
  )
    return;
  const session =
    existing ??
    applyQuickWorkspace(
      newSession(launch.harness, launch.cwd, launch.model, launch.runtimeMode),
      launch,
    );
  session.id = deliveryId;
  if (launch.modelSettings) {
    session.modelSettings = mergeModelSettings(
      resolveModel(session.harness, session.model),
      launch.modelSettings,
    );
  }
  if (!existing) {
    workspace.updateSessions((sessions) => [...sessions, session]);
    const tab = newTab(session.id);
    workspace.appendTab(tab, launch.cwd);
    if (launch.reveal) {
      // The title bar filters tabs by this project. Select it before the tab.
      workspace.setProjectCwd(launch.cwd);
      workspace.setRecents(rememberProject(launch.cwd));
      workspace.revealTab(tab.id, launch.cwd);
    }
  }
  if (launch.draft) {
    if (
      !workspace.saveDraft ||
      !(await workspace.saveDraft(
        session.id,
        launch.prompt,
        attachments,
        deliveryId,
      ))
    ) {
      throw new Error("The workspace could not save the session draft yet.");
    }
  } else if (
    !(await workspace.submit(session.id, launch.prompt, attachments))
  ) {
    throw new Error("The workspace could not accept the queued session yet.");
  }
  workspace.updateSessions((sessions) =>
    sessions.map((item) =>
      item.id === deliveryId ? { ...item, quickLaunchAccepted: true } : item,
    ),
  );
}
