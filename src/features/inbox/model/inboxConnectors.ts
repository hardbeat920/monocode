import { useSyncExternalStore } from "react";
import type { InboxProvider } from "./githubTasks";

export type InboxConnectors = Record<InboxProvider, boolean>;

const KEY = "monocode.inboxConnectors.v1";
const CHANGE = "monocode:inbox-connectors-change";
const DEFAULTS: InboxConnectors = Object.freeze({
  github: true,
  gitlab: true,
  azuredevops: true,
  jira: true,
  linear: true,
});
const PROVIDERS = Object.keys(DEFAULTS) as InboxProvider[];

let snapshot = DEFAULTS;
let lastStorageValue: string | null | undefined;
let memoryOverride:
  | { value: InboxConnectors; storageValue: string | null | undefined }
  | undefined;

function updateSnapshot(value: InboxConnectors): InboxConnectors {
  if (PROVIDERS.some((provider) => snapshot[provider] !== value[provider]))
    snapshot = Object.freeze(value);
  return snapshot;
}

function parseConnectors(value: string | null): InboxConnectors {
  try {
    const parsed: unknown = JSON.parse(value ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
      return DEFAULTS;
    const values = parsed as Record<string, unknown>;
    return Object.fromEntries(
      PROVIDERS.map((provider) => [
        provider,
        typeof values[provider] === "boolean" ? values[provider] : true,
      ]),
    ) as InboxConnectors;
  } catch {
    return DEFAULTS;
  }
}

/** Connector choices are independent of the saved connection credentials. */
export function loadInboxConnectors(): InboxConnectors {
  try {
    const value = localStorage.getItem(KEY);
    lastStorageValue = value;
    // Keep a failed write effective for this window until persisted data changes.
    if (
      memoryOverride &&
      (memoryOverride.storageValue === undefined ||
        memoryOverride.storageValue === value)
    )
      return updateSnapshot(memoryOverride.value);
    memoryOverride = undefined;
    return updateSnapshot(parseConnectors(value));
  } catch {
    return snapshot;
  }
}

export function setInboxConnectorEnabled(
  provider: InboxProvider,
  enabled: boolean,
): void {
  const previous = loadInboxConnectors();
  if (previous[provider] === enabled) return;
  const value = updateSnapshot({ ...previous, [provider]: enabled });
  try {
    localStorage.setItem(KEY, JSON.stringify(value));
    memoryOverride = undefined;
  } catch {
    memoryOverride = { value, storageValue: lastStorageValue };
  }
  if (typeof window !== "undefined") window.dispatchEvent(new Event(CHANGE));
}

export function isInboxConnectorEnabled(provider: InboxProvider): boolean {
  return loadInboxConnectors()[provider];
}

export function subscribeInboxConnectors(listener: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const onStorage = (event: StorageEvent) => {
    if (event.key !== KEY && event.key !== null) return;
    memoryOverride = undefined;
    listener();
  };
  window.addEventListener(CHANGE, listener);
  window.addEventListener("storage", onStorage);
  window.addEventListener("focus", listener);
  return () => {
    window.removeEventListener(CHANGE, listener);
    window.removeEventListener("storage", onStorage);
    window.removeEventListener("focus", listener);
  };
}

/** Share a stable reactive snapshot across Settings and all inbox surfaces. */
export function useInboxConnectors(): InboxConnectors {
  return useSyncExternalStore(
    subscribeInboxConnectors,
    loadInboxConnectors,
    () => DEFAULTS,
  );
}
