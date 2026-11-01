/**
 * What the phone shows of a desktop. Only `projectIds` is picked now: Monos
 * come with their projects. `monoIds` stays in the stored shape so a
 * selection saved before that still loads; nothing reads it.
 */
export type MobileSelection = { projectIds: string[]; monoIds: string[] };

export function normalizeSelection(value: unknown): MobileSelection | null {
  if (!value || typeof value !== "object") return null;
  const item = value as Record<string, unknown>;
  if (!Array.isArray(item.projectIds) || !Array.isArray(item.monoIds))
    return null;
  const ids = (values: unknown[]) => [
    ...new Set(
      values.filter((value): value is string => typeof value === "string"),
    ),
  ];
  return { projectIds: ids(item.projectIds), monoIds: ids(item.monoIds) };
}

/**
 * The catalog narrowed to the picked projects. A session shows only in a
 * picked project, whoever started it. A Mono shows whenever any of its
 * projects is picked; one with no project, which belongs to none to pick,
 * always shows (see sessionMonos.ts for where).
 */
export function selectedWorkspace<
  T extends {
    projects: { id: string }[];
    monos: { id: string; projectIds?: string[] }[];
    sessions: { projectId: string }[];
  },
>(catalog: T, selection: MobileSelection | null) {
  const projects = new Set(selection?.projectIds ?? []);
  return {
    ...catalog,
    projects: catalog.projects.filter((project) => projects.has(project.id)),
    monos: catalog.monos.filter(
      (mono) =>
        !mono.projectIds?.length || mono.projectIds.some((id) => projects.has(id)),
    ),
    sessions: catalog.sessions.filter((session) => projects.has(session.projectId)),
  };
}
