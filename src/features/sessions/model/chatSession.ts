/** Working directory of a chat that belongs to no project. */
export const CHATS_CWD = "~";

let chatWorkspaceDir: string | null = null;

export function isChatCwd(cwd: string | undefined | null): boolean {
  return cwd === CHATS_CWD;
}

export function isChatSession(session: { cwd: string } | undefined): boolean {
  return isChatCwd(session?.cwd);
}

/** Folder the app keeps for projectless chats, once the backend has made it. */
export function setChatWorkspaceDir(path: string | null): void {
  const trimmed = path?.trim();
  chatWorkspaceDir = trimmed && trimmed !== CHATS_CWD ? trimmed : null;
}

/**
 * Where a harness should actually run for `cwd`. A projectless chat runs in
 * the app's chats folder rather than the home directory; until that folder is
 * known it keeps the home directory it has always had.
 */
export function chatWorkCwd(cwd: string): string {
  return isChatCwd(cwd) && chatWorkspaceDir ? chatWorkspaceDir : cwd;
}
