/** Desktop project IDs are encoded paths. Expo Router also decodes local route
 * params, so a navigated project can arrive as its original filesystem path.
 * Resolve it back to the catalog entry and keep its canonical ID for RPCs. */
export function projectForRoute<T extends { id: string; path: string }>(
  projects: readonly T[],
  projectId: string | undefined,
): T | undefined {
  return (
    projects.find((project) => project.id === projectId) ??
    projects.find((project) => project.path === projectId)
  );
}
