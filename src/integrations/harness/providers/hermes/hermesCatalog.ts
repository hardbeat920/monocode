import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels, type AgentModel } from "../../../../features/sessions/model/models";
import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveHermesBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { modelsFromHermesSession } from "./hermesProtocol";

const PROBE_ID = "monocode-hermes-probe";
const DISCOVERY_TIMEOUT_MS = 30_000;
const REQUEST_TIMEOUT_MS = 20_000;

let inflight: Promise<void> | null = null;

export function refreshHermesCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverHermesModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("hermes", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] hermes catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverHermesModels(): Promise<AgentModel[]> {
  const { path } = await resolveHermesBinary();
  const cwd = await homeDir();
  const acp = new AcpClient(PROBE_ID, {
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
    unwatchChild(PROBE_ID);
    await killChild(PROBE_ID).catch(() => undefined);
  };

  watchChild(
    PROBE_ID,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Hermes catalog probe exited")),
  );

  try {
    await spawnChild(PROBE_ID, path, ["acp"], cwd);
    return await withTimeout(
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
        return modelsFromHermesSession(created);
      },
      () => {
        void stop();
      },
    );
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
      reject(new Error("Hermes model discovery timed out"));
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
