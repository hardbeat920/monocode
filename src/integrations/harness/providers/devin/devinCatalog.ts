import { homeDir } from "../../../../platform/tauri/fs";
import {
  setHarnessModels,
  type AgentModel,
} from "../../../../features/sessions/model/models";
import { killChild, unwatchChild, watchChild } from "../../core/child";
import { JsonRpcClient } from "../../core/jsonRpc";
import { sessionIdFromResult } from "../antigravity/antigravityProtocol";
import { startDevinAcp } from "./devin";
import {
  devinModelChoices,
  devinThoughtLevel,
  modelsFromDevinSession,
  type DevinThoughtLevel,
  type DevinThoughtLevels,
} from "./devinProtocol";

const PROBE_ID = "monocode-devin-probe";
const DISCOVERY_TIMEOUT_MS = 60_000;
const REQUEST_TIMEOUT_MS = 30_000;
const THOUGHT_PROBE_BUDGET_MS = 20_000;
const MODEL_PROBE_TIMEOUT_MS = 5_000;

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

/**
 * Devin's efforts differ per model and a session only shows the current
 * model's, so switch the throwaway probe session through each model (local
 * calls; about 5 s for 60 models). Switching does not touch the user's saved
 * default. Models missed within the budget fall back to the session's levels.
 */
async function probeThoughtLevels(
  rpc: JsonRpcClient,
  sessionId: string,
  created: unknown,
): Promise<DevinThoughtLevels | undefined> {
  if (!devinThoughtLevel(created)) return undefined;
  const levels = new Map<string, DevinThoughtLevel | null>();
  const deadline = Date.now() + THOUGHT_PROBE_BUDGET_MS;
  for (const { value } of devinModelChoices(created)) {
    // Fusion carries its lead's effort in the id.
    if (value.startsWith("fusion-") || levels.has(value)) continue;
    if (Date.now() > deadline) break;
    try {
      const result = await rpc.request<unknown>(
        "session/set_config_option",
        { sessionId, configId: "model", value },
        MODEL_PROBE_TIMEOUT_MS,
      );
      levels.set(value, devinThoughtLevel(result));
    } catch (error) {
      // A stalled child would spend the whole discovery budget; stop here.
      if (/timed out/i.test(String(error))) break;
      // Unknown for this model: the session-wide levels stand in.
    }
  }
  return levels;
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
        if (!sessionId) return modelsFromDevinSession(created);
        try {
          const levels = await probeThoughtLevels(rpc, sessionId, created);
          return modelsFromDevinSession(created, levels);
        } finally {
          await rpc
            .request("session/delete", { sessionId }, REQUEST_TIMEOUT_MS)
            .catch(() => undefined);
        }
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
