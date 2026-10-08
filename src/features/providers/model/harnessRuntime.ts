import { invoke } from "@tauri-apps/api/core";
import type { HarnessId } from "../../sessions/model/session";

export type HarnessRuntimeVariable = {
  name: string;
  configured: boolean;
  sensitive: boolean;
};

export type HarnessRuntimeModel = {
  id: string;
  name: string;
};

export type HarnessRuntimeSnapshot = {
  harness: HarnessId;
  binaryPath: string | null;
  configPath: string | null;
  configSource: string | null;
  authMode: "api" | "oauth" | "unknown";
  authStatus: "configured" | "missing-key" | "authenticated" | "not-configured" | "unknown";
  providerName: string | null;
  baseUrl: string | null;
  model: string | null;
  environment: HarnessRuntimeVariable[];
  models: HarnessRuntimeModel[];
  modelSource: string | null;
  error: string | null;
};

export type HarnessRuntimeOverride = {
  baseUrl?: string;
  model?: string;
  environment?: Record<string, string>;
  environmentNames?: string[];
};

const overrides = new Map<HarnessId, HarnessRuntimeOverride>();
const volatileEnvironment = new Map<HarnessId, Record<string, string>>();
const OVERRIDE_KEY = "monocode.harnessRuntimeOverrides.v1";
try {
  const raw = localStorage.getItem(OVERRIDE_KEY);
  if (raw) {
    const parsed = JSON.parse(raw) as Record<string, HarnessRuntimeOverride>;
    const sanitized: Record<string, HarnessRuntimeOverride> = {};
    for (const [harness, value] of Object.entries(parsed)) {
      const safe = {
        baseUrl: value.baseUrl,
        model: value.model,
        environmentNames: value.environmentNames ?? Object.keys(value.environment ?? {}),
      };
      overrides.set(harness as HarnessId, safe);
      sanitized[harness] = safe;
    }
    localStorage.setItem(OVERRIDE_KEY, JSON.stringify(sanitized));
  }
} catch { /* unavailable in native/headless startup */ }
export function getHarnessRuntimeOverride(harness: HarnessId): HarnessRuntimeOverride | undefined {
  const metadata = overrides.get(harness);
  const environment = volatileEnvironment.get(harness);
  if (!metadata && !environment) return undefined;
  return { ...metadata, ...(environment ? { environment } : {}) };
}
export function setHarnessRuntimeOverride(harness: HarnessId, value: HarnessRuntimeOverride): void {
  const { environment, ...metadata } = value;
  const activeEnvironment = environment
    ? Object.fromEntries(
        Object.entries(environment).filter(([, entry]) => entry.trim().length > 0),
      )
    : {};
  overrides.set(harness, {
    ...metadata,
    environmentNames: environment
      ? Object.keys(environment)
      : metadata.environmentNames ?? [],
  });
  if (Object.keys(activeEnvironment).length > 0) {
    volatileEnvironment.set(harness, activeEnvironment);
  } else {
    volatileEnvironment.delete(harness);
  }
  try {
    const persisted = Object.fromEntries(
      [...overrides.entries()].map(([key, entry]) => [key, {
        baseUrl: entry.baseUrl,
        model: entry.model,
        environmentNames: Object.keys(entry.environment ?? {}).length > 0
          ? Object.keys(entry.environment ?? {})
          : entry.environmentNames ?? [],
      }]),
    );
    localStorage.setItem(OVERRIDE_KEY, JSON.stringify(persisted));
  } catch { /* ignore storage failures */ }
  revision += 1;
  for (const listener of listeners) listener();
}
export function clearHarnessRuntimeOverride(harness: HarnessId): void {
  overrides.delete(harness);
  volatileEnvironment.delete(harness);
  try {
    localStorage.setItem(OVERRIDE_KEY, JSON.stringify(Object.fromEntries(overrides)));
  } catch { /* ignore storage failures */ }
  revision += 1;
  for (const listener of listeners) listener();
}

const snapshots = new Map<HarnessId, HarnessRuntimeSnapshot>();
const requests = new Map<string, Promise<HarnessRuntimeSnapshot>>();
const listeners = new Set<() => void>();
let revision = 0;

export function getHarnessRuntimeSnapshot(harness: HarnessId): HarnessRuntimeSnapshot | null {
  return snapshots.get(harness) ?? null;
}

export function harnessRuntimeRevision(): number {
  return revision;
}

export function subscribeHarnessRuntime(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function publish(harness: HarnessId, snapshot: HarnessRuntimeSnapshot): void {
  snapshots.set(harness, snapshot);
  revision += 1;
  for (const listener of listeners) listener();
}

function normalizeSnapshot(raw: unknown, harness: HarnessId): HarnessRuntimeSnapshot {
  const value = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const authMode = value.authMode === "api" || value.authMode === "oauth" ? value.authMode : "unknown";
  const authStatus = ["configured", "missing-key", "authenticated", "not-configured", "unknown"].includes(String(value.authStatus))
    ? (value.authStatus as HarnessRuntimeSnapshot["authStatus"])
    : "unknown";
  const list = (value.environment as unknown[] | undefined) ?? [];
  const models = (value.models as unknown[] | undefined) ?? [];
  return {
    harness,
    binaryPath: typeof value.binaryPath === "string" ? value.binaryPath : null,
    configPath: typeof value.configPath === "string" ? value.configPath : null,
    configSource: typeof value.configSource === "string" ? value.configSource : null,
    authMode,
    authStatus,
    providerName: typeof value.providerName === "string" ? value.providerName : null,
    baseUrl: typeof value.baseUrl === "string" ? value.baseUrl : null,
    model: typeof value.model === "string" ? value.model : null,
    environment: list.flatMap((entry) => {
      if (!entry || typeof entry !== "object") return [];
      const row = entry as Record<string, unknown>;
      return typeof row.name === "string"
        ? [{ name: row.name, configured: row.configured === true, sensitive: row.sensitive === true }]
        : [];
    }),
    models: models.flatMap((entry) => {
      if (!entry || typeof entry !== "object") return [];
      const row = entry as Record<string, unknown>;
      return typeof row.id === "string"
        ? [{ id: row.id, name: typeof row.name === "string" ? row.name : row.id }]
        : [];
    }),
    modelSource: typeof value.modelSource === "string" ? value.modelSource : null,
    error: typeof value.error === "string" ? value.error : null,
  };
}

export function inspectHarnessRuntime(
  harness: HarnessId,
  options: { refreshModels?: boolean; force?: boolean; override?: HarnessRuntimeOverride } = {},
): Promise<HarnessRuntimeSnapshot> {
  const key = `${harness}:${options.refreshModels === true}`;
  if (!options.force) {
    const cached = snapshots.get(harness);
    if (cached && options.refreshModels !== true) return Promise.resolve(cached);
    const pending = requests.get(key);
    if (pending) return pending;
  }
  const request = invoke<unknown>("harness_runtime_inspect", {
    harness,
    refreshModels: options.refreshModels === true,
    overrideConfig: options.override,
  }).then((raw) => {
    const snapshot = normalizeSnapshot(raw, harness);
    publish(harness, snapshot);
    return snapshot;
  });
  requests.set(key, request);
  return request.finally(() => {
    if (requests.get(key) === request) requests.delete(key);
  });
}

export function resetHarnessRuntimeSnapshots(): void {
  snapshots.clear();
  requests.clear();
  revision += 1;
}
