import { homeDir } from "../../../../platform/tauri/fs";
import { museSupportsMax } from "./museProtocol";
import {
  setHarnessModels,
  type AgentModel,
  type ModelSettingChoice,
} from "../../../../features/sessions/model/models";
import {
  killChild,
  resolveMuseBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "../../core/child";

const PROBE_ID = "monocode-muse-probe";
const HANDSHAKE_TIMEOUT_MS = 15_000;
const LIST_TIMEOUT_MS = 20_000;

let inflight: Promise<void> | null = null;

/**
 * Refresh the Muse catalog from the CLI's own `model/list` over a throwaway
 * `muse serve` probe, mirroring the cursor ACP probe. The static MODELS
 * entries stay as the offline fallback; a successful probe replaces them via
 * `setHarnessModels` until the next refresh.
 */
export function refreshMuseCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverMuseModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("muse", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] muse catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

type JsonRpcResponse = {
  id?: number;
  result?: unknown;
  error?: { message?: string };
};

async function discoverMuseModels(): Promise<AgentModel[]> {
  const { path } = await resolveMuseBinary();
  const cwd = await homeDir();
  let nextId = 1;
  const pending = new Map<number, (response: JsonRpcResponse) => void>();
  let failed: Error | undefined;

  const stop = async () => {
    pending.clear();
    unwatchChild(PROBE_ID);
    await killChild(PROBE_ID).catch(() => undefined);
  };

  const fail = (error: Error) => {
    failed ??= error;
    for (const resolve of pending.values()) {
      resolve({ error: { message: error.message } });
    }
    pending.clear();
  };

  watchChild(
    PROBE_ID,
    (line) => {
      let message: JsonRpcResponse;
      try {
        message = JSON.parse(line) as JsonRpcResponse;
      } catch {
        return;
      }
      if (typeof message.id !== "number") return;
      pending.get(message.id)?.(message);
      pending.delete(message.id);
    },
    (code) => {
      fail(new Error(`Muse catalog probe exited (code ${code ?? "unknown"})`));
    },
  );

  const request = async (method: string, params: Record<string, unknown>, timeoutMs: number) => {
    const id = nextId++;
    let answer!: (response: JsonRpcResponse) => void;
    const response = new Promise<JsonRpcResponse>((resolve) => {
      answer = resolve;
      pending.set(id, resolve);
    });
    const timer = setTimeout(() => {
      pending.delete(id);
      answer({ error: { message: `${method} timed out` } });
    }, timeoutMs);
    try {
      await writeChild(PROBE_ID, JSON.stringify({ jsonrpc: "2.0", id, method, params }));
      const reply = await response;
      if (reply.error) throw new Error(`${method} failed: ${reply.error.message ?? "unknown"}`);
      return reply.result;
    } finally {
      clearTimeout(timer);
    }
  };

  const notify = (method: string) =>
    writeChild(PROBE_ID, JSON.stringify({ jsonrpc: "2.0", method }));

  try {
    await spawnChild(PROBE_ID, path, ["serve", "--no-session-log"], cwd);
    await request(
      "initialize",
      { clientInfo: { name: "monocode", version: "0.1.0" } },
      HANDSHAKE_TIMEOUT_MS,
    );
    await notify("initialized");
    const listed = await request("model/list", {}, LIST_TIMEOUT_MS);
    return modelsFromList(listed);
  } catch (error) {
    throw failed ?? error;
  } finally {
    await stop();
  }
}

type CatalogRow = {
  model_id?: unknown;
  modelId?: unknown;
  display_label?: unknown;
  displayLabel?: unknown;
  visibility?: unknown;
  context_limit?: unknown;
  contextLimit?: unknown;
  is_default?: unknown;
  isDefault?: unknown;
  reasoning_effort_variants?: unknown;
  reasoningEffortVariants?: unknown;
};

function stringField(camel: unknown, snake: unknown): string {
  if (typeof camel === "string" && camel.trim()) return camel.trim();
  if (typeof snake === "string" && snake.trim()) return snake.trim();
  return "";
}

function rowModelId(row: CatalogRow): string {
  return stringField(row.modelId, row.model_id);
}

function rowLabel(row: CatalogRow): string {
  return stringField(row.displayLabel, row.display_label);
}

function rowContextLimit(row: CatalogRow): number | undefined {
  for (const value of [row.contextLimit, row.context_limit]) {
    if (typeof value === "number" && Number.isFinite(value) && value > 0) return value;
  }
  return undefined;
}

function rowIsDefault(row: CatalogRow): boolean {
  return row.isDefault === true || row.is_default === true;
}

const EFFORT_LABELS: Record<string, string> = {
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  max: "Max",
};

/** Documented tiers only: `none` 400s on Spark and `ultra` is undocumented,
 * so neither is offered even when the CLI advertises them. `max` is
 * standard `muse-spark-1.3` only; 1.2 and contributor ids omit it. */
export function effortChoicesForRow(row: CatalogRow): ModelSettingChoice[] {
  const modelId = rowModelId(row);
  const variants = Array.isArray(row.reasoningEffortVariants)
    ? row.reasoningEffortVariants
    : Array.isArray(row.reasoning_effort_variants)
      ? row.reasoning_effort_variants
      : [];
  const advertised = new Set(
    variants.flatMap((variant) => {
      const tier =
        typeof variant === "object" && variant !== null
          ? (variant as Record<string, unknown>).tier
          : undefined;
      return typeof tier === "string" ? [tier] : [];
    }),
  );
  // Live `model/list` carries no variants; fall back to the documented
  // per-tier list so fresh model ids still get a correct select.
  const tiers = advertised.size > 0 ? advertised : new Set(defaultTiersFor(modelId));
  return [...EFFORTS_IN_ORDER(tiers)]
    .filter((tier) => tier !== "max" || museSupportsMax(modelId))
    .map((tier) => ({ value: tier, label: EFFORT_LABELS[tier] ?? tier }));
}

function defaultTiersFor(modelId: string): string[] {
  const tiers = ["minimal", "low", "medium", "high", "xhigh"];
  if (museSupportsMax(modelId)) tiers.push("max");
  return tiers;
}

function EFFORTS_IN_ORDER(tiers: Set<string>): string[] {
  return ["minimal", "low", "medium", "high", "xhigh", "max"].filter((tier) =>
    tiers.has(tier),
  );
}

export function displayNameForRow(row: CatalogRow): string {
  const modelId = rowModelId(row);
  const label = rowLabel(row);
  if (label && label !== modelId) return label;
  const match = /^muse-spark-(.+?)(-contributor)?$/.exec(modelId);
  if (match) {
    return `Muse Spark ${match[1]}${match[2] ? " (Contributor)" : ""}`;
  }
  return label || modelId;
}

export function modelsFromList(listed: unknown): AgentModel[] {
  const root =
    typeof listed === "object" && listed !== null
      ? (listed as Record<string, unknown>)
      : undefined;
  const rows = Array.isArray(root?.models) ? root.models : [];
  const models: Array<{ model: AgentModel; isDefault: boolean; index: number }> = [];
  for (const entry of rows) {
    if (typeof entry !== "object" || entry === null) continue;
    const row = entry as CatalogRow;
    const modelId = rowModelId(row);
    if (!modelId) continue;
    if (row.visibility === "hidden") continue;
    const choices = effortChoicesForRow(row);
    models.push({
      isDefault: rowIsDefault(row),
      index: models.length,
      model: {
        id: `muse:${modelId}`,
        harness: "muse",
        name: displayNameForRow(row),
        nativeId: modelId,
        contextWindow: rowContextLimit(row),
        settings:
          choices.length > 0
            ? [
                {
                  id: "effort",
                  label: "Reasoning",
                  kind: "select",
                  value: choices.some((choice) => choice.value === "high")
                    ? "high"
                    : choices[0].value,
                  options: choices,
                },
              ]
            : undefined,
      },
    });
  }
  models.sort((a, b) => {
    if (a.isDefault !== b.isDefault) return a.isDefault ? -1 : 1;
    return a.index - b.index;
  });
  return models.map((entry) => entry.model);
}
