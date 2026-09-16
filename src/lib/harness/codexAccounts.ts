import { invoke } from "@tauri-apps/api/core";
import { asRecord, stringField } from "./codexProtocol";
import type { JsonRpcClient, JsonRpcId } from "./jsonRpc";

export type CodexAccount = {
  id: string;
  email: string;
  accountId: string;
  planType?: string | null;
  expiresAtMs?: number | null;
  blockedUntilMs?: number | null;
  disabledCause?: string | null;
  addedAt: number;
  lastUsedAt?: number | null;
  hasTokens: boolean;
};
export type CodexCredentials = CodexAccount & {
  accessToken: string;
  refreshToken: string;
};
type ExternalAuth = {
  accessToken: string;
  refreshToken: string;
  accountId: string;
  idToken?: string;
};
export type RateLimitSnapshot = {
  primary?: { usedPercent?: number; resetsAt?: number } | null;
  secondary?: { usedPercent?: number; resetsAt?: number } | null;
  rateLimitReachedType?: string | null;
};

export function isQuotaWallError(info: unknown, message = ""): boolean {
  if (info != null)
    return info === "usageLimitExceeded" || info === "rateLimitExceeded";
  return /usage limit|rate.?limit|quota|429/i.test(message);
}

export function isSharedScopeReached(type: unknown): boolean {
  return typeof type === "string" && type.startsWith("workspace_");
}

export function pickFailoverAccount(
  accounts: CodexAccount[],
  current: string | undefined,
  now = Date.now(),
): CodexAccount | undefined {
  const currentAccount = accounts.find((a) => a.id === current);
  return accounts
    .filter(
      (a) =>
        a.id !== current &&
        (!currentAccount || a.accountId !== currentAccount.accountId) &&
        a.hasTokens &&
        !a.disabledCause &&
        (a.blockedUntilMs ?? 0) <= now,
    )
    .sort((a, b) => (a.lastUsedAt ?? 0) - (b.lastUsedAt ?? 0))[0];
}

export function quotaResetAt(
  snapshot: RateLimitSnapshot | undefined,
  now = Date.now(),
): number {
  const resets = [snapshot?.primary, snapshot?.secondary]
    .filter(
      (w) => (w?.usedPercent ?? 0) >= 100 && (w?.resetsAt ?? 0) * 1000 > now,
    )
    .map((w) => w!.resetsAt! * 1000);
  // Both windows must recover before the account can be used again.
  return resets.length ? Math.max(...resets) : now + 60 * 60 * 1000;
}

function claims(token?: string): Record<string, unknown> | null {
  try {
    const part = token?.split(".")[1];
    if (!part) return null;
    return asRecord(
      JSON.parse(atob(part.replace(/-/g, "+").replace(/_/g, "/"))),
    );
  } catch {
    return null;
  }
}

let cached: CodexAccount[] = [];
let external: CodexAccount | undefined;
let refreshQueue = Promise.resolve();

export async function loadCodexAccounts(): Promise<CodexAccount[]> {
  cached = await invoke<CodexAccount[]>("codex_accounts_list");
  return cached;
}

export function failoverAccounts(): CodexAccount[] {
  return external ? [...cached, external] : cached;
}

export async function captureExternalAccount(
  rpc: Pick<JsonRpcClient, "request">,
): Promise<void> {
  const auth = await invoke<ExternalAuth>("codex_auth_json_read");
  const response = await rpc.request<{ account?: { email?: string } }>(
    "account/read",
    {},
    10_000,
  );
  const email = response.account?.email;
  if (!email) throw new Error("Cannot identify external Codex account");
  const identity = asRecord(
    claims(auth.idToken)?.["https://api.openai.com/auth"],
  );
  external = {
    ...(external?.accountId === auth.accountId ? external : {}),
    id: "external",
    email,
    accountId: auth.accountId,
    planType: stringField(identity, "chatgpt_plan_type"),
    addedAt: external?.addedAt ?? Date.now(),
    hasTokens: Boolean(auth.accessToken && auth.refreshToken),
  };
}

export async function updateAccountState(
  id: string,
  state: Partial<
    Pick<CodexAccount, "blockedUntilMs" | "disabledCause" | "lastUsedAt">
  >,
): Promise<void> {
  if (id === "external") {
    if (external) Object.assign(external, state);
    return;
  }
  await invoke("codex_account_update_state", { id, state });
  const account = cached.find((a) => a.id === id);
  if (account) Object.assign(account, state);
}

export async function accountCredentials(
  id: string,
  forceRefresh = false,
): Promise<CodexCredentials> {
  if (id === "external") {
    let auth = await invoke<ExternalAuth>("codex_auth_json_read");
    if (!external || external.accountId !== auth.accountId)
      throw new Error("External Codex account changed");
    const exp = claims(auth.accessToken)?.exp;
    if (forceRefresh || (typeof exp === "number" && exp * 1000 <= Date.now())) {
      try {
        auth = await invoke<ExternalAuth>("codex_auth_json_refresh", {
          accountId: auth.accountId,
          lastRefresh: new Date().toISOString(),
        });
      } catch {
        external.disabledCause = "External Codex refresh failed; sign in again";
        throw new Error(external.disabledCause);
      }
    }
    external.disabledCause = null;
    return { ...external, ...auth };
  }
  let account = await invoke<CodexCredentials>("codex_account_credentials", {
    id,
  });
  if (
    forceRefresh ||
    (account.expiresAtMs != null && account.expiresAtMs <= Date.now())
  ) {
    account = await invoke<CodexCredentials>("codex_account_refresh", { id });
  }
  return account;
}

/** Handles requests even during initialize/login, before a Live has been bound. */
export function respondCodexRefresh(
  rpc: Pick<JsonRpcClient, "respond" | "respondError">,
  requestId: JsonRpcId,
  params: unknown,
  pinnedId?: string,
): Promise<void> {
  const task = refreshQueue
    .catch(() => undefined)
    .then(async () => {
      const previous = stringField(asRecord(params), "previousAccountId");
      for (let attempt = 0; attempt < 3; attempt++) {
        try {
          await loadCodexAccounts();
          const account = previous
            ? failoverAccounts().find((a) => a.accountId === previous)
            : failoverAccounts().find((a) => a.id === pinnedId);
          if (!account) break;
          const credentials = await accountCredentials(account.id, true);
          await rpc.respond(requestId, {
            accessToken: credentials.accessToken,
            chatgptAccountId: credentials.accountId,
            ...(credentials.planType
              ? { chatgptPlanType: credentials.planType }
              : {}),
          });
          return;
        } catch {
          /* Bounded retry; never expose token endpoint payloads. */
        }
      }
      await rpc.respondError(requestId, {
        code: -32000,
        message: "Codex account refresh failed; sign in again",
      });
    });
  refreshQueue = task;
  return task;
}

export function resetCodexAccounts(): void {
  cached = [];
  external = undefined;
  refreshQueue = Promise.resolve();
}
