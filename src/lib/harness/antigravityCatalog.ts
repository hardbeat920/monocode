import { homeDir } from "../fs";
import { setHarnessModels } from "../models";
import { execChild, resolveAntigravityBinary } from "./child";
import { modelsFromAntigravityOutput } from "./antigravityProtocol";

let inflight: Promise<void> | null = null;

export function refreshAntigravityCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverAntigravityModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("antigravity", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] antigravity catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverAntigravityModels() {
  const { path } = await resolveAntigravityBinary();
  const cwd = await homeDir();
  const stdout = await execChild(path, ["models"], cwd);
  return modelsFromAntigravityOutput(stdout);
}
