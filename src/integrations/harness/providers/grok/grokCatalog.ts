import { homeDir } from "../../../../platform/tauri/fs";
import { setHarnessModels } from "../../../../features/sessions/model/models";
import { AcpClient } from "../../core/acp";
import {
  execChild,
  killChild,
  resolveGrokBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { resolveHarnessBinary } from "../../core/runtime";
import {
  fallbackGrokModels,
  grokAuthMethodId,
  grokSpawnArgs,
  modelsFromGrokModelsOutput,
  modelsFromInitialize,
  modelsFromSessionNew,
} from "./grokProtocol";

const PROBE_ID = "monocode-grok-probe";
const DISCOVERY_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 12_000;

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

let inflight: Promise<void> | null = null;

export function refreshGrokCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverGrokModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("grok", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] grok catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverGrokModels() {
  const fromAcp = await discoverViaAcp().catch((error: unknown) => {
    console.debug("[monocode] grok ACP catalog failed", error);
    return [];
  });
  if (fromAcp.length > 0) return fromAcp;
  const fromCli = await discoverViaCli().catch((error: unknown) => {
    console.debug("[monocode] grok CLI catalog failed", error);
    return [];
  });
  if (fromCli.length > 0) return fromCli;
  return fallbackGrokModels();
}

async function discoverViaAcp() {
  const { path } = await resolveHarnessBinary("grok", resolveGrokBinary);
  const cwd = await homeDir();
  const acp = new AcpClient(PROBE_ID, {
    onRequest: (id) => {
      void acp.respond(id, {}).catch(() => undefined);
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
    () => acp.close(new Error("Grok Build probe exited")),
  );

  try {
    await spawnChild(PROBE_ID, path, grokSpawnArgs({ model: "" }), cwd);
    return await withTimeout(DISCOVERY_TIMEOUT_MS, async () => {
      const init = await acp.request(
        "initialize",
        {
          protocolVersion: 1,
          clientCapabilities: CLIENT_CAPABILITIES,
          clientInfo: { name: "monocode", version: "0.1.0" },
        },
        REQUEST_TIMEOUT_MS,
      );
      const fromInit = modelsFromInitialize(init);
      if (fromInit.length > 0) return fromInit;

      const methodId = grokAuthMethodId(init);
      if (methodId) {
        await acp
          .request(
            "authenticate",
            { methodId, _meta: { headless: true } },
            REQUEST_TIMEOUT_MS,
          )
          .catch(() => undefined);
      }
      const created = await acp.request(
        "session/new",
        { cwd, mcpServers: [] },
        REQUEST_TIMEOUT_MS,
      );
      return modelsFromSessionNew(created);
    }, () => {
      void stop();
    });
  } finally {
    await stop();
  }
}

async function discoverViaCli() {
  const { path } = await resolveHarnessBinary("grok", resolveGrokBinary);
  const cwd = await homeDir();
  const stdout = await execChild(path, ["models"], cwd);
  return modelsFromGrokModelsOutput(stdout);
}

function withTimeout<T>(
  ms: number,
  run: () => Promise<T>,
  onTimeout: () => void,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      onTimeout();
      reject(new Error("Grok Build catalog probe timed out"));
    }, ms);
    run()
      .then(resolve, reject)
      .finally(() => clearTimeout(timer));
  });
}
