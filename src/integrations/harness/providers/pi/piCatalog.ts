import { homeDir } from "../../../../platform/tauri/fs";
import {
  setHarnessModels,
  type AgentModel,
} from "../../../../features/sessions/model/models";
import {
  killChild,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { PiRpc } from "./piClient";
import { OMP_FLAVOR, PI_FLAVOR, type PiFlavor } from "./piFlavor";
import { buildPiSpawnArgs, modelsFromRpcData } from "./piProtocol";

const DISCOVERY_TIMEOUT_MS = 45_000;
// Pi can answer RPC before asynchronous extension providers register models.
// Identical early responses are not a readiness signal: observe startup for at
// least five seconds, then require a quiet second, within the overall deadline.
const PI_STARTUP_WINDOW_MS = 5_000;
const PI_CATALOG_QUIET_MS = 1_000;
const PI_CATALOG_POLL_MS = 500;

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

async function discoverModels(flavor: PiFlavor, workingDirectory?: string) {
  const { path } = await flavor.resolveBinary();
  const cwd = workingDirectory ?? (await homeDir());
  const probeId = `${flavor.probeChildId}-${crypto.randomUUID()}`;
  const rpc = new PiRpc(probeId, () => undefined, flavor.label);
  const models = new Map<string, AgentModel>();
  const collectedModels = () =>
    [...models.values()].sort((left, right) =>
      left.name.localeCompare(right.name),
    );
  let stopped = false;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let wakePoll: (() => void) | undefined;
  let exitError: Error | undefined;
  let rejectExit: ((error: Error) => void) | undefined;

  const stop = async () => {
    stopped = true;
    if (pollTimer !== undefined) clearTimeout(pollTimer);
    wakePoll?.();
    rpc.close();
    unwatchChild(probeId);
    await killChild(probeId).catch(() => undefined);
  };

  watchChild(
    probeId,
    (line) => rpc.pushLine(line),
    () => {
      exitError = new Error(`${flavor.label} catalog probe exited`);
      rpc.close(exitError);
      rejectExit?.(exitError);
    },
  );

  let timeout: ReturnType<typeof setTimeout> | undefined;
  const timeoutError = new Error(`${flavor.label} model discovery timed out`);
  try {
    await spawnChild(
      probeId,
      path,
      buildPiSpawnArgs(flavor, {
        noSession: true,
        noExtensions: flavor.id !== "pi",
      }),
      cwd,
      undefined,
      flavor.id,
    );
    if (exitError) throw exitError;
    const started = Date.now();
    const deadline = started + DISCOVERY_TIMEOUT_MS;
    const timedOut = new Promise<never>((_, reject) => {
      timeout = setTimeout(() => reject(timeoutError), DISCOVERY_TIMEOUT_MS);
    });
    const exited = new Promise<never>((_, reject) => {
      rejectExit = reject;
    });
    const pollModels = async () => {
      let previousSnapshot = "";
      let lastChange = started;
      while (!stopped) {
        const response = await rpc.request(
          { type: "get_available_models" },
          Math.max(1, deadline - Date.now()),
        );
        if (stopped) break;
        const snapshot = modelsFromRpcData(flavor, response.data);
        const signature = JSON.stringify(snapshot);
        if (signature !== previousSnapshot) lastChange = Date.now();
        previousSnapshot = signature;
        for (const model of snapshot) models.set(model.id, model);
        if (
          flavor.id !== "pi" ||
          (Date.now() - started >= PI_STARTUP_WINDOW_MS &&
            Date.now() - lastChange >= PI_CATALOG_QUIET_MS)
        ) {
          break;
        }
        await new Promise<void>((resolve) => {
          wakePoll = resolve;
          pollTimer = setTimeout(resolve, PI_CATALOG_POLL_MS);
        });
      }
      return collectedModels();
    };
    return await Promise.race([pollModels(), timedOut, exited]);
  } catch (error) {
    // A slow or continually changing extension must not discard useful models
    // already received, or keep the probe alive beyond the discovery deadline.
    if (error === timeoutError && models.size > 0) return collectedModels();
    throw error;
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
    await stop();
  }
}

export function refreshPiCatalog(): Promise<void> {
  return refreshCatalog(PI_FLAVOR);
}

export function refreshOmpCatalog(): Promise<void> {
  return refreshCatalog(OMP_FLAVOR);
}

export function discoverPiModels(workingDirectory: string) {
  return discoverModels(PI_FLAVOR, workingDirectory);
}

export function discoverOmpModels(workingDirectory: string) {
  return discoverModels(OMP_FLAVOR, workingDirectory);
}
