import { pathKey } from "../../../shared/lib/paths";
import type { HarnessId } from "./session";

/**
 * Per-project overrides for the Providers settings: which provider new
 * conversations start with, each provider's model, and which providers show in
 * the model picker. Anything left unset falls back to the global defaults, so a
 * project only stores what it actually overrides.
 */

const KEY = "monocode.projectProviderSettings.v1";

/** Fired on `window` when any project's provider settings change. */
export const PROJECT_PROVIDERS_CHANGE_EVENT = "monocode:project-providers-change";

export type ProjectProviderSettings = {
  defaultHarness?: HarnessId;
  defaultModel?: string;
  models?: Partial<Record<HarnessId, string>>;
  /** Providers this project keeps out of the picker and out of new sessions. */
  hidden?: HarnessId[];
};

type Stored = Record<string, ProjectProviderSettings>;

let revision = 0;
let cache: Stored | null = null;
let cacheRaw: string | null = null;

/** Bumps on every write so `useSyncExternalStore` consumers can re-render. */
export function projectProvidersRevision(): number {
  return revision;
}

export function loadProjectProviderSettings(
  project: string | undefined,
): ProjectProviderSettings {
  if (!project) return {};
  return readAll()[pathKey(project)] ?? {};
}

export function isProviderHidden(
  project: string | undefined,
  harness: HarnessId,
): boolean {
  return (loadProjectProviderSettings(project).hidden ?? []).includes(harness);
}

export function setProjectDefaultProvider(
  project: string,
  harness: HarnessId,
  model: string,
): void {
  update(project, (current) => ({
    ...current,
    defaultHarness: harness,
    defaultModel: model,
  }));
}

export function setProjectDefaultModel(
  project: string,
  harness: HarnessId,
  model: string,
): void {
  update(project, (current) => ({
    ...current,
    models: { ...current.models, [harness]: model },
  }));
}

export function setProjectProviderHidden(  project: string,
  harness: HarnessId,
  hidden: boolean,
): void {
  update(project, (current) => {
    const next = new Set(current.hidden ?? []);
    if (hidden) next.add(harness);
    else next.delete(harness);
    const list = [...next];
    return { ...current, hidden: list.length > 0 ? list : undefined };
  });
}

export function clearProjectProviders(project: string): void {
  const key = pathKey(project);
  const all = { ...readAll() };
  if (!(key in all)) return;
  delete all[key];
  writeAll(all);
}

/** Follow a project rename so its overrides are not orphaned. */
export function rebaseProjectProviders(from: string, to: string): void {
  const fromKey = pathKey(from);
  const toKey = pathKey(to);
  if (fromKey === toKey) return;
  const all = { ...readAll() };
  if (!(fromKey in all)) return;
  all[toKey] = all[fromKey];
  delete all[fromKey];
  writeAll(all);
}

export function subscribeProjectProviders(listener: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const storage = (event: StorageEvent) => {
    if (event.key !== KEY) return;
    cache = null;
    cacheRaw = null;
    revision += 1;
    listener();
  };
  window.addEventListener(PROJECT_PROVIDERS_CHANGE_EVENT, listener);
  window.addEventListener("storage", storage);
  return () => {
    window.removeEventListener(PROJECT_PROVIDERS_CHANGE_EVENT, listener);
    window.removeEventListener("storage", storage);
  };
}

function update(
  project: string,
  change: (current: ProjectProviderSettings) => ProjectProviderSettings,
): void {
  const key = pathKey(project);
  const all = { ...readAll() };
  const next = normalize(change(all[key] ?? {}));
  if (empty(next)) delete all[key];
  else all[key] = next;
  writeAll(all);
}

function empty(value: ProjectProviderSettings): boolean {
  return (
    value.defaultHarness == null &&
    value.defaultModel == null &&
    Object.keys(value.models ?? {}).length === 0 &&
    (value.hidden ?? []).length === 0
  );
}

function readAll(): Stored {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(KEY);
  } catch {
    raw = null;
  }
  if (cache && cacheRaw === raw) return cache;
  cacheRaw = raw;
  cache = parse(raw);
  return cache;
}

function parse(raw: string | null): Stored {
  if (!raw) return {};
  try {
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    const out: Stored = {};
    for (const [key, entry] of Object.entries(
      value as Record<string, unknown>,
    )) {
      if (!entry || typeof entry !== "object" || Array.isArray(entry)) continue;
      const clean = normalize(entry as ProjectProviderSettings);
      if (!empty(clean)) out[key] = clean;
    }
    return out;
  } catch {
    return {};
  }
}

function normalize(value: ProjectProviderSettings): ProjectProviderSettings {
  const next: ProjectProviderSettings = {};
  if (typeof value.defaultHarness === "string") {
    next.defaultHarness = value.defaultHarness;
  }
  if (typeof value.defaultModel === "string" && value.defaultModel) {
    next.defaultModel = value.defaultModel;
  }
  if (value.models && typeof value.models === "object") {
    const models: Partial<Record<HarnessId, string>> = {};
    for (const [harness, model] of Object.entries(value.models)) {
      if (typeof model === "string" && model) {
        models[harness as HarnessId] = model;
      }
    }
    if (Object.keys(models).length > 0) next.models = models;
  }
  if (Array.isArray(value.hidden)) {
    const hidden = [
      ...new Set(
        (value.hidden as unknown[]).filter(
          (id): id is HarnessId => typeof id === "string" && id.length > 0,
        ),
      ),
    ];
    if (hidden.length > 0) next.hidden = hidden;
  }
  return next;
}

function writeAll(next: Stored): void {
  revision += 1;
  cache = next;
  const serialized = JSON.stringify(next);
  cacheRaw = serialized;
  try {
    localStorage.setItem(KEY, serialized);
  } catch {
    // private mode / quota
  }
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(PROJECT_PROVIDERS_CHANGE_EVENT));
}
