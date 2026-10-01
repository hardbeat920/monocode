import { isHarnessAvailable } from "../../../integrations/harness/core/availability";
import {
  harnessCatalogRefreshResult,
  refreshHarnessCatalog,
} from "../../../integrations/harness/core/registry";
import { hasLiveCatalog, modelsFor } from "../../sessions/model/models";
import {
  HARNESSES,
  RUNTIME_MODES,
  RUNTIME_MODE_HINT,
  RUNTIME_MODE_LABEL,
  type HarnessId,
} from "../../sessions/model/session";
import { readUsedModels } from "../../sessions/model/usedModels";

/** Compact by default; an explicit scope enumerates every known matching row. */
export async function appCapabilities(input: Record<string, unknown>) {
  if (
    input.harness !== undefined &&
    !HARNESSES.includes(input.harness as HarnessId)
  )
    throw new Error("Unknown harness");
  if (
    input.model !== undefined &&
    (typeof input.model !== "string" ||
      !input.model.trim() ||
      input.model.length > 512)
  )
    throw new Error(
      "model must be a non-empty search string under 512 characters",
    );
  const harness = input.harness as HarnessId | undefined;
  const search =
    typeof input.model === "string"
      ? input.model.trim().toLowerCase()
      : undefined;
  // Explicit unavailable scopes can describe fallback data, but never spawn a probe.
  const requestedRefresh =
    harness && isHarnessAvailable(harness)
      ? await refreshHarnessCatalog(harness)
      : undefined;
  const ids = harness
    ? [harness]
    : search
      ? HARNESSES.filter(isHarnessAvailable)
      : HARNESSES;
  return {
    runtimeModes: RUNTIME_MODES.map((id) => ({
      id,
      label: RUNTIME_MODE_LABEL[id],
      description: RUNTIME_MODE_HINT[id],
    })),
    harnesses: ids.map((id) => {
      const catalog = modelsFor(id);
      const models = search
        ? catalog.filter((model) =>
            [
              model.id,
              model.name,
              model.nativeId,
              model.provider?.id,
              model.provider?.name,
            ].some((value) => value?.toLowerCase().includes(search)),
          )
        : catalog;
      return {
        id,
        // Binary availability is not authentication or model-access readiness.
        available: isHarnessAvailable(id),
        catalog: {
          source: hasLiveCatalog(id) ? "live" : "fallback",
          refresh: requestedRefresh ??
            harnessCatalogRefreshResult(id) ?? { status: "not-requested" },
        },
        count: catalog.length,
        ...(harness || search
          ? {
              models: models.map((model) => ({
                id: model.id,
                name: model.name,
                settings: model.settings ?? [],
                ...(model.nativeId !== undefined
                  ? { nativeId: model.nativeId }
                  : {}),
                ...(model.provider ? { provider: model.provider } : {}),
              })),
            }
          : {}),
      };
    }),
    recentModels: readUsedModels(),
  };
}
