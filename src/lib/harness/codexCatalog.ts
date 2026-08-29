import { homeDir } from "../fs";
import {
  setHarnessModels,
  type AgentModel,
  type ModelSetting,
  type ModelSettingChoice,
} from "../models";
import {
  killChild,
  resolveCodexBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import { asRecord, stringField } from "./codexProtocol";
import { JsonRpcClient } from "./jsonRpc";

const PROBE_ID = "monocode-codex-probe";
const DISCOVERY_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 12_000;

const REASONING_LABELS: Record<string, string> = {
  none: "None",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  max: "Max",
  ultra: "Ultra",
};

let inflight: Promise<void> | null = null;

export function refreshCodexCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverCodexModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("codex", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] codex catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverCodexModels(): Promise<AgentModel[]> {
  const { path } = await resolveCodexBinary();
  const cwd = await homeDir();
  const rpc = new JsonRpcClient(
    PROBE_ID,
    {
      onRequest: (id) => {
        void rpc.respond(id, {}).catch(() => undefined);
      },
    },
    { includeJsonrpc: false, label: "codex-probe" },
  );

  const stop = async () => {
    rpc.close();
    unwatchChild(PROBE_ID);
    await killChild(PROBE_ID).catch(() => undefined);
  };

  watchChild(
    PROBE_ID,
    (line) => rpc.pushLine(line),
    () => rpc.close(new Error("Codex probe exited")),
  );

  try {
    await spawnChild(PROBE_ID, path, ["app-server"], cwd);
    return await withTimeout(DISCOVERY_TIMEOUT_MS, async () => {
      await rpc.request(
        "initialize",
        {
          clientInfo: {
            name: "monocode",
            title: "MonoCode",
            version: "0.1.0",
          },
          capabilities: { experimentalApi: true },
        },
        REQUEST_TIMEOUT_MS,
      );
      await rpc.notify("initialized", undefined);

      const account = await rpc
        .request<{
          account?: unknown;
          requiresOpenaiAuth?: boolean;
        }>("account/read", {}, REQUEST_TIMEOUT_MS)
        .catch(() => null);

      if (account && !account.account && account.requiresOpenaiAuth) {
        throw new Error(
          "Codex CLI is not authenticated. Run `codex login` and try again.",
        );
      }

      return await listAllModels(rpc);
    }, () => {
      void stop();
    });
  } finally {
    await stop();
  }
}

async function listAllModels(rpc: JsonRpcClient): Promise<AgentModel[]> {
  const models: AgentModel[] = [];
  const rows: unknown[] = [];
  let cursor: string | null | undefined;
  do {
    const response = await rpc.request<{
      data?: unknown[];
      nextCursor?: string | null;
    }>("model/list", cursor ? { cursor } : {}, REQUEST_TIMEOUT_MS);
    const page = Array.isArray(response.data) ? response.data : [];
    rows.push(...page);
    for (const row of page) {
      const model = parseModel(row);
      if (model) models.push(model);
    }
    cursor = response.nextCursor ?? null;
  } while (cursor);

  return orderDefaultFirst(uniqueByNative(models), rows);
}

export function parseCodexModelList(data: unknown[]): AgentModel[] {
  return orderDefaultFirst(
    uniqueByNative(data.flatMap((row) => {
      const model = parseModel(row);
      return model ? [model] : [];
    })),
    data,
  );
}

function parseModel(raw: unknown): AgentModel | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  if (rec.hidden === true) return null;
  const nativeId =
    stringField(rec, "model") ??
    stringField(rec, "slug") ??
    stringField(rec, "id");
  if (!nativeId) return null;
  const name = formatDisplayName(
    stringField(rec, "displayName") ??
      stringField(rec, "name") ??
      nativeId,
  );
  const settings = parseModelSettings(rec);
  return {
    id: `codex:${nativeId}`,
    harness: "codex",
    name,
    nativeId,
    ...(settings.length > 0 ? { settings } : {}),
  };
}

function parseModelSettings(rec: Record<string, unknown>): ModelSetting[] {
  const settings: ModelSetting[] = [];
  const efforts = Array.isArray(rec.supportedReasoningEfforts)
    ? rec.supportedReasoningEfforts
    : [];
  const effortOptions: ModelSettingChoice[] = [];
  let defaultEffort: string | undefined;
  for (const entry of efforts) {
    if (typeof entry === "string") {
      effortOptions.push({
        value: entry,
        label: REASONING_LABELS[entry] ?? entry,
      });
      continue;
    }
    const row = asRecord(entry);
    const value =
      stringField(row, "reasoningEffort") ?? stringField(row, "id");
    if (!value) continue;
    effortOptions.push({
      value,
      label: REASONING_LABELS[value] ?? stringField(row, "label") ?? value,
    });
  }
  const defaultRaw = stringField(rec, "defaultReasoningEffort");
  if (defaultRaw) defaultEffort = defaultRaw;
  if (effortOptions.length > 0) {
    settings.push({
      id: "reasoningEffort",
      label: "Reasoning",
      kind: "select",
      value: defaultEffort ?? effortOptions[0].value,
      options: effortOptions,
    });
  }

  const tiersRaw =
    (Array.isArray(rec.serviceTiers) && rec.serviceTiers.length > 0
      ? rec.serviceTiers
      : Array.isArray(rec.additionalSpeedTiers)
        ? rec.additionalSpeedTiers
        : []) ?? [];
  const tierOptions: ModelSettingChoice[] = [
    { value: "default", label: "Standard" },
  ];
  for (const entry of tiersRaw) {
    if (typeof entry === "string") {
      if (entry === "default") continue;
      tierOptions.push({
        value: entry,
        label: entry === "fast" ? "Fast" : entry,
      });
      continue;
    }
    const row = asRecord(entry);
    const id = stringField(row, "id");
    if (!id || id === "default") continue;
    tierOptions.push({
      value: id,
      label: stringField(row, "name") ?? id,
    });
  }
  if (tierOptions.length > 1) {
    const defaultTier =
      stringField(rec, "defaultServiceTier") ?? "default";
    settings.push({
      id: "serviceTier",
      label: "Service Tier",
      kind: "select",
      value: tierOptions.some((o) => o.value === defaultTier)
        ? defaultTier
        : "default",
      options: tierOptions,
    });
  }

  return settings;
}

function formatDisplayName(name: string): string {
  return name
    .replace(/^gpt/i, "GPT")
    .replace(/-([a-z])/g, (_, c: string) => `-${c.toUpperCase()}`);
}

function orderDefaultFirst(
  models: AgentModel[],
  rows: unknown[] = [],
): AgentModel[] {
  if (models.length <= 1) return models;
  const defaultNative = rows
    .map((row) => asRecord(row))
    .find((rec) => rec?.isDefault === true);
  const nativeId =
    stringField(defaultNative, "model") ??
    stringField(defaultNative, "slug") ??
    stringField(defaultNative, "id");
  if (!nativeId) return models;
  const index = models.findIndex((model) => model.nativeId === nativeId);
  if (index <= 0) return models;
  return [models[index], ...models.slice(0, index), ...models.slice(index + 1)];
}

function uniqueByNative(models: AgentModel[]): AgentModel[] {
  const seen = new Set<string>();
  return models.filter((model) => {
    if (!model.nativeId || seen.has(model.nativeId)) return false;
    seen.add(model.nativeId);
    return true;
  });
}

async function withTimeout<T>(
  ms: number,
  work: () => Promise<T>,
  onTimeout: () => void,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const pending = work();
  try {
    return await Promise.race([
      pending,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => {
          onTimeout();
          reject(new Error("Codex model discovery timed out"));
        }, ms);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
    void pending.catch(() => undefined);
  }
}
