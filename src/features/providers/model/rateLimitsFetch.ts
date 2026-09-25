import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "../../../platform/tauri/fs";
import {
  errorRateLimits,
  parseClaudeOAuthUsage,
  parseCodexRateLimits,
  parseOpencodeGoUsage,
  unavailableRateLimits,
  type ProviderRateLimits,
} from "./rateLimits";
import {
  killChild,
  resolveCodexBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../../integrations/harness/core/child";
import { asRecord } from "../../../integrations/harness/providers/codex/codexProtocol";
import { JsonRpcClient } from "../../../integrations/harness/core/jsonRpc";
import {
  harnessRuntimeEnv,
  harnessRuntimeExtraArgs,
  resolveHarnessBinary,
} from "../../../integrations/harness/core/runtime";
import {
  loadEffectiveClaudeConfigDir,
  loadHarnessRuntime,
} from "../../settings/model/settings";

const USAGE_CHILD_ID = "monocode-codex-usage";
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
      configDirOverride: loadEffectiveClaudeConfigDir(),
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
    path = (await resolveHarnessBinary("codex", resolveCodexBinary)).path;
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
  accountId = "default",
): Promise<CodexRateLimitResetOutcome> {
  const path = (await resolveHarnessBinary("codex", resolveCodexBinary)).path;
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

async function requestCodexAccount<T>(
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
    const runtime = loadHarnessRuntime("codex");
    await spawnChild(
      USAGE_CHILD_ID,
      path,
      ["app-server", ...harnessRuntimeExtraArgs(runtime)],
      cwd,
      { provider: "codex", id: accountId },
      harnessRuntimeEnv(runtime),
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
