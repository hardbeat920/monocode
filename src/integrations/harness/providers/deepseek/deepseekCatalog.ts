import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "../../../../platform/tauri/fs";
import {
  setHarnessModels,
  type AgentModel,
} from "../../../../features/sessions/model/models";
import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveDeepseekBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  deepseekSpawnArgs,
  fallbackDeepseekModels,
  modelsFromOfficialApi,
  modelsFromSessionConfig,
  type DeepseekOfficialModelRow,
} from "./deepseekProtocol";

const PROBE_ID = "monocode-deepseek-probe";
const DISCOVERY_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 12_000;

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
};

export type DeepseekCatalogSource = "live" | "acp" | "fallback";

export type DeepseekCatalogSync = {
  status:
    | "idle"
    | "loading"
    | "ok"
    | "unauthenticated"
    | "unauthorized"
    | "error";
  source: DeepseekCatalogSource | null;
  count: number;
  error: string | null;
  modelIds: string[];
};

type OfficialModelsFetch = {
  status: string;
  httpStatus?: number | null;
  models?: DeepseekOfficialModelRow[];
  error?: string | null;
};

type DiscoverResult = {
  models: AgentModel[];
  source: DeepseekCatalogSource;
  sync: DeepseekCatalogSync;
};

let inflight: Promise<void> | null = null;
let lastSync: DeepseekCatalogSync = {
  status: "idle",
  source: null,
  count: 0,
  error: null,
  modelIds: [],
};
const syncListeners = new Set<() => void>();

export function getDeepseekCatalogSync(): DeepseekCatalogSync {
  return lastSync;
}

export function subscribeDeepseekCatalogSync(
  onStoreChange: () => void,
): () => void {
  syncListeners.add(onStoreChange);
  return () => {
    syncListeners.delete(onStoreChange);
  };
}

function setSync(next: DeepseekCatalogSync) {
  lastSync = next;
  for (const listener of syncListeners) listener();
}

export function refreshDeepseekCatalog(): Promise<void> {
  if (inflight) return inflight;
  setSync({ ...lastSync, status: "loading", error: null });
  inflight = discoverDeepseekModelsDetailed()
    .then((result) => {
      setSync(result.sync);
      // Live / ACP overlays replace the catalog. Pure static fallback stays on
      // the bundled MODELS list so a later grant login can still refresh live.
      if (result.source !== "fallback" && result.models.length > 0) {
        setHarnessModels("deepseek", result.models);
      }
    })
    .catch((error: unknown) => {
      console.debug("[monocode] deepseek catalog", error);
      setSync({
        status: "error",
        source: lastSync.source,
        count: lastSync.count,
        error:
          error instanceof Error ? error.message : "DeepSeek 模型目录刷新失败",
        modelIds: lastSync.modelIds,
      });
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export async function discoverDeepseekModels(workingDirectory?: string) {
  return (await discoverDeepseekModelsDetailed(workingDirectory)).models;
}

async function discoverDeepseekModelsDetailed(
  workingDirectory?: string,
): Promise<DiscoverResult> {
  const live = await discoverViaOfficialApi();
  if (live.models.length > 0) {
    // Live fully replaces the static table — never keep both side by side.
    return {
      models: live.models,
      source: "live",
      sync: {
        status: "ok",
        source: "live",
        count: live.models.length,
        error: null,
        modelIds: live.models.map((model) => model.nativeId ?? model.id),
      },
    };
  }

  const fromAcp = await discoverViaAcp(workingDirectory).catch(
    (error: unknown) => {
      console.debug("[monocode] deepseek ACP catalog failed", error);
      return [] as AgentModel[];
    },
  );
  if (fromAcp.length > 0) {
    return {
      models: fromAcp,
      source: "acp",
      sync: {
        status: live.syncStatus === "ok" ? "ok" : live.syncStatus,
        source: "acp",
        count: fromAcp.length,
        error: live.error,
        modelIds: fromAcp.map((model) => model.nativeId ?? model.id),
      },
    };
  }

  const fallback = fallbackDeepseekModels();
  return {
    models: fallback,
    source: "fallback",
    sync: {
      status: live.syncStatus,
      source: "fallback",
      count: fallback.length,
      error: live.error,
      modelIds: fallback.map((model) => model.nativeId ?? model.id),
    },
  };
}

async function discoverViaOfficialApi(): Promise<{
  models: AgentModel[];
  syncStatus: DeepseekCatalogSync["status"];
  error: string | null;
}> {
  try {
    const result = await invoke<OfficialModelsFetch>(
      "fetch_deepseek_official_models",
    );
    const status = normalizeFetchStatus(result.status);
    if (status !== "ok") {
      return {
        models: [],
        syncStatus: status,
        error: result.error?.trim() || statusMessage(status),
      };
    }
    const models = modelsFromOfficialApi(result.models ?? []);
    if (models.length === 0) {
      return {
        models: [],
        syncStatus: "error",
        error: result.error?.trim() || "模型列表为空",
      };
    }
    return { models, syncStatus: "ok", error: null };
  } catch (error: unknown) {
    console.debug("[monocode] deepseek official catalog failed", error);
    return {
      models: [],
      syncStatus: "error",
      error:
        error instanceof Error ? error.message : "模型列表请求失败",
    };
  }
}

function normalizeFetchStatus(
  status: string,
): DeepseekCatalogSync["status"] {
  if (status === "ok") return "ok";
  if (status === "unauthenticated") return "unauthenticated";
  if (status === "unauthorized") return "unauthorized";
  return "error";
}

function statusMessage(status: DeepseekCatalogSync["status"]): string {
  switch (status) {
    case "unauthenticated":
      return "未登录 DeepSeek 账号";
    case "unauthorized":
      return "账号授权无效或已过期";
    case "loading":
      return "正在同步模型…";
    case "ok":
      return "";
    default:
      return "模型列表同步失败";
  }
}


async function discoverViaAcp(workingDirectory?: string) {
  const { path } = await resolveDeepseekBinary();
  const cwd = workingDirectory ?? (await homeDir());
  const probeId = `${PROBE_ID}-${crypto.randomUUID()}`;
  const acp = new AcpClient(probeId, {
    onRequest: (id) => {
      void acp.respond(id, {}).catch(() => undefined);
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
    () => acp.close(new Error("DeepSeek Harness probe exited")),
  );

  try {
    await spawnChild(
      probeId,
      path,
      deepseekSpawnArgs(),
      cwd,
      undefined,
      "deepseek",
    );
    return await withTimeout(
      DISCOVERY_TIMEOUT_MS,
      async () => {
        await acp.request(
          "initialize",
          {
            protocolVersion: 1,
            clientCapabilities: CLIENT_CAPABILITIES,
            clientInfo: { name: "monocode", version: "0.1.0" },
          },
          REQUEST_TIMEOUT_MS,
        );
        const created = await acp.request<{ configOptions?: unknown }>(
          "session/new",
          { cwd, mcpServers: [] },
          REQUEST_TIMEOUT_MS,
        );
        const models = modelsFromSessionConfig(created?.configOptions);
        const sid =
          typeof (created as { sessionId?: string })?.sessionId === "string"
            ? (created as { sessionId: string }).sessionId
            : undefined;
        if (sid) {
          await acp
            .request("session/close", { sessionId: sid }, REQUEST_TIMEOUT_MS)
            .catch(() => undefined);
        }
        return models;
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
      reject(new Error("DeepSeek Harness catalog probe timed out"));
    }, ms);
    run()
      .then(resolve, reject)
      .finally(() => clearTimeout(timer));
  });
}
