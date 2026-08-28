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
/** Rust-side backstop, above the timeout so our own cleanup normally wins. */
const PROBE_TTL_MS = 60_000;

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

  const stop = () => {
    rpc.close();
    unwatchChild(probeId);
    void killChild(probeId).catch(() => undefined);
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
      // Listing models needs no extension host. Loading one cost 2.9s and
      // 357 MB against 0.8s and 190 MB, and it is what pulled in the child
      // process that kept the leaked probe busy.
      buildPiSpawnArgs(flavor, { noSession: true, noExtensions: true }),
      cwd,
      PROBE_TTL_MS,
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
    stop();
  }
}

export function refreshPiCatalog(): Promise<void> {
  return refreshCatalog(PI_FLAVOR);
}

export function refreshOmpCatalog(): Promise<void> {
  return refreshCatalog(OMP_FLAVOR);
}
