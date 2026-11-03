import { looksLikeProject, sameProjectPath } from "./recents";

/**
 * Whether something filed under `path` belongs in a view scoped to `scope`.
 * No scope keeps everything. Anything not filed under a project (a project-
 * less note, a team-level Linear or Jira issue) belongs to every project, so
 * scoping never hides it.
 */
export function inProjectScope(
  path: string | null | undefined,
  scope: string | undefined,
): boolean {
  if (!scope) return true;
  if (!path || !looksLikeProject(path)) return true;
  return sameProjectPath(path, scope);
}
