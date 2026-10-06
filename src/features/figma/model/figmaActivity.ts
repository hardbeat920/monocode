import { sameProjectPath } from "../../projects/model/recents";
import type { FigmaActivity } from "./figma";

let activity: FigmaActivity | null = null;
const listeners = new Set<() => void>();

export function reportFigmaActivity(next: FigmaActivity | null): void {
  activity = next;
  for (const listener of listeners) listener();
}

export function figmaActivityFor(project: string): FigmaActivity | null {
  return activity && sameProjectPath(activity.cwd, project) ? activity : null;
}

export function subscribeFigmaActivity(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
