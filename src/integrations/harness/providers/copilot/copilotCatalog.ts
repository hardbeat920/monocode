import { homeDir } from "../../../../platform/tauri/fs";
import {
  setHarnessModels,
  type AgentModel,
} from "../../../../features/sessions/model/models";
import { JsonRpcClient } from "../../core/jsonRpc";
import type { NativeCommand } from "../../core/nativeCommands";
import {
  killChild,
  resolveCopilotBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  modelsFromCopilotSession,
  commandsFromCopilotUpdate,
  copilotSpawnArgs,
  copilotError,
  COPILOT_INITIALIZE_PARAMS,
} from "./copilotProtocol";

const PROBE_ID = "monocode-copilot-probe";
const DISCOVERY_TIMEOUT_MS = 30_000;
const REQUEST_TIMEOUT_MS = 20_000;
const COMMANDS_WAIT_MS = 500;

let inflight: Promise<void> | null = null;

export function refreshCopilotCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverCopilotModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("copilot", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] copilot catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export async function discoverCopilotModels(
  workingDirectory?: string,
): Promise<AgentModel[]> {
  return (await probeCopilot(workingDirectory)).models;
}

export async function discoverCopilotCommands(
  cwd: string,
): Promise<NativeCommand[]> {
  return (await probeCopilot(cwd)).commands;
}

async function probeCopilot(
  workingDirectory?: string,
): Promise<{ models: AgentModel[]; commands: NativeCommand[] }> {
  const { path } = await resolveCopilotBinary();
  const cwd = workingDirectory ?? (await homeDir());
  const probeId = `${PROBE_ID}-${crypto.randomUUID()}`;
  let commands: NativeCommand[] = [];
  let resolveCommands!: () => void;
  const commandsReceived = new Promise<void>((resolve) => {
    resolveCommands = resolve;
  });
  const acp = new JsonRpcClient(probeId, {
    onNotification: (method, params) => {
      if (method === "session/update") {
        const update = commandsFromCopilotUpdate(params);
        if (update !== undefined) {
          commands = update;
          resolveCommands();
        }
      }
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

  let stopped = false;
  const stop = async () => {
    if (stopped) return;
    stopped = true;
    resolveCommands();
    acp.close();
    unwatchChild(probeId);
    await killChild(probeId).catch(() => undefined);
  };

  watchChild(
    probeId,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Copilot catalog probe exited")),
  );

  try {
    await spawnChild(
      probeId,
      path,
      copilotSpawnArgs(),
      cwd,
      undefined,
      "copilot",
    );
    return await withTimeout(
      DISCOVERY_TIMEOUT_MS,
      async () => {
        await acp.request(
          "initialize",
          COPILOT_INITIALIZE_PARAMS,
          REQUEST_TIMEOUT_MS,
        );
        const created = await acp.request<unknown>(
          "session/new",
          { cwd, mcpServers: [] },
          REQUEST_TIMEOUT_MS,
        );
        // Command metadata can follow session/new's response. Allow a brief
        // grace period, including for CLIs that never advertise commands.
        await new Promise<void>((resolve) => {
          const timer = setTimeout(resolve, COMMANDS_WAIT_MS);
          void commandsReceived.then(() => {
            clearTimeout(timer);
            resolve();
          });
        });
        return { models: modelsFromCopilotSession(created), commands };
      },
      () => {
        void stop();
      },
    );
  } catch (error) {
    throw copilotError(error);
  } finally {
    await stop();
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
      reject(new Error("Copilot model discovery timed out"));
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
