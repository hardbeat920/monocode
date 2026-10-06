import { useSyncExternalStore } from "react";
import {
  figmaSessionTargetFor,
  subscribeFigmaSessionTarget,
  type FigmaSessionTarget,
} from "../model/figmaTarget";

export function useFigmaSessionTarget(
  project: string,
): FigmaSessionTarget | null {
  const current = () => figmaSessionTargetFor(project);
  return useSyncExternalStore(subscribeFigmaSessionTarget, current, current);
}
