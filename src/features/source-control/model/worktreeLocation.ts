import { joinPath, pathKey, projectName } from "../../../shared/lib/paths";

/**
 * Where new worktrees go: a project's own folder, else `<global folder>/<project>`,
 * else the `<repo>-worktrees` folder beside the repository.
 */

const GLOBAL_KEY = "monocode.worktreesFolder";
const PROJECT_KEY = "monocode.projectWorktreesFolders.v1";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string | null) {
  try {
    if (value) localStorage.setItem(key, value);
    else localStorage.removeItem(key);
  } catch {
    // private mode / quota
  }
}

function projectFolders(): Record<string, string> {
  const raw = read(PROJECT_KEY);
  if (!raw) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === "object"
      ? (parsed as Record<string, string>)
      : {};
  } catch {
    return {};
  }
}

export function loadWorktreesFolder(): string | null {
  return read(GLOBAL_KEY);
}

export function saveWorktreesFolder(folder: string | null) {
  write(GLOBAL_KEY, folder);
}

/** Keyed by the main checkout, so every worktree of a repo shares it. */
export function loadProjectWorktreesFolder(mainPath: string): string | null {
  const folders = projectFolders();
  const key = pathKey(mainPath);
  return folders[key] || null;
}

export function saveProjectWorktreesFolder(
  mainPath: string,
  folder: string | null,
) {
  const folders = projectFolders();
  const key = pathKey(mainPath);
  if (folder) folders[key] = folder;
  else delete folders[key];
  const serialized = JSON.stringify(folders);
  write(PROJECT_KEY, serialized);
}

export function resolveWorktreesFolder(
  mainPath: string,
  fallback: string,
): string {
  const projectFolder = loadProjectWorktreesFolder(mainPath);
  if (projectFolder) return projectFolder;
  const globalFolder = loadWorktreesFolder();
  if (!globalFolder) return fallback;
  const name = projectName(mainPath);
  return joinPath(globalFolder, name);
}
