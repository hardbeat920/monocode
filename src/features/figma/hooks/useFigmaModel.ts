import { useSyncExternalStore } from "react";
import {
  getHarnessAvailabilitySnapshot,
  subscribeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { getModelSnapshot, subscribeModels } from "../../sessions/model/models";
import {
  projectProvidersRevision,
  subscribeProjectProviders,
} from "../../sessions/model/projectProviders";
import {
  figmaModelsRevision,
  loadFigmaDefaultModel,
  resolveFigmaModel,
  subscribeFigmaModels,
  type FigmaModelChoice,
  type ResolvedFigmaModel,
} from "../model/figmaModels";

export function useFigmaModel(cwd: string): ResolvedFigmaModel {
  useSyncExternalStore(
    subscribeFigmaModels,
    figmaModelsRevision,
    figmaModelsRevision,
  );
  useSyncExternalStore(
    subscribeProjectProviders,
    projectProvidersRevision,
    projectProvidersRevision,
  );
  useSyncExternalStore(subscribeModels, getModelSnapshot, getModelSnapshot);
  useSyncExternalStore(
    subscribeHarnessAvailability,
    getHarnessAvailabilitySnapshot,
    getHarnessAvailabilitySnapshot,
  );
  return resolveFigmaModel(cwd);
}

export function useFigmaDefaultModel(): FigmaModelChoice | null {
  useSyncExternalStore(
    subscribeFigmaModels,
    figmaModelsRevision,
    figmaModelsRevision,
  );
  return loadFigmaDefaultModel();
}
