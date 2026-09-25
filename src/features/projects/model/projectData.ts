import { projectKey } from "../../../shared/lib/paths";
import { clearProjectLogo } from "./projectLogos";
import { clearProjectChatBackground } from "./chatBackground";
import {
  clearProjectChatBackgroundSetting,
  rebaseProjectChatBackgroundSetting,
} from "./projectChatBackground";
import { normalizeProjectPath } from "./recents";
import { deleteSession, listSessionsByProject } from "../../sessions/data/sessionStore";
import {
  clearTabGroupSettings,
  rebaseProjectTabGroupSettings,
} from "../../workspace/model/tabGroups";
import {
  rebaseProjectGroupAssignment,
  removeProjectGroupAssignment,
} from "./projectGroups";
import { rebaseSessionFolderSettings } from "../../sessions/model/sessionFolders";
import { clearProjectProviders, rebaseProjectProviders } from "../../sessions/model/projectProviders";
import {
  clearProjectSidebarTab,
  rebaseProjectSidebarTab,
} from "../../settings/model/projectSidebarTab";

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
  await clearProjectChatBackground(key).catch(() => undefined);
  clearProjectChatBackgroundSetting(key);
  clearTabGroupSettings(key);
  removeProjectGroupAssignment(normalized);
  clearProjectProviders(key);
  clearProjectSidebarTab(normalized);
}

/** Move local project settings after the filesystem resolver finds a rename. */
export function rebaseProjectData(from: string, to: string): void {
  const oldKey = projectKey(normalizeProjectPath(from));
  const newKey = projectKey(normalizeProjectPath(to));
  rebaseProjectTabGroupSettings(from, to);
  rebaseProjectGroupAssignment(from, to);
  rebaseProjectChatBackgroundSetting(oldKey, newKey);
  rebaseSessionFolderSettings(from, to);
  rebaseProjectProviders(oldKey, newKey);
  rebaseProjectSidebarTab(from, to);
}
