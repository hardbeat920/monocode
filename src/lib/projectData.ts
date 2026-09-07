import { projectKey } from "./paths";
import { clearProjectLogo } from "./projectLogos";
import { normalizeProjectPath } from "./recents";
import { deleteSession, listSessionsByProject } from "./sessionStore";
import { clearTabGroupSettings } from "./tabGroups";

/** Saved chats filed under this project, so the confirm prompt can count them. */
export async function projectSessionCount(path: string): Promise<number> {
  const sessions = await listSessionsByProject(path).catch(() => []);
  return sessions.length;
}

/** Everything we persist for a project: saved chats plus its rail appearance. */
export async function removeProjectData(path: string): Promise<void> {
  const normalized = normalizeProjectPath(path);
  const key = projectKey(normalized);
  const sessions = await listSessionsByProject(normalized).catch(() => []);
  for (const session of sessions) {
    await deleteSession(session.id).catch(() => undefined);
  }
  // Drops the copied image from app data; the localStorage entry goes with it.
  await clearProjectLogo(key).catch(() => undefined);
  clearTabGroupSettings(key);
}
