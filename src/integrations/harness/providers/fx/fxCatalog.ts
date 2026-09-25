import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels } from "../../../../features/sessions/model/models";
import { execChild, resolveFxBinary } from "../../core/child";
import { harnessRuntimeEnv, resolveHarnessBinary } from "../../core/runtime";
import { loadHarnessRuntime } from "../../../../features/settings/model/settings";
import {
  mergeFxCatalogModels,
  modelFromFxStatusOutput,
  modelsFromFxOutput,
} from "./fxProtocol";

let inflight: Promise<void> | null = null;

export function refreshFxCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverFxModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("fx", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] fx catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverFxModels() {
  const { path } = await resolveHarnessBinary("fx", resolveFxBinary);
  const cwd = await homeDir();
  const env = harnessRuntimeEnv(loadHarnessRuntime("fx"));
  const [modelsOutput, statusOutput] = await Promise.all([
    execChild(path, ["models", "--json"], cwd, env),
    execChild(path, ["status", "--json"], cwd, env).catch(() => ""),
  ]);
  return mergeFxCatalogModels(
    modelsFromFxOutput(modelsOutput),
    modelFromFxStatusOutput(statusOutput),
  );
}
