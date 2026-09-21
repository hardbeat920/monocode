import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels } from "../../../../features/sessions/model/models";
import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveAntigravityBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { resolveHarnessBinary } from "../../core/runtime";
import { antigravitySpawnCwd, modelsFromSessionNew } from "./antigravityProtocol";

const PROBE_ID = "monocode-antigravity-probe";
const REQUEST_TIMEOUT_MS = 12_000;
let inflight: Promise<void> | null = null;

export function refreshAntigravityCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("antigravity", models);
    })
    .catch((error: unknown) => {
      // Preserve the last live catalog (or startup seeds) when offline/logged out.
      console.debug("[monocode] antigravity catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverModels() {
  const resolved = await resolveHarnessBinary(
    "antigravity",
    resolveAntigravityBinary,
  );
  const { path } = resolved;
  const args = resolved.args ?? [];
  const cwd = await homeDir();
  const acp = new AcpClient(PROBE_ID, {
    onRequest: (id, method) => {
      const response = method === "session/request_permission"
        ? acp.respond(id, { outcome: { outcome: "cancelled" } })
        : acp.respondError(id, { code: -32601, message: `Method not found: ${method}` });
      void response.catch(() => undefined);
    },
  });
  watchChild(
    PROBE_ID,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Antigravity probe exited")),
  );
  try {
    await spawnChild(PROBE_ID, path, args, antigravitySpawnCwd(path, cwd));
    await acp.request(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: {
          fs: { readTextFile: false, writeTextFile: false },
          terminal: false,
          // Without this claim a compliant agent may omit configOptions from
          // session/new, leaving the catalog with nothing to enumerate.
          session: { configOptions: { boolean: {} } },
        },
        clientInfo: { name: "monocode", version: "0.1.0" },
      },
      REQUEST_TIMEOUT_MS,
    );
    // Authentication stays in the provider's Terminal UI; never start OAuth here.
    const created = await acp.request(
      "session/new",
      { cwd, mcpServers: [] },
      REQUEST_TIMEOUT_MS,
    );
    return modelsFromSessionNew(created);
  } finally {
    acp.close();
    unwatchChild(PROBE_ID);
    await killChild(PROBE_ID).catch(() => undefined);
  }
}
