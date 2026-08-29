import { homeDir } from "../fs";
import { setHarnessModels } from "../models";
import {
  killChild,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import { PiRpc } from "./piClient";
import { OMP_FLAVOR, PI_FLAVOR, type PiFlavor } from "./piFlavor";
import { buildPiSpawnArgs, modelsFromRpcData } from "./piProtocol";

const DISCOVERY_TIMEOUT_MS = 45_000;

const inflight = new Map<string, Promise<void>>();

function refreshCatalog(flavor: PiFlavor): Promise<void> {
  const running = inflight.get(flavor.id);
  if (running) return running;
  const run = discoverModels(flavor)
    .then((models) => {
      if (models.length > 0) setHarnessModels(flavor.id, models);
    })
    .catch((error: unknown) => {
      console.debug(`[monocode] ${flavor.id} catalog`, error);
    })
    .finally(() => {
      inflight.delete(flavor.id);
    });
  inflight.set(flavor.id, run);
  return run;
}

async function discoverModels(flavor: PiFlavor) {
  const { path } = await flavor.resolveBinary();
  const cwd = await homeDir();
  const probeId = flavor.probeChildId;
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

  try {
    await spawnChild(
      probeId,
      path,
      buildPiSpawnArgs(flavor, { noSession: true, noExtensions: true }),
      cwd,
    );
    const response = await Promise.race([
      rpc.request({ type: "get_available_models" }, DISCOVERY_TIMEOUT_MS),
      new Promise<never>((_, reject) => {
        setTimeout(
          () => reject(new Error(`${flavor.label} model discovery timed out`)),
          DISCOVERY_TIMEOUT_MS,
        );
      }),
    ]);
    return modelsFromRpcData(flavor, response.data);
  } finally {
    await stop();
  }
}

export function refreshPiCatalog(): Promise<void> {
  return refreshCatalog(PI_FLAVOR);
}

export function refreshOmpCatalog(): Promise<void> {
  return refreshCatalog(OMP_FLAVOR);
}
