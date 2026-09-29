import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "../../../platform/tauri/fs";
import {
  errorRateLimits,
  parseClaudeOAuthUsage,
  parseCodexRateLimits,
  parseDroidUsage,
  parseGrokBilling,
  parseOpencodeGoUsage,
  unavailableRateLimits,
  type ProviderRateLimits,
} from "./rateLimits";
import {
  killChild,
  resolveCodexBinary,
  resolveGrokBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../../integrations/harness/core/child";
import { asRecord } from "../../../integrations/harness/providers/codex/codexProtocol";
import { JsonRpcClient } from "../../../integrations/harness/core/jsonRpc";
import { AcpClient } from "../../../integrations/harness/core/acp";
import {
  grokAuthMethodId,
  grokSpawnArgs,
} from "../../../integrations/harness/providers/grok/grokProtocol";

const USAGE_CHILD_ID = "monocode-codex-usage";
const GROK_USAGE_CHILD_ID = "monocode-grok-usage";
const DISCOVERY_TIMEOUT_MS = 15_000;
const REQUEST_TIMEOUT_MS = 12_000;

type OpencodeGoUsageFetch = {
  status: "ok" | "error" | "unavailable" | string;
  httpStatus?: number | null;
  body?: string | null;
  error?: string | null;
};

/**
 * Fetch OpenCode Go 5h / weekly / monthly usage via the official API.
 * Runs through a Tauri command so the webview CORS policy does not apply.
 */
export async function fetchOpencodeGoRateLimits(): Promise<ProviderRateLimits> {
  let result: OpencodeGoUsageFetch;
  try {
    result = await invoke<OpencodeGoUsageFetch>("fetch_opencode_go_usage");
  } catch (error) {
    return errorRateLimits(
      "opencode",
      error instanceof Error
        ? error.message
        : "OpenCode Go usage unavailable",
    );
  }
  if (result.status === "ok" && result.body) {
    try {
      const parsed = parseOpencodeGoUsage(JSON.parse(result.body));
      if (parsed.session || parsed.weekly || parsed.monthly) return parsed;
    } catch {
      return errorRateLimits("opencode", "OpenCode Go response was not JSON");
    }
    // A 200 with no usable windows is malformed: report an error so the
    // footer retries instead of sticking in "unavailable" forever.
    return errorRateLimits(
      "opencode",
      "OpenCode Go usage response was unexpected",
    );
  }
  if (result.status === "unavailable") {
    return unavailableRateLimits(
      "opencode",
      result.error?.trim() || "OpenCode Go not connected",
    );
  }
  return errorRateLimits(
    "opencode",
    result.error?.trim() || "OpenCode Go usage unavailable",
  );
}

type DroidUsageFetch = {
  status: "ok" | "error" | "unavailable" | string;
  httpStatus?: number | null;
  body?: string | null;
  error?: string | null;
};

/**
 * Fetch Factory Droid 5h / weekly / monthly usage. The Tauri command reads
 * the token Droid stores in ~/.factory and calls Factory's billing API.
 */
export async function fetchDroidRateLimits(): Promise<ProviderRateLimits> {
  let result: DroidUsageFetch;
  try {
    result = await invoke<DroidUsageFetch>("fetch_droid_usage");
  } catch (error) {
    return errorRateLimits(
      "droid",
      error instanceof Error ? error.message : "Droid usage unavailable",
    );
  }
  if (result.status === "ok" && result.body) {
    let parsed: ProviderRateLimits;
    try {
      parsed = parseDroidUsage(JSON.parse(result.body));
    } catch {
      return errorRateLimits("droid", "Droid usage response was not JSON");
    }
    if (parsed.session || parsed.weekly || parsed.monthly) return parsed;
    return unavailableRateLimits("droid", "No Droid usage data");
  }
  if (result.status === "unavailable") {
    return unavailableRateLimits(
      "droid",
      result.error?.trim() || "Droid not signed in",
    );
  }
  return errorRateLimits(
    "droid",
    result.error?.trim() || "Droid usage unavailable",
  );
}

/**
 * Fetch the Grok Build credit allowance. Grok exposes it only through its
 * ACP agent (`_x.ai/billing`), so this spawns a short-lived agent, asks,
 * and exits. The agent reads its own cached sign-in from ~/.grok.
 */
export async function fetchGrokRateLimits(): Promise<ProviderRateLimits> {
  let path: string;
  try {
    path = (await resolveGrokBinary()).path;
  } catch {
    return unavailableRateLimits("grok", "Grok Build CLI not found");
  }
  try {
    const parsed = parseGrokBilling(await requestGrokBilling(path));
    if (parsed.weekly || parsed.monthly) return parsed;
    return unavailableRateLimits("grok", "No Grok usage data");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (/authentication required|not authenticated|not signed in/i.test(message)) {
      return unavailableRateLimits("grok", "Grok not signed in");
    }
    if (/ENOENT|not found|could not run/i.test(message)) {
      return unavailableRateLimits("grok", "Grok Build CLI not found");
    }
    return errorRateLimits("grok", message);
  }
}

async function requestGrokBilling(path: string): Promise<unknown> {
  const cwd = await homeDir();
  const acp = new AcpClient(GROK_USAGE_CHILD_ID, {
    onRequest: (id) => {
      void acp.respond(id, {}).catch(() => undefined);
    },
  });

  const stop = async () => {
    acp.close();
    unwatchChild(GROK_USAGE_CHILD_ID);
    await killChild(GROK_USAGE_CHILD_ID).catch(() => undefined);
  };

  await killChild(GROK_USAGE_CHILD_ID).catch(() => undefined);

  watchChild(
    GROK_USAGE_CHILD_ID,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Grok usage probe exited")),
  );

  try {
    await spawnChild(
      GROK_USAGE_CHILD_ID,
      path,
      grokSpawnArgs({ model: "" }),
      cwd,
    );
    return await withTimeout(
      DISCOVERY_TIMEOUT_MS,
      async () => {
        const init = await acp.request(
          "initialize",
          {
            protocolVersion: 1,
            clientCapabilities: {
              fs: { readTextFile: false, writeTextFile: false },
              terminal: false,
            },
            clientInfo: { name: "monocode", version: "0.1.0" },
          },
          REQUEST_TIMEOUT_MS,
        );
        const methodId = grokAuthMethodId(init);
        if (methodId) {
          await acp
            .request(
              "authenticate",
              { methodId, _meta: { headless: true } },
              REQUEST_TIMEOUT_MS,
            )
            .catch(() => undefined);
        }
        return acp.request<unknown>("_x.ai/billing", {}, REQUEST_TIMEOUT_MS);
      },
      () => {
        void stop();
      },
      "Grok usage probe timed out",
    );
  } finally {
    await stop();
  }
}

export type CodexRateLimitResetOutcome =
  "reset" | "nothingToReset" | "noCredit" | "alreadyRedeemed";

type ClaudeUsageFetch = {
  status: "ok" | "error" | "unavailable" | string;
  httpStatus?: number | null;
  body?: string | null;
  error?: string | null;
};

export async function fetchClaudeRateLimits(
  accountId = "default",
): Promise<ProviderRateLimits> {
  try {
    const result = await invoke<ClaudeUsageFetch>("fetch_claude_usage", {
      accountId,
    });
    if (result.status === "ok" && result.body) {
      const parsed = parseClaudeOAuthUsage(result.body);
      if (parsed.session || parsed.weekly) return parsed;
      return {
        ...parsed,
        status: parsed.status === "ok" ? "ok" : parsed.status,
      };
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
    );
  } catch (error) {
    return errorRateLimits(
      "claude",
      error instanceof Error ? error.message : "Claude usage unavailable",
    );
  }
}

export async function fetchCodexRateLimits(
  accountId = "default",
): Promise<ProviderRateLimits> {
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
      accountId,
    );
    const parsed = parseCodexRateLimits(result);
    if (parsed.session || parsed.weekly || parsed.monthly || parsed.resetCredits) {
      return parsed;
    }
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
  accountId = "default",
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
    accountId,
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

// Every probe reuses USAGE_CHILD_ID and kills whatever holds it first, so
// probes for different accounts (footer, Settings, account picker) must not
// overlap or they terminate each other.
let codexUsageQueue: Promise<unknown> = Promise.resolve();

function requestCodexAccount<T>(
  path: string,
  cwd: string,
  method: string,
  params: unknown,
  accountId: string,
): Promise<T> {
  const run = codexUsageQueue.then(() =>
    runCodexAccountRequest<T>(path, cwd, method, params, accountId),
  );
  codexUsageQueue = run.catch(() => undefined);
  return run;
}

async function runCodexAccountRequest<T>(
  path: string,
  cwd: string,
  method: string,
  params: unknown,
  accountId: string,
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
    await spawnChild(
      USAGE_CHILD_ID,
      path,
      ["app-server"],
      cwd,
      {
        provider: "codex",
        id: accountId,
      },
      "codex",
    );
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
  message = "Codex usage probe timed out",
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const pending = work();
  try {
    return await Promise.race([
      pending,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => {
          onTimeout();
          reject(new Error(message));
        }, ms);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
    void pending.catch(() => undefined);
  }
}
