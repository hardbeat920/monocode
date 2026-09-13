import { homeDir } from "../fs";
import {
  setHarnessModels,
  type AgentModel,
  type ModelSetting,
} from "../models";
import {
  execChild,
  killChild,
  resolveClaudeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import {
  asRecord,
  buildClaudeSpawnArgs,
  buildControlRequest,
  compareSemver,
  isClaudeInitMessage,
  listModelsFromControlResponse,
  MINIMUM_CLAUDE_FABLE_5_VERSION,
  MINIMUM_CLAUDE_OPUS_4_7_VERSION,
  MINIMUM_CLAUDE_OPUS_4_8_VERSION,
  MINIMUM_CLAUDE_OPUS_5_VERSION,
  MINIMUM_CLAUDE_SONNET_5_VERSION,
  parseClaudeVersion,
  parseControlResponse,
  parseJsonLine,
  stringField,
} from "./claudeProtocol";

const EFFORT_LOW_TO_ULTRATHINK: ModelSetting = {
  id: "effort",
  label: "Reasoning",
  kind: "select",
  value: "high",
  options: [
    { value: "low", label: "Low" },
    { value: "medium", label: "Medium" },
    { value: "high", label: "High" },
    { value: "max", label: "Max" },
    { value: "ultrathink", label: "Ultrathink" },
  ],
};

const EFFORT_WITH_XHIGH: ModelSetting = {
  id: "effort",
  label: "Reasoning",
  kind: "select",
  value: "high",
  options: [
    { value: "low", label: "Low" },
    { value: "medium", label: "Medium" },
    { value: "high", label: "High" },
    { value: "xhigh", label: "Extra High" },
    { value: "max", label: "Max" },
    {
      value: "ultracode",
      label: "Ultracode",
    },
    { value: "ultrathink", label: "Ultrathink" },
  ],
};

const EFFORT_OPUS_47: ModelSetting = {
  id: "effort",
  label: "Reasoning",
  kind: "select",
  value: "xhigh",
  options: [
    { value: "low", label: "Low" },
    { value: "medium", label: "Medium" },
    { value: "high", label: "High" },
    { value: "xhigh", label: "Extra High" },
    { value: "max", label: "Max" },
    { value: "ultrathink", label: "Ultrathink" },
  ],
};

const FAST_MODE: ModelSetting = {
  id: "fast",
  label: "Fast",
  kind: "toggle",
  value: "false",
  options: [
    { value: "true", label: "On" },
    { value: "false", label: "Off" },
  ],
};

const THINKING: ModelSetting = {
  id: "thinking",
  label: "Thinking",
  kind: "toggle",
  value: "false",
  options: [
    { value: "true", label: "On" },
    { value: "false", label: "Off" },
  ],
};

function contextWindow(defaultValue: "200k" | "1m"): ModelSetting {
  return {
    id: "context",
    label: "Context",
    kind: "select",
    value: defaultValue,
    options: [
      { value: "200k", label: "200k" },
      { value: "1m", label: "1M" },
    ],
  };
}

/** Fallback catalog when `list_models` is unavailable. */
export const CLAUDE_MODEL_CATALOG: AgentModel[] = [
  {
    id: "claude:fable-5",
    harness: "claude",
    name: "Claude Fable 5",
    nativeId: "claude-fable-5",
    settings: [EFFORT_WITH_XHIGH, contextWindow("1m")],
  },
  {
    id: "claude:opus-5",
    harness: "claude",
    name: "Claude Opus 5",
    nativeId: "claude-opus-5",
    settings: [EFFORT_WITH_XHIGH, FAST_MODE, contextWindow("1m")],
  },
  {
    id: "claude:sonnet-5",
    harness: "claude",
    name: "Claude Sonnet 5",
    nativeId: "claude-sonnet-5",
    settings: [EFFORT_WITH_XHIGH, contextWindow("200k")],
  },
  {
    id: "claude:opus-4.8",
    harness: "claude",
    name: "Claude Opus 4.8",
    nativeId: "claude-opus-4-8",
    settings: [EFFORT_WITH_XHIGH, FAST_MODE],
  },
  {
    id: "claude:opus-4.7",
    harness: "claude",
    name: "Claude Opus 4.7",
    nativeId: "claude-opus-4-7",
    settings: [EFFORT_OPUS_47, FAST_MODE],
  },
  {
    id: "claude:opus-4.6",
    harness: "claude",
    name: "Claude Opus 4.6",
    nativeId: "claude-opus-4-6",
    settings: [EFFORT_LOW_TO_ULTRATHINK, FAST_MODE, contextWindow("1m")],
  },
  {
    id: "claude:sonnet-4.6",
    harness: "claude",
    name: "Claude Sonnet 4.6",
    nativeId: "claude-sonnet-4-6",
    settings: [EFFORT_LOW_TO_ULTRATHINK, contextWindow("200k")],
  },
  {
    id: "claude:opus-4.5",
    harness: "claude",
    name: "Claude Opus 4.5",
    nativeId: "claude-opus-4-5",
    settings: [
      {
        id: "effort",
        label: "Reasoning",
        kind: "select",
        value: "high",
        options: [
          { value: "low", label: "Low" },
          { value: "medium", label: "Medium" },
          { value: "high", label: "High" },
          { value: "max", label: "Max" },
        ],
      },
      FAST_MODE,
    ],
  },
  {
    id: "claude:haiku-4.5",
    harness: "claude",
    name: "Claude Haiku 4.5",
    nativeId: "claude-haiku-4-5",
    settings: [THINKING],
  },
];

const PROBE_ID = "monocode-claude-probe";
const LIST_MODELS_REQUEST_ID = "monocode_list_models";
const INIT_REQUEST_ID = "monocode_init";
const DISCOVERY_TIMEOUT_MS = 15_000;

const EFFORT_LABELS: Record<string, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  max: "Max",
};

let inflight: Promise<void> | null = null;

export function refreshClaudeCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverClaudeModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("claude", models);
    })
    .catch((error: unknown) => {
      console.debug("[monocode] claude catalog", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

async function discoverClaudeModels(): Promise<AgentModel[]> {
  const listed = await discoverViaListModels().catch((error: unknown) => {
    console.debug("[monocode] claude list_models catalog failed", error);
    return [];
  });
  if (listed.length > 0) return listed;
  return discoverViaVersion();
}

async function discoverViaListModels(): Promise<AgentModel[]> {
  const { path } = await resolveClaudeBinary();
  const cwd = await homeDir();
  const sessionId = crypto.randomUUID();

  let listed: ((models: AgentModel[]) => void) | null = null;
  let failed: ((error: Error) => void) | null = null;
  const pending = new Promise<AgentModel[]>((resolve, reject) => {
    listed = resolve;
    failed = reject;
  });

  let asked = false;
  const ask = () => {
    if (asked) return;
    asked = true;
    void writeChild(
      PROBE_ID,
      JSON.stringify(
        buildControlRequest(LIST_MODELS_REQUEST_ID, { subtype: "list_models" }),
      ),
    ).catch((error: unknown) => {
      failed?.(error instanceof Error ? error : new Error(String(error)));
    });
  };

  const stop = async () => {
    unwatchChild(PROBE_ID);
    await killChild(PROBE_ID).catch(() => undefined);
  };

  watchChild(
    PROBE_ID,
    (line) => {
      const rec = parseJsonLine(line);
      if (!rec) return;
      if (isClaudeInitMessage(rec)) ask();
      const init = parseControlResponse(rec);
      if (init?.ok && init.requestId === INIT_REQUEST_ID) ask();
      const rows = listModelsFromControlResponse(rec, LIST_MODELS_REQUEST_ID);
      if (rows) listed?.(modelsFromClaudeListModels(rows));
    },
    () => failed?.(new Error("Claude Code catalog probe exited")),
  );

  try {
    await spawnChild(
      PROBE_ID,
      path,
      buildClaudeSpawnArgs({ isolated: true, sessionId }),
      cwd,
    );
    await writeChild(
      PROBE_ID,
      JSON.stringify(
        buildControlRequest(INIT_REQUEST_ID, { subtype: "initialize" }),
      ),
    );
    return await withTimeout(DISCOVERY_TIMEOUT_MS, pending, () => {
      void stop();
    });
  } finally {
    await stop();
  }
}

async function discoverViaVersion(): Promise<AgentModel[]> {
  const { path } = await resolveClaudeBinary();
  const cwd = await homeDir();
  const versionOut = await execChild(path, ["--version"], cwd);
  const version = parseClaudeVersion(versionOut);
  return modelsForClaudeVersion(version);
}

function withTimeout<T>(
  ms: number,
  promise: Promise<T>,
  onTimeout: () => void,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      onTimeout();
      reject(new Error("Claude Code catalog probe timed out"));
    }, ms);
    promise.then(resolve, reject).finally(() => clearTimeout(timer));
  });
}

/** Map a `list_models` payload into the picker catalog. */
export function modelsFromClaudeListModels(raw: unknown): AgentModel[] {
  const rows = Array.isArray(raw)
    ? raw
    : Array.isArray(asRecord(raw)?.models)
      ? (asRecord(raw)?.models as unknown[])
      : [];
  const models: AgentModel[] = [];
  const seen = new Set<string>();
  for (const item of rows) {
    const model = modelFromListRow(item);
    if (!model) continue;
    const key = model.nativeId ?? model.id;
    if (seen.has(key)) continue;
    seen.add(key);
    models.push(model);
  }
  return models;
}

function modelFromListRow(raw: unknown): AgentModel | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  if (rec.disabled === true) return null;
  const value = stringField(rec, "value") ?? "";
  if (!value || value === "default" || value.startsWith("cc-update-required")) {
    return null;
  }
  const resolved = stringField(rec, "resolvedModel") ?? "";
  const fromValue = splitClaudeModelValue(value);
  const fromResolved = splitClaudeModelValue(resolved);
  const nativeId = fromValue.id || fromResolved.id;
  if (!nativeId) return null;

  const displayName = stringField(rec, "displayName") ?? "";
  const description = stringField(rec, "description") ?? "";
  const name = pickerName(displayName, description, nativeId);
  const settings = settingsFromListRow(rec, fromValue.context1m || fromResolved.context1m);

  return {
    id: claudeCatalogId(nativeId),
    harness: "claude",
    name,
    nativeId,
    ...(settings.length > 0 ? { settings } : {}),
  };
}

function settingsFromListRow(
  rec: Record<string, unknown>,
  context1m: boolean,
): ModelSetting[] {
  const settings: ModelSetting[] = [];
  const levels = advertisedEffortLevels(rec);
  if (rec.supportsEffort === true || levels.length > 0) {
    settings.push(effortSetting(levels));
  } else if (rec.supportsAdaptiveThinking === true) {
    settings.push(THINKING);
  }
  if (rec.supportsFastMode === true) settings.push(FAST_MODE);
  if (context1m) settings.push(contextWindow("1m"));
  return settings;
}

function advertisedEffortLevels(rec: Record<string, unknown>): string[] {
  const raw = rec.supportedEffortLevels;
  if (!Array.isArray(raw)) return [];
  return raw.filter((level): level is string => typeof level === "string" && level.trim() !== "");
}

function effortSetting(levels: string[]): ModelSetting {
  const known = levels.filter((level) => EFFORT_LABELS[level]);
  const options = (known.length > 0 ? known : ["low", "medium", "high", "max"]).map(
    (value) => ({ value, label: EFFORT_LABELS[value] ?? value }),
  );
  if (options.some((option) => option.value === "xhigh")) {
    options.push({ value: "ultracode", label: "Ultracode" });
  }
  options.push({ value: "ultrathink", label: "Ultrathink" });
  const defaultValue = options.some((option) => option.value === "high")
    ? "high"
    : (options[0]?.value ?? "high");
  return {
    id: "effort",
    label: "Reasoning",
    kind: "select",
    value: defaultValue,
    options,
  };
}

function pickerName(
  displayName: string,
  description: string,
  fallback: string,
): string {
  const name = displayName.trim();
  const head = description.split("·")[0]?.trim() ?? "";
  if (
    head &&
    name &&
    head.toLowerCase().startsWith(name.toLowerCase()) &&
    head.length > name.length
  ) {
    return head;
  }
  return name || head || fallback;
}

function splitClaudeModelValue(value: string): { id: string; context1m: boolean } {
  const match = /^(.*)\[1m\]$/i.exec(value.trim());
  if (match?.[1]?.trim()) return { id: match[1].trim(), context1m: true };
  return { id: value.trim(), context1m: false };
}

function claudeCatalogId(nativeId: string): string {
  const slug = nativeId.startsWith("claude-") ? nativeId.slice("claude-".length) : nativeId;
  return `claude:${slug}`;
}

export function modelsForClaudeVersion(
  version: string | null | undefined,
): AgentModel[] {
  return CLAUDE_MODEL_CATALOG.filter((model) => {
    const slug = model.nativeId ?? "";
    if (slug === "claude-opus-5") {
      return version
        ? compareSemver(version, MINIMUM_CLAUDE_OPUS_5_VERSION) >= 0
        : false;
    }
    if (slug === "claude-sonnet-5") {
      return version
        ? compareSemver(version, MINIMUM_CLAUDE_SONNET_5_VERSION) >= 0
        : false;
    }
    if (slug === "claude-fable-5") {
      return version
        ? compareSemver(version, MINIMUM_CLAUDE_FABLE_5_VERSION) >= 0
        : false;
    }
    if (slug === "claude-opus-4-8") {
      return version
        ? compareSemver(version, MINIMUM_CLAUDE_OPUS_4_8_VERSION) >= 0
        : false;
    }
    if (slug === "claude-opus-4-7") {
      return version
        ? compareSemver(version, MINIMUM_CLAUDE_OPUS_4_7_VERSION) >= 0
        : false;
    }
    return true;
  });
}
