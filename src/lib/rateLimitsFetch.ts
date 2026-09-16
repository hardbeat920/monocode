import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "./fs";
import {
  errorRateLimits,
  parseClaudeOAuthUsage,
  parseCodexRateLimits,
  unavailableRateLimits,
  type ProviderRateLimits,
} from "./rateLimits";
import {
  killChild,
  resolveCodexBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./harness/child";
import { asRecord } from "./harness/codexProtocol";
import { JsonRpcClient } from "./harness/jsonRpc";

const USAGE_CHILD_ID = "monocode-codex-usage";
const DISCOVERY_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 12_000;

export type CodexRateLimitResetOutcome =
  "reset" | "nothingToReset" | "noCredit" | "alreadyRedeemed";

type ClaudeUsageFetch = {
  status: "ok" | "error" | "unavailable" | string;
  httpStatus?: number | null;
  body?: string | null;
  error?: string | null;
};

let lastClaudeSnapshot: ProviderRateLimits | null = null;

/** Every caller shares one snapshot; `maxAgeMs` skips the keychain and network. */
export async function fetchClaudeRateLimits(options?: {
  maxAgeMs?: number;
}): Promise<ProviderRateLimits> {
  if (
    options?.maxAgeMs != null &&
    lastClaudeSnapshot &&
    lastClaudeSnapshot.updatedAt > 0 &&
    Date.now() - lastClaudeSnapshot.updatedAt < options.maxAgeMs
  ) {
    return lastClaudeSnapshot;
  }
  lastClaudeSnapshot = await fetchClaudeRateLimitsNow(lastClaudeSnapshot);
  return lastClaudeSnapshot;
}

async function fetchClaudeRateLimitsNow(
  previous: ProviderRateLimits | null,
): Promise<ProviderRateLimits> {
  try {
    const result = await invoke<ClaudeUsageFetch>("fetch_claude_usage");
    if (result.status === "ok" && result.body) {
      return parseClaudeOAuthUsage(result.body);
    }
    if (result.status === "unavailable") {
      return unavailableRateLimits(
        "claude",
        result.error?.trim() || "Claude not signed in",
      );
    }
    return errorRateLimits(
      "claude",
      result.error?.trim() || "Claude usage unavailable",
      previous,
    );
  } catch (error) {
    return errorRateLimits(
      "claude",
      error instanceof Error ? error.message : "Claude usage unavailable",
      previous,
    );
  }
}

export async function fetchCodexRateLimits(): Promise<ProviderRateLimits> {
  let path: string;
  try {
    path = (await resolveCodexBinary()).path;
  } catch {
    return unavailableRateLimits("codex", "Codex CLI not found");
  }

  const cwd = await homeDir();
  try {
    const result = await requestCodexAccount<unknown>(
      path,
      cwd,
      "account/rateLimits/read",
      {},
    );
    const parsed = parseCodexRateLimits(result);
    if (parsed.session || parsed.weekly || parsed.resetCredits) return parsed;
    const rec = asRecord(result);
    if (rec && !parsed.session && !parsed.weekly) {
      return unavailableRateLimits("codex", "No Codex usage data");
    }
    return parsed;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (
      /not signed in|chatgpt authentication required|not authenticated/i.test(
        message,
      )
    ) {
      return unavailableRateLimits("codex", "Codex not signed in");
    }
    if (/ENOENT|not found|could not run/i.test(message)) {
      return unavailableRateLimits("codex", "Codex CLI not found");
    }
    return errorRateLimits("codex", message);
  }
}

export async function consumeCodexRateLimitResetCredit(
  creditId?: string,
): Promise<CodexRateLimitResetOutcome> {
  const path = (await resolveCodexBinary()).path;
  const cwd = await homeDir();
  const result = await requestCodexAccount<unknown>(
    path,
    cwd,
    "account/rateLimitResetCredit/consume",
    {
      idempotencyKey: crypto.randomUUID(),
      ...(creditId ? { creditId } : {}),
    },
  );
  const outcome = asRecord(result)?.outcome;
  if (
    outcome === "reset" ||
    outcome === "nothingToReset" ||
    outcome === "noCredit" ||
    outcome === "alreadyRedeemed"
  ) {
    return outcome;
  }
  throw new Error("Codex returned an unknown reset result");
}

async function requestCodexAccount<T>(
  path: string,
  cwd: string,
  method: string,
  params: unknown,
): Promise<T> {
  const rpc = new JsonRpcClient(
    USAGE_CHILD_ID,
    {
      onRequest: (id) => {
        void rpc.respond(id, {}).catch(() => undefined);
      },
    },
    { includeJsonrpc: false, label: "codex-usage" },
  );

  const stop = async () => {
    rpc.close();
    unwatchChild(USAGE_CHILD_ID);
    await killChild(USAGE_CHILD_ID).catch(() => undefined);
  };

  await killChild(USAGE_CHILD_ID).catch(() => undefined);

  watchChild(
    USAGE_CHILD_ID,
    (line) => rpc.pushLine(line),
    () => rpc.close(new Error("Codex usage probe exited")),
  );

  try {
    await spawnChild(USAGE_CHILD_ID, path, ["app-server"], cwd);
    return await withTimeout(
      DISCOVERY_TIMEOUT_MS,
      async () => {
        await rpc.request(
          "initialize",
          {
            clientInfo: {
              name: "monocode",
              title: "MonoCode",
              version: "0.1.0",
            },
            capabilities: { experimentalApi: true },
          },
          REQUEST_TIMEOUT_MS,
        );
        await rpc.notify("initialized", undefined);

        return rpc.request<T>(method, params, REQUEST_TIMEOUT_MS);
      },
      () => {
        void stop();
      },
    );
  } finally {
    await stop();
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
          reject(new Error("Codex usage probe timed out"));
        }, ms);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
    void pending.catch(() => undefined);
  }
}
