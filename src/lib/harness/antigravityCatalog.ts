import { setHarnessModels, type AgentModel } from "../models";
import { execChild, resolveAntigravityBinary } from "./child";
import {
  antigravityContextWindow,
  parseAntigravityModels,
  unifyAntigravityCatalogModels,
} from "./antigravityProtocol";

let inflight: Promise<void> | null = null;

/** Fetch the CLI's authenticated model catalog without starting an agent session. */
export function refreshAntigravityCatalog(cwd?: string): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverAntigravityModels(cwd)
    .then((models) => {
      if (models.length) setHarnessModels("antigravity", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] antigravity catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export const antigravityContextWindowForModel = antigravityContextWindow;

export async function discoverAntigravityModels(
  cwd?: string,
): Promise<AgentModel[]> {
  const workingDirectory = cwd?.trim();
  if (!workingDirectory || workingDirectory === "~") return [];
  const { path } = await resolveAntigravityBinary();
  // Even metadata-only commands start an agy backend. Keep that backend in
  // the active project instead of inheriting the desktop app's process cwd
  // (which is commonly `/` when launched from Finder).
  const output = await execChild(
    path,
    ["--output-format", "json", "models"],
    workingDirectory,
  );
  return unifyAntigravityCatalogModels(parseAntigravityModels(output));
}
