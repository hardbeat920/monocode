import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type {
  ProviderAccount,
  ProviderAccountProvider,
} from "./providerAccounts";

/** Identity the provider CLI cached on disk after sign-in. */
export type ProviderAccountIdentity = {
  email?: string | null;
  name?: string | null;
  plan?: string | null;
  organization?: string | null;
};

export async function readProviderAccountIdentity(
  provider: ProviderAccountProvider,
  accountId: string,
): Promise<ProviderAccountIdentity | null> {
  try {
    return await invoke<ProviderAccountIdentity | null>(
      "provider_account_identity",
      { provider, accountId },
    );
  } catch {
    return null;
  }
}

/** "Max · user@example.com", or whichever half is known. */
export function identitySubtitle(
  identity: ProviderAccountIdentity | null | undefined,
): string | null {
  const parts = [identity?.plan, identity?.email].filter(Boolean);
  return parts.length ? parts.join(" · ") : null;
}

/** Org chip text: "Personal" for Claude's default "<name>'s Organization". */
export function identityOrganizationTag(
  identity: ProviderAccountIdentity | null | undefined,
): string | null {
  const name = identity?.organization?.trim();
  if (!name) return null;
  return /['’]s Organization$/.test(name) ? "Personal" : name;
}

export function identityKey(account: ProviderAccount): string {
  return `${account.provider}:${account.id}`;
}

/**
 * Load identities for `accounts`, keyed by `identityKey`. Re-reads whenever
 * `refreshKey` changes.
 */
export function useProviderAccountIdentities(
  accounts: ProviderAccount[],
  refreshKey?: unknown,
): Record<string, ProviderAccountIdentity | null> {
  const [identities, setIdentities] = useState<
    Record<string, ProviderAccountIdentity | null>
  >({});
  const key = accounts.map(identityKey).join("|");

  useEffect(() => {
    let cancelled = false;
    void Promise.all(
      accounts.map(
        async (account) =>
          [
            identityKey(account),
            await readProviderAccountIdentity(account.provider, account.id),
          ] as const,
      ),
    ).then((entries) => {
      if (!cancelled) setIdentities(Object.fromEntries(entries));
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, refreshKey]);

  return identities;
}
