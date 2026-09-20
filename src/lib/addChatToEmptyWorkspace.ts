import { composerSeedForAddToChat, type AddToChatMode } from "./quoteDraft";
import { newSessionLike, type Session } from "./session";
import { newTab, type WorkspaceTab } from "./layout";

export type AddChatToEmptyWorkspaceResult = {
  /** Updated sessions array including the seeded replacement chat. */
  sessions: Session[];
  /** Updated tabs array including the opened fallback tab. */
  tabs: WorkspaceTab[];
  /** Fallback tab that now hosts the new chat. */
  tab: WorkspaceTab;
  /** The new chat session, already present in `sessions`. */
  session: Session;
};

/**
 * Zero-tab add-to-chat: build one session seeded with the quoted text and
 * open it as the workspace's replacement tab. The session doubles as the
 * harness/model/settings donor, so exactly one conversation is created.
 */
export function addChatToEmptyWorkspace({
  sessions,
  tabs,
  projectCwd,
  text,
  mode = "quote",
}: {
  sessions: readonly Session[];
  tabs: readonly WorkspaceTab[];
  /** Project directory, preferred over any session-owned cwd fallback. */
  projectCwd: string;
  /** Text quoted into the composer of the new chat. */
  text: string;
  mode?: AddToChatMode;
}): AddChatToEmptyWorkspaceResult | null {
  const composerSeed = composerSeedForAddToChat(text, mode);
  if (!composerSeed) return null;

  // Issue #311 / PR #325 review: with every workspace tab closed, add-to-chat
  // must still open a usable chat. Seed the replacement from the last known
  // session so its harness/model/settings survive, but never from its cwd:
  // sessions[0] may belong to another project, so the project directory
  // always wins.
  const donor = sessions[sessions.length - 1];
  const session = {
    ...newSessionLike(donor, projectCwd),
    composerSeed,
  };
  const tab = newTab(session.id);

  return {
    sessions: [...sessions, session],
    tabs: [...tabs, tab],
    tab,
    session,
  };
}
