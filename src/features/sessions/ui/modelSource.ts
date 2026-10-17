import { createContext, useContext, useMemo } from "react";
import {
  findModel,
  modelsFor,
  resolveModel,
  type AgentModel,
} from "../model/models";
import type { HarnessId } from "../model/session";
import {
  hasProbedHarnessAvailability,
  isHarnessAvailable,
  probeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import {
  projectOpenCodeModels,
  refreshProjectOpenCodeCatalog,
} from "../../../integrations/harness/providers/opencode/opencodeCatalog";

/** Where the model picker gets its models and provider availability. The
 * default is this computer's catalog; a remote session supplies its host's. */
export type ModelSource = {
  /** Present only for non-local sources; local sources use global stores. */
  id?: string;
  modelsFor(harness: HarnessId): AgentModel[];
  resolve(harness: HarnessId, id?: string): AgentModel;
  find(id: string): AgentModel | undefined;
  available(harness: HarnessId): boolean;
  probed(): boolean;
  /** Refresh availability and catalogs, when the source supports it. */
  refresh(harnesses: HarnessId[]): void;
};

export const LOCAL_MODEL_SOURCE: ModelSource = {
  modelsFor,
  resolve: resolveModel,
  find: findModel,
  available: isHarnessAvailable,
  probed: hasProbedHarnessAvailability,
  refresh: (harnesses) => {
    void probeHarnessAvailability();
    void refreshHarnessCatalogs(harnesses);
  },
};

export const ModelSourceContext =
  createContext<ModelSource>(LOCAL_MODEL_SOURCE);

export function localProjectModelSource(project: string): ModelSource {
  return {
    ...LOCAL_MODEL_SOURCE,
    modelsFor: (harness) =>
      harness === "opencode"
        ? (projectOpenCodeModels(project) ?? modelsFor(harness))
        : modelsFor(harness),
    find: (id) =>
      id.startsWith("opencode:")
        ? (projectOpenCodeModels(project) ?? modelsFor("opencode")).find(
            (model) => model.id === id,
          )
        : findModel(id),
    resolve: (harness, id) => resolveModel(harness, id, project),
    refresh: (harnesses) => {
      LOCAL_MODEL_SOURCE.refresh(
        harnesses.filter((harness) => harness !== "opencode"),
      );
      if (harnesses.includes("opencode"))
        void refreshProjectOpenCodeCatalog(project);
    },
  };
}

export function useModelSource(project?: string): ModelSource {
  const source = useContext(ModelSourceContext);
  return useMemo(
    () =>
      project && source === LOCAL_MODEL_SOURCE
        ? localProjectModelSource(project)
        : source,
    [project, source],
  );
}
