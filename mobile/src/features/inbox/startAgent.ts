import type { Harness, Project } from "@/features/workspace/types";

/**
 * The project an agent starts in: the item's own when the desktop mapped it,
 * else the project the user is in, else the first the phone has.
 */
export function defaultProject(
  projects: Pick<Project, "id">[],
  itemProjectId: string | undefined,
  currentProjectId: string | undefined,
): string | undefined {
  const has = (id: string | undefined) => (id && projects.some((p) => p.id === id) ? id : undefined);
  return has(itemProjectId) ?? has(currentProjectId) ?? projects[0]?.id;
}

export type StartMode = "work" | "ask";

/** The request for `inbox.start`: only what was chosen is sent, so the desktop's defaults fill the rest. */
export function startParams(input: {
  key: string;
  projectId: string;
  mode: StartMode;
  harness?: Harness;
  model?: string;
  text: string;
}) {
  const text = input.text.trim();
  return {
    key: input.key,
    projectId: input.projectId,
    mode: input.mode,
    ...(input.harness ? { harness: input.harness } : {}),
    ...(input.model ? { model: input.model } : {}),
    ...(text ? { text } : {}),
  };
}
