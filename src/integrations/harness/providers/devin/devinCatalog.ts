import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels } from "../../../../features/sessions/model/models";
import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveDevinBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  DEVIN_CLIENT_CAPABILITIES,
  DEVIN_CLIENT_INFO,
  modelsFromDevinSession,
} from "./devinProtocol";

const PROBE_ID = "monocode-devin-probe";
const REQUEST_TIMEOUT_MS = 20_000;
let inflight: Promise<void> | null = null;

export function refreshDevinCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverModels()
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

async function discoverModels() {
  const [{ path }, cwd] = await Promise.all([
    resolveDevinBinary(),
    homeDir(),
  ]);
  const acp = new AcpClient(PROBE_ID, {
    onRequest: (id, method) => {
      const response =
        method === "_cognition.ai/request_diagnostics"
          ? acp.respond(id, {})
          : method === "session/request_permission"
            ? acp.respond(id, { outcome: { outcome: "cancelled" } })
            : acp.respondError(id, {
                code: -32601,
                message: `Method not found: ${method}`,
              });
      void response.catch(() => undefined);
    },
  });

  watchChild(
    PROBE_ID,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Devin catalog probe exited")),
  );

  try {
    await spawnChild(PROBE_ID, path, ["acp"], cwd);
    await acp.request(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: DEVIN_CLIENT_CAPABILITIES,
        clientInfo: DEVIN_CLIENT_INFO,
      },
      REQUEST_TIMEOUT_MS,
    );
    const created = await acp.request(
      "session/new",
      { cwd, mcpServers: [] },
      REQUEST_TIMEOUT_MS,
    );
    return modelsFromDevinSession(created);
  } finally {
    acp.close();
    unwatchChild(PROBE_ID);
    await killChild(PROBE_ID).catch(() => undefined);
  }
}
