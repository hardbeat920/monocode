import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels, type AgentModel } from "../../../../features/sessions/model/models";
import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveDroidBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  DROID_ACP_ARGS,
  droidConfigOptionsFrom,
  droidEffortConfig,
  droidSessionId,
  modelsFromDroidSession,
  type DroidConfigOption,
} from "./droidProtocol";

const PROBE_ID = "monocode-droid-probe";
const DISCOVERY_TIMEOUT_MS = 60_000;
const REQUEST_TIMEOUT_MS = 20_000;
const CONFIG_SETTLE_MS = 250;

let inflight: Promise<void> | null = null;

export function refreshDroidCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = probeDroidModels((models) => {
    if (models.length > 0) setHarnessModels("droid", models);
  })
    .catch((error: unknown) => {
      console.debug("[monocode] droid catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

/** Resolves with the full catalog, including each model's effort choices. */
export async function discoverDroidModels(
  workingDirectory?: string,
): Promise<AgentModel[]> {
  let latest: AgentModel[] = [];
  await probeDroidModels((models) => {
    latest = models;
  }, workingDirectory);
  return latest;
}

/**
 * Droid lists its models on session/new but only reports reasoning levels
 * for the selected one. Publish the plain list first, then walk the models on
 * the throwaway probe session (a local switch, no inference) to attach each
 * model's own effort choices.
 */
async function probeDroidModels(
  publish: (models: AgentModel[]) => void,
  workingDirectory?: string,
): Promise<void> {
  const { path } = await resolveDroidBinary();
  const cwd = workingDirectory ?? (await homeDir());
  const probeId = `${PROBE_ID}-${crypto.randomUUID()}`;
  let latestConfig: DroidConfigOption[] | null = null;
  const acp = new AcpClient(probeId, {
    onNotification: (method, params) => {
      if (method !== "session/update") return;
      const options = droidConfigOptionsFrom(params);
      if (options) latestConfig = options;
    },
    onRequest: (id, method) => {
      void acp
        .respondError(id, {
          code: -32601,
          message: `Method not found: ${method}`,
        })
        .catch(() => undefined);
    },
  });

  const stop = async () => {
    acp.close();
    unwatchChild(probeId);
    await killChild(probeId).catch(() => undefined);
  };

  watchChild(
    probeId,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Droid catalog probe exited")),
  );

  try {
    await spawnChild(probeId, path, DROID_ACP_ARGS, cwd, undefined, "droid");
    await withTimeout(
      DISCOVERY_TIMEOUT_MS,
      async () => {
        await acp.request(
          "initialize",
          {
            protocolVersion: 1,
            clientCapabilities: {
              fs: { readTextFile: false, writeTextFile: false },
              terminal: false,
            },
            clientInfo: { name: "monocode", version: "0.1.0" },
          },
          REQUEST_TIMEOUT_MS,
        );
        const created = await acp.request<unknown>(
          "session/new",
          { cwd, mcpServers: [] },
          REQUEST_TIMEOUT_MS,
        );
        const models = modelsFromDroidSession(created);
        publish(models);
        const sessionId = droidSessionId(created);
        if (!sessionId || models.length === 0) return;

        const efforts = new Map<string, DroidConfigOption>();
        const initial = droidEffortConfig(droidConfigOptionsFrom(created) ?? []);
        for (const model of models) {
          const nativeId = model.nativeId ?? "";
          if (!nativeId) continue;
          latestConfig = null;
          try {
            const result = await acp.request<unknown>(
              "session/set_config_option",
              { sessionId, configId: "model", value: nativeId },
              REQUEST_TIMEOUT_MS,
            );
            const options =
              droidConfigOptionsFrom(result) ??
              (await settledConfig(() => latestConfig));
            const effort = options ? droidEffortConfig(options) : undefined;
            if (effort) efforts.set(nativeId, effort);
          } catch {
            // Keep the model without effort choices rather than drop it.
          }
        }
        if (efforts.size === 0 && initial) {
          const current = models[0]?.nativeId;
          if (current) efforts.set(current, initial);
        }
        publish(modelsFromDroidSession(created, efforts));
      },
      () => {
        void stop();
      },
    );
  } finally {
    await stop();
  }
}

/** The config_option_update notification can trail the empty response. */
async function settledConfig(
  read: () => DroidConfigOption[] | null,
): Promise<DroidConfigOption[] | null> {
  const deadline = Date.now() + CONFIG_SETTLE_MS;
  for (;;) {
    const value = read();
    if (value || Date.now() >= deadline) return value;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

function withTimeout<T>(
  ms: number,
  run: () => Promise<T>,
  onTimeout: () => void,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      onTimeout();
      reject(new Error("Droid model discovery timed out"));
    }, ms);
    void run().then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}
