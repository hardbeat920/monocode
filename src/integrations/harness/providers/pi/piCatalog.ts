import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels } from "../../../../features/sessions/model/models";
import {
  killChild,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import type { CatalogRefreshResult } from "../../core/registry";
import { PiRpc } from "./piClient";
import { OMP_FLAVOR, PI_FLAVOR, type PiFlavor } from "./piFlavor";
import { buildPiSpawnArgs, modelsFromRpcData } from "./piProtocol";

const DISCOVERY_TIMEOUT_MS = 45_000;

const inflight = new Map<string, Promise<CatalogRefreshResult>>();

function refreshCatalog(flavor: PiFlavor): Promise<CatalogRefreshResult> {
  const running = inflight.get(flavor.id);
  if (running) return running;
  const run = discoverModels(flavor)
    .then((models): CatalogRefreshResult => {
      if (models.length === 0)
        return { status: "failed", error: `${flavor.label} catalog returned no models` };
      setHarnessModels(flavor.id, models);
      return { status: "succeeded" };
    })
    .catch((error: unknown): CatalogRefreshResult => {
      console.debug(`[monocode] ${flavor.id} catalog`, error);
      return {
        status: "failed",
        error: error instanceof Error ? error.message : String(error),
      };
    })
    .finally(() => {
      inflight.delete(flavor.id);
    });
  inflight.set(flavor.id, run);
  return run;
}

async function discoverModels(flavor: PiFlavor, workingDirectory?: string) {
  const { path } = await flavor.resolveBinary();
  const cwd = workingDirectory ?? (await homeDir());
  const probeId = `${flavor.probeChildId}-${crypto.randomUUID()}`;
  const rpc = new PiRpc(probeId, () => undefined, flavor.label);

  const stop = async () => {
    rpc.close();
    unwatchChild(probeId);
    await killChild(probeId).catch(() => undefined);
  };

  watchChild(
    probeId,
    (line) => rpc.pushLine(line),
    () => rpc.close(new Error(`${flavor.label} catalog probe exited`)),
  );

  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    await spawnChild(
      probeId,
      path,
      buildPiSpawnArgs(flavor, { noSession: true, noExtensions: true }),
      cwd,
      undefined,
      flavor.id,
    );
    const response = await Promise.race([
      rpc.request({ type: "get_available_models" }, DISCOVERY_TIMEOUT_MS),
      new Promise<never>((_, reject) => {
        timeout = setTimeout(
          () => reject(new Error(`${flavor.label} model discovery timed out`)),
          DISCOVERY_TIMEOUT_MS,
        );
      }),
    ]);
    return modelsFromRpcData(flavor, response.data);
  } finally {
    if (timeout) clearTimeout(timeout);
    await stop();
  }
}

export function refreshPiCatalog(): Promise<CatalogRefreshResult> {
  return refreshCatalog(PI_FLAVOR);
}

export function refreshOmpCatalog(): Promise<CatalogRefreshResult> {
  return refreshCatalog(OMP_FLAVOR);
}

export function discoverPiModels(workingDirectory: string) {
  return discoverModels(PI_FLAVOR, workingDirectory);
}

export function discoverOmpModels(workingDirectory: string) {
  return discoverModels(OMP_FLAVOR, workingDirectory);
}
