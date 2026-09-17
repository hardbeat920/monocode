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
  limitId?: string | null;
  limitName?: string | null;
  planType?: string | null;
  credits?: { hasCredits?: boolean; unlimited?: boolean; balance?: string } | null;
  individualLimit?: unknown;
  spendControlReached?: unknown;
  normalModelSlug?: string | null;
};

/** `account/rateLimits/updated` is sparse: present non-null keys replace,
 * absent and null keys keep (live pushes null out fields like primary). */
export function mergeRateLimits(
  base: RateLimitSnapshot | undefined,
  update: RateLimitSnapshot | undefined,
): RateLimitSnapshot | undefined {
  if (!update) return base;
  const merged: Record<string, unknown> = { ...(base as object) };
  for (const [key, value] of Object.entries(update)) {
    if (value != null) merged[key] = value;
  }
  return merged as RateLimitSnapshot;
}

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
  tried?: ReadonlySet<string>,
): CodexAccount | undefined {
  const currentAccount = accounts.find((a) => a.id === current);
  return accounts
    .filter(
      (a) =>
        a.id !== current &&
        !tried?.has(a.id) &&
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
  // The file on disk and the live process must describe the same identity,
  // otherwise a pooled candidate could bill the wrong account.
  const tokenEmail = stringField(claims(auth.idToken) ?? {}, "email");
  if (tokenEmail && tokenEmail.toLowerCase() !== email.toLowerCase())
    throw new Error("External Codex account does not match auth.json");
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

type CredentialError = Error & { permanent?: boolean };

function credentialError(error: unknown, fallback: string): CredentialError {
  const raw = error instanceof Error ? error.message : String(error);
  const permanent = raw.startsWith("CODEX_PERMANENT:");
  const wrapped: CredentialError = new Error(
    permanent ? raw.slice("CODEX_PERMANENT:".length) : raw || fallback,
  );
  wrapped.permanent = permanent;
  return wrapped;
}

// Refresh is single-flight per canonical account id so parallel requests can
// never race a rotated grant. Keying on the request's previousAccountId /
// pinnedId instead would let one account occupy two flights under aliases.
const refreshInFlight = new Map<string, Promise<unknown>>();

function dedupeRefresh<T>(key: string, run: () => Promise<T>): Promise<T> {
  const flight = refreshInFlight.get(key) as Promise<T> | undefined;
  if (flight) return flight;
  const task = run();
  refreshInFlight.set(key, task);
  const cleanup = () => {
    if (refreshInFlight.get(key) === task) refreshInFlight.delete(key);
  };
  void task.then(cleanup, cleanup);
  return task;
}

export async function accountCredentials(
  id: string,
  forceRefresh = false,
): Promise<CodexCredentials> {
  if (id === "external") {
    let auth = await invoke<ExternalAuth>("codex_auth_json_read");
    if (!external || external.accountId !== auth.accountId) {
      const changed: CredentialError = new Error(
        "External Codex account changed",
      );
      changed.permanent = true;
      throw changed;
    }
    const exp = claims(auth.accessToken)?.exp;
    if (forceRefresh || (typeof exp === "number" && exp * 1000 <= Date.now())) {
      try {
        auth = await dedupeRefresh("external", () =>
          invoke<ExternalAuth>("codex_auth_json_refresh", {
            accountId: auth.accountId,
          }),
        );
      } catch (error) {
        const wrapped = credentialError(
          error,
          "External Codex refresh failed; sign in again",
        );
        if (wrapped.permanent) external.disabledCause = wrapped.message;
        throw wrapped;
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
    try {
      account = await dedupeRefresh(id, () =>
        invoke<CodexCredentials>("codex_account_refresh", { id }),
      );
    } catch (error) {
      throw credentialError(error, "Codex account refresh failed");
    }
  }
  return account;
}

async function refreshedCredentials(
  previous: string | undefined,
  pinnedId?: string,
): Promise<CodexCredentials> {
  // The server allows roughly ten seconds for an answer, so a single attempt
  // per request; Codex re-asks on the next 401 if this one fails.
  await loadCodexAccounts();
  const account = previous
    ? failoverAccounts().find((a) => a.accountId === previous)
    : failoverAccounts().find((a) => a.id === pinnedId);
  if (!account) throw new Error("Unknown Codex account");
  return await accountCredentials(account.id, true);
}

/** Handles requests even during initialize/login, before a Live has been bound. */
export function respondCodexRefresh(
  rpc: Pick<JsonRpcClient, "respond" | "respondError">,
  requestId: JsonRpcId,
  params: unknown,
  pinnedId?: string,
  stillCurrent?: () => boolean,
): Promise<void> {
  return (async () => {
    try {
      const previous = stringField(asRecord(params), "previousAccountId");
      const credentials = await refreshedCredentials(previous, pinnedId);
      if (stillCurrent && !stillCurrent()) {
        await rpc.respondError(requestId, {
          code: -32000,
          message: "Codex account changed during refresh",
        });
        return;
      }
      await rpc.respond(requestId, {
        accessToken: credentials.accessToken,
        chatgptAccountId: credentials.accountId,
        ...(credentials.planType
          ? { chatgptPlanType: credentials.planType }
          : {}),
      });
    } catch {
      /* Never expose token endpoint payloads. */
      await rpc.respondError(requestId, {
        code: -32000,
        message: "Codex account refresh failed; sign in again",
      });
    }
  })();
}

export function resetCodexAccounts(): void {
  cached = [];
  external = undefined;
  refreshInFlight.clear();
}
