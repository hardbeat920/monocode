import { homeDir } from "../fs";
import {
  setHarnessModels,
  type AgentModel,
  type ModelSetting,
  type ModelSettingChoice,
} from "../models";
import { AcpClient } from "./acp";
import {
  execChild,
  killChild,
  resolveCursorBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";

const PROBE_ID = "monocode-cursor-probe";
const DISCOVERY_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 12_000;

const CURSOR_CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
  _meta: { parameterizedModelPicker: true },
};

let inflight: Promise<void> | null = null;

export function refreshCursorCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverCursorModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("cursor", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] cursor catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverCursorModels(): Promise<AgentModel[]> {
  const fromAcp = await discoverViaAcp().catch((error: unknown) => {
    console.debug("[monocode] cursor ACP catalog failed", error);
    return [];
  });
  if (fromAcp.length > 0) return fromAcp;
  return discoverViaCli().catch((error: unknown) => {
    console.debug("[monocode] cursor CLI catalog failed", error);
    return [];
  });
}

async function discoverViaAcp(): Promise<AgentModel[]> {
  const { path } = await resolveCursorBinary();
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
    () => acp.close(new Error("Cursor probe exited")),
  );

  try {
    await spawnChild(PROBE_ID, path, ["acp"], cwd);
    return await withTimeout(DISCOVERY_TIMEOUT_MS, async () => {
      await acp.request(
        "initialize",
        {
          protocolVersion: 1,
          clientCapabilities: CURSOR_CLIENT_CAPABILITIES,
          clientInfo: { name: "monocode", version: "0.1.0" },
        },
        REQUEST_TIMEOUT_MS,
      );
      await acp
        .request("authenticate", { methodId: "cursor_login" }, REQUEST_TIMEOUT_MS)
        .catch(() => undefined);
      const listed = await acp.request<unknown>(
        "cursor/list_available_models",
        {},
        REQUEST_TIMEOUT_MS,
      );
      const models = modelsFromListAvailable(listed);
      if (models.length > 0) return models;

      const created = await acp.request<unknown>(
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

async function discoverViaCli(): Promise<AgentModel[]> {
  const { path } = await resolveCursorBinary();
  const cwd = await homeDir();
  const stdout = await execChild(path, ["--list-models"], cwd);
  return modelsFromListModelsOutput(stdout);
}

function modelsFromListAvailable(result: unknown): AgentModel[] {
  const models = asRecord(result)?.models;
  if (!Array.isArray(models)) return [];
  return uniqueCursorModels(
    models.flatMap((item) => {
      const rec = asRecord(item);
      if (!rec) return [];
      const nativeId = String(rec.value ?? rec.modelId ?? rec.id ?? "").trim();
      const name = String(rec.name ?? nativeId).trim();
      if (!nativeId || !name) return [];
      return [
        {
          id: `cursor:${nativeId}`,
          harness: "cursor" as const,
          name,
          nativeId,
          settings: parseConfigOptions(rec.configOptions),
        },
      ];
    }),
  );
}

function modelsFromSessionNew(result: unknown): AgentModel[] {
  const rec = asRecord(result);
  const available = asRecord(rec?.models)?.availableModels;
  if (Array.isArray(available) && available.length > 0) {
    return uniqueCursorModels(
      available.flatMap((item) => {
        const model = asRecord(item);
        if (!model) return [];
        const nativeId = String(model.modelId ?? model.value ?? "").trim();
        const name = String(model.name ?? nativeId).trim();
        if (!nativeId || !name) return [];
        return [
          {
            id: `cursor:${nativeId}`,
            harness: "cursor" as const,
            name,
            nativeId,
          },
        ];
      }),
    );
  }

  const options = Array.isArray(rec?.configOptions) ? rec.configOptions : [];
  for (const option of options) {
    const config = asRecord(option);
    const id = String(config?.id ?? "").toLowerCase();
    const category = String(config?.category ?? "").toLowerCase();
    if (id !== "model" && category !== "model") continue;
    return uniqueCursorModels(
      flattenSelectOptions(config?.options).map((entry) => ({
        id: `cursor:${entry.value}`,
        harness: "cursor" as const,
        name: entry.label || entry.value,
        nativeId: entry.value,
      })),
    );
  }
  return [];
}

function modelsFromListModelsOutput(stdout: string): AgentModel[] {
  const rows: Array<{ id: string; name: string }> = [];
  for (const raw of stdout.split(/\r?\n/)) {
    const line = raw.replace(/\x1b\[[0-9;]*[A-Za-z]/g, "").trim();
    const match = /^(\S+)\s+-\s+(.+)$/.exec(line);
    if (!match) continue;
    const id = match[1];
    const name = match[2].replace(/\s*\(default\)\s*$/i, "").trim();
    if (!id || !name) continue;
    rows.push({ id, name });
  }
  return groupCliModels(rows);
}

const EFFORT_TOKENS = [
  "extra-high",
  "xhigh",
  "minimal",
  "medium",
  "none",
  "low",
  "high",
  "max",
];

function groupCliModels(
  rows: Array<{ id: string; name: string }>,
): AgentModel[] {
  type Flags = { effort?: string; fast: boolean; thinking: boolean };
  const families = new Map<
    string,
    {
      name: string;
      variants: Array<Flags & { id: string; name: string }>;
    }
  >();

  for (const row of rows) {
    const parsed = parseCliVariant(row.id);
    const family = families.get(parsed.base) ?? {
      name: row.name,
      variants: [],
    };
    if (row.id === parsed.base || family.variants.length === 0) {
      family.name = stripVariantWords(row.name);
    }
    family.variants.push({
      id: row.id,
      name: row.name,
      effort: parsed.effort,
      fast: parsed.fast,
      thinking: parsed.thinking,
    });
    families.set(parsed.base, family);
  }

  return [...families.entries()].map(([base, family]) => {
    const efforts = unique(
      family.variants
        .map((variant) => variant.effort)
        .filter((value): value is string => Boolean(value)),
    );
    const hasFast = family.variants.some((variant) => variant.fast);
    const hasThinking = family.variants.some((variant) => variant.thinking);
    const canonical =
      family.variants.find((variant) => variant.id === base) ??
      family.variants[0];
    const settings: ModelSetting[] = [];
    if (efforts.length > 1) {
      settings.push({
        id: effortKeyFor(base),
        label: "Effort",
        kind: "select",
        value: canonical?.effort ?? efforts[0],
        options: efforts.map((value) => ({
          value,
          label: effortLabel(value),
        })),
      });
    }
    if (hasThinking) {
      settings.push({
        id: "thinking",
        label: "Thinking",
        kind: "toggle",
        value: canonical?.thinking ? "true" : "false",
        options: [
          { value: "false", label: "Off" },
          { value: "true", label: "On" },
        ],
      });
    }
    if (hasFast) {
      settings.push({
        id: "fast",
        label: "Fast",
        kind: "toggle",
        value: canonical?.fast ? "true" : "false",
        options: [
          { value: "false", label: "Off" },
          { value: "true", label: "Fast" },
        ],
      });
    }
    return {
      id: `cursor:${base}`,
      harness: "cursor" as const,
      name: family.name,
      nativeId: base,
      settings: settings.length > 0 ? settings : undefined,
    };
  });
}

function parseCliVariant(id: string): {
  base: string;
  effort?: string;
  fast: boolean;
  thinking: boolean;
} {
  let rest = id;
  let fast = false;
  let thinking = false;
  let effort: string | undefined;
  if (rest.endsWith("-fast")) {
    fast = true;
    rest = rest.slice(0, -5);
  }
  if (rest.endsWith("-thinking")) {
    thinking = true;
    rest = rest.slice(0, -9);
  }
  for (const token of EFFORT_TOKENS) {
    const suffix = `-${token}`;
    if (rest.endsWith(suffix) && rest.length > suffix.length) {
      effort = token;
      rest = rest.slice(0, -suffix.length);
      break;
    }
  }
  if (rest.endsWith("-thinking")) {
    thinking = true;
    rest = rest.slice(0, -9);
  }
  return { base: rest || id, effort, fast, thinking };
}

function parseConfigOptions(raw: unknown): ModelSetting[] | undefined {
  if (!Array.isArray(raw) || raw.length === 0) return undefined;
  const settings: ModelSetting[] = [];
  for (const item of raw) {
    const rec = asRecord(item);
    if (!rec) continue;
    const id = String(rec.id ?? rec.configId ?? "").trim();
    const category = String(rec.category ?? "").trim().toLowerCase();
    if (
      !id ||
      id === "mode" ||
      id === "model" ||
      category === "mode" ||
      category === "model"
    ) {
      continue;
    }
    const label = String(rec.name ?? rec.label ?? id).trim() || id;
    const description =
      typeof rec.description === "string" ? rec.description : undefined;
    const type = String(rec.type ?? "select");
    if (type === "boolean") {
      const on = rec.currentValue === true || rec.currentValue === "true";
      settings.push({
        id,
        label,
        description,
        kind: "toggle",
        value: on ? "true" : "false",
        options: [
          { value: "false", label: "Off" },
          { value: "true", label: "On" },
        ],
      });
      continue;
    }
    const options = flattenSelectOptions(rec.options);
    if (options.length === 0) continue;
    const current = String(rec.currentValue ?? options[0]?.value ?? "");
    const values = new Set(options.map((option) => option.value.toLowerCase()));
    const kind =
      values.has("true") && values.has("false") && options.length <= 2
        ? "toggle"
        : "select";
    settings.push({
      id,
      label,
      description,
      kind,
      value: current,
      options,
    });
  }
  return settings.length > 0 ? settings : undefined;
}

function flattenSelectOptions(raw: unknown): ModelSettingChoice[] {
  if (!Array.isArray(raw)) return [];
  const out: ModelSettingChoice[] = [];
  for (const entry of raw) {
    const rec = asRecord(entry);
    if (!rec) continue;
    if (typeof rec.value === "string") {
      const value = rec.value.trim();
      if (!value) continue;
      out.push({
        value,
        label: String(rec.name ?? rec.label ?? value).trim() || value,
      });
      continue;
    }
    out.push(...flattenSelectOptions(rec.options));
  }
  return out;
}

function uniqueCursorModels(models: AgentModel[]): AgentModel[] {
  const seen = new Set<string>();
  return models.filter((model) => {
    if (!model.nativeId || seen.has(model.nativeId)) return false;
    seen.add(model.nativeId);
    return true;
  });
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function unique<T>(values: T[]): T[] {
  return [...new Set(values)];
}

function stripVariantWords(name: string): string {
  return name
    .replace(
      /\s+(Low|Medium|High|Extra High|Max|None|Minimal|Fast|Thinking)(\s+Fast)?$/i,
      "",
    )
    .trim();
}

function effortKeyFor(base: string): string {
  if (base.startsWith("gpt-") || base.startsWith("kimi-") || base.startsWith("glm-")) {
    return "reasoning";
  }
  return "effort";
}

function effortLabel(value: string): string {
  switch (value) {
    case "none":
      return "None";
    case "minimal":
      return "Minimal";
    case "low":
      return "Low";
    case "medium":
      return "Medium";
    case "high":
      return "High";
    case "xhigh":
    case "extra-high":
      return "Extra High";
    case "max":
      return "Max";
    default:
      return value;
  }
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
          reject(new Error("Cursor model discovery timed out"));
        }, ms);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
    void pending.catch(() => undefined);
  }
}
