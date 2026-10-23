import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels } from "../../../../features/sessions/model/models";
import { execChild, resolveCmdBinary } from "../../core/child";
import {
  defaultModelFromCmdListOutput,
  modelsFromCmdListOutput,
} from "./cmdProtocol";

let inflight: Promise<void> | null = null;

export function refreshCmdCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverCmdModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("cmd", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] cmd catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export async function discoverCmdModels(workingDirectory?: string) {
  const { path } = await resolveCmdBinary();
  const cwd = workingDirectory ?? (await homeDir());
  // The provider tag is required: the backend only runs catalog probes
  // against a resolved harness CLI (provider-less exec is rejected).
  const output = await execChild(path, ["--list-models"], cwd, "cmd");
  const models = modelsFromCmdListOutput(output);
  // Prefer the CLI-flagged default so a fresh catalog keeps working.
  const preferred = defaultModelFromCmdListOutput(output);
  if (preferred) {
    const idx = models.findIndex((model) => model.nativeId === preferred);
    if (idx > 0) {
      const [hit] = models.splice(idx, 1);
      if (hit) models.unshift(hit);
    }
  }
  return models;
}
