import { pathKey } from "./paths";
import type { RateLimitProvider } from "./rateLimits";

const ACCOUNTS_KEY = "monocode.providerAccounts.v1";
const SELECTIONS_KEY = "monocode.providerAccountSelections.v1";
const CHANGE_EVENT = "monocode-provider-accounts-changed";

export const DEFAULT_PROVIDER_ACCOUNT_ID = "default";

export type ProviderAccount = {
  id: string;
  provider: RateLimitProvider;
  label: string;
  isDefault?: boolean;
};

type StoredAccounts = Partial<Record<RateLimitProvider, ProviderAccount[]>>;
type StoredSelections = Record<
  string,
  Partial<Record<RateLimitProvider, string>>
>;

export function providerAccounts(
  provider: RateLimitProvider,
): ProviderAccount[] {
  const stored = readRecord<StoredAccounts>(ACCOUNTS_KEY);
  const seen = new Set<string>([DEFAULT_PROVIDER_ACCOUNT_ID]);
  const accounts = Array.isArray(stored[provider]) ? stored[provider] : [];
  const profiles = accounts.flatMap((account) => {
    const id = validAccountId(account?.id) ? account.id : "";
    const label = cleanLabel(account?.label);
    if (!id || id === DEFAULT_PROVIDER_ACCOUNT_ID || !label || seen.has(id)) {
      return [];
    }
    seen.add(id);
    return [{ id, provider, label }];
  });
  return [
    {
      id: DEFAULT_PROVIDER_ACCOUNT_ID,
      provider,
      label: "Default account",
      isDefault: true,
    },
    ...profiles,
  ];
}

export function newProviderAccount(
  provider: RateLimitProvider,
  label: string,
): ProviderAccount {
  return {
    id: `account-${crypto.randomUUID()}`,
    provider,
    label:
      cleanLabel(label) || `Account ${providerAccounts(provider).length + 1}`,
  };
}

export function saveProviderAccount(account: ProviderAccount): void {
  if (
    account.id === DEFAULT_PROVIDER_ACCOUNT_ID ||
    !validAccountId(account.id)
  ) {
    return;
  }
  const label = cleanLabel(account.label);
  if (!label) return;
  const stored = readRecord<StoredAccounts>(ACCOUNTS_KEY);
  const storedAccounts = stored[account.provider];
  const accounts = Array.isArray(storedAccounts) ? storedAccounts : [];
  const next = accounts.flatMap((entry) => {
    if (!isRecord(entry)) return [];
    const id = validAccountId(entry.id) ? entry.id : "";
    const storedLabel = cleanLabel(entry.label);
    if (
      !id ||
      id === DEFAULT_PROVIDER_ACCOUNT_ID ||
      id === account.id ||
      !storedLabel
    ) {
      return [];
    }
    return [{ id, provider: account.provider, label: storedLabel }];
  });
  stored[account.provider] = [
    ...next,
    { ...account, label, isDefault: undefined },
  ];
  writeJson(ACCOUNTS_KEY, stored);
  announceChange();
}

export function selectedProviderAccountId(
  provider: RateLimitProvider,
  project: string | undefined,
): string {
  const selections = readRecord<StoredSelections>(SELECTIONS_KEY);
  const id = selections[selectionKey(project)]?.[provider];
  return providerAccounts(provider).some((account) => account.id === id)
    ? id!
    : DEFAULT_PROVIDER_ACCOUNT_ID;
}

export function selectProviderAccount(
  provider: RateLimitProvider,
  project: string | undefined,
  accountId: string,
): void {
  if (!providerAccounts(provider).some((account) => account.id === accountId)) {
    return;
  }
  const selections = readRecord<StoredSelections>(SELECTIONS_KEY);
  const key = selectionKey(project);
  selections[key] = { ...selections[key], [provider]: accountId };
  writeJson(SELECTIONS_KEY, selections);
  announceChange();
}

export function providerAccountLabel(
  provider: RateLimitProvider,
  accountId: string | undefined,
): string {
  return (
    providerAccounts(provider).find((account) => account.id === accountId)
      ?.label ?? "Default account"
  );
}

export function subscribeProviderAccounts(listener: () => void): () => void {
  const local = () => listener();
  const storage = (event: StorageEvent) => {
    if (event.key === ACCOUNTS_KEY || event.key === SELECTIONS_KEY) listener();
  };
  window.addEventListener(CHANGE_EVENT, local);
  window.addEventListener("storage", storage);
  return () => {
    window.removeEventListener(CHANGE_EVENT, local);
    window.removeEventListener("storage", storage);
  };
}

function selectionKey(project: string | undefined): string {
  return pathKey(project?.trim() || "~");
}

function cleanLabel(value: unknown): string {
  return typeof value === "string"
    ? value.replace(/\s+/g, " ").trim().slice(0, 48)
    : "";
}

function validAccountId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 80 &&
    /^[A-Za-z0-9_-]+$/.test(value)
  );
}

function readRecord<T>(key: string): T {
  const value = readJson<unknown>(key, {});
  return isRecord(value) ? (value as T) : ({} as T);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function writeJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // A private or full storage area should not block the provider itself.
  }
}

function announceChange(): void {
  window.dispatchEvent(new Event(CHANGE_EVENT));
}
