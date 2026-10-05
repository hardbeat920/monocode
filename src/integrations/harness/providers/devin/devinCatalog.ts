import { homeDir } from "../../../../platform/tauri/fs";
import {
  setHarnessModels,
  type AgentModel,
} from "../../../../features/sessions/model/models";
import { killChild, unwatchChild, watchChild } from "../../core/child";
import { JsonRpcClient } from "../../core/jsonRpc";
import { sessionIdFromResult } from "../antigravity/antigravityProtocol";
import { startDevinAcp } from "./devin";
import { modelsFromDevinSession } from "./devinProtocol";

const PROBE_ID = "monocode-devin-probe";
const DISCOVERY_TIMEOUT_MS = 60_000;
const REQUEST_TIMEOUT_MS = 30_000;

let inflight: Promise<void> | null = null;

export function refreshDevinCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverDevinModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("devin", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] devin catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export async function discoverDevinModels(
  workingDirectory?: string,
): Promise<AgentModel[]> {
  const cwd = workingDirectory ?? (await homeDir());
  const probeId = `${PROBE_ID}-${crypto.randomUUID()}`;
  const rpc = new JsonRpcClient(
    probeId,
    {
      onRequest: (id, method) => {
        void rpc
          .respondError(id, { code: -32601, message: `Method not found: ${method}` })
          .catch(() => undefined);
      },
    },
    { includeJsonrpc: true, label: "devin" },
  );
  const stop = async () => {
    rpc.close();
    unwatchChild(probeId);
    await killChild(probeId).catch(() => undefined);
  };
  watchChild(
    probeId,
    (line) => rpc.pushLine(line),
    () => rpc.close(new Error("Devin catalog probe exited")),
  );

  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      (async () => {
        await startDevinAcp(probeId, cwd, rpc, { allowBrowser: false });
        const created = await rpc.request<unknown>(
          "session/new",
          { cwd, mcpServers: [] },
          REQUEST_TIMEOUT_MS,
        );
        // The probe session is throwaway; keep it out of `devin list`.
        const sessionId = sessionIdFromResult(created);
        if (sessionId) {
          await rpc
            .request("session/delete", { sessionId }, REQUEST_TIMEOUT_MS)
            .catch(() => undefined);
        }
        return modelsFromDevinSession(created);
      })(),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Devin model discovery timed out")),
          DISCOVERY_TIMEOUT_MS,
        );
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
    await stop();
  }
}
