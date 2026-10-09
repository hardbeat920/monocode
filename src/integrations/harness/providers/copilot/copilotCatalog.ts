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
  const acp = new JsonRpcClient(probeId, {
    onNotification: (method, params) => {
      if (method === "session/update") {
        commands = commandsFromCopilotUpdate(params) ?? commands;
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
