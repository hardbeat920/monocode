import { useSyncExternalStore } from "react";
import type { FigmaActivity } from "../model/figma";
import {
  figmaActivityFor,
  subscribeFigmaActivity,
} from "../model/figmaActivity";

export function useFigmaActivity(project: string): FigmaActivity | null {
  const current = () => figmaActivityFor(project);
  return useSyncExternalStore(subscribeFigmaActivity, current, current);
}
