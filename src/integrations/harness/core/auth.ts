import { getCurrentWindow } from "@tauri-apps/api/window";
import { homeDir } from "../../../platform/tauri/fs";
import { supportsAccountProfiles } from "../../../features/providers/model/providerAccounts";
import {
  HARNESS_TITLE,
  type HarnessId,
} from "../../../features/sessions/model/session";
import * as child from "./child";
import { harnessLoginArgs } from "./authSupport";

export {
  harnessLoginArgs,
  isHarnessAuthError,
  latestTurnNeedsHarnessLogin,
  supportsHarnessLogin,
} from "./authSupport";

const LOGIN_TIMEOUT_MS = 10 * 60_000;
const LOGIN_CLEANUP_WAIT_MS = 5_000;
const LOGIN_CHILD_PREFIX = "monocode-provider-login-";

function loginChildId(harness: HarnessId, accountId?: string): string {
  let windowLabel = "main";
  try {
    windowLabel = getCurrentWindow().label || windowLabel;
  } catch {
    // Keep the login helper usable in browser previews and isolated tests.
  }
  const safeWindowLabel = windowLabel.replace(/[^a-zA-Z0-9_-]/g, "-");
  const legacyId = `${LOGIN_CHILD_PREFIX}${safeWindowLabel}-${harness}`;
  if (!accountId || accountId === "default") return legacyId;
  const safeAccount = accountId.replace(/[^a-zA-Z0-9_-]/g, "-");
  return `${legacyId}-${safeAccount}`;
}

const LOGIN_RESOLVERS: Partial<
  Record<HarnessId, () => Promise<{ path: string }>>
> = {
  // Resolve through the module at click time. Some isolated harness tests mock
  // only their own binary resolver, and merely rendering a transcript must not
  // require every other CLI resolver to exist in that mock.
  claude: () => child.resolveClaudeBinary(),
  codex: () => child.resolveCodexBinary(),
  cursor: () => child.resolveCursorBinary(),
  grok: () => child.resolveGrokBinary(),
  fx: () => child.resolveFxBinary(),
  devin: () => child.resolveDevinBinary(),
};

type LoginRun = {
  promise: Promise<void>;
  controller: AbortController;
  owners: number;
  url?: string;
  listeners: Set<(url: string) => void>;
  signals: Set<AbortSignal>;
};
const inflight = new Map<string, LoginRun>();

function loginKey(harness: HarnessId, accountId?: string): string {
  return `${harness}:${accountId ?? "default"}`;
}

function waitForLoginCleanup(
  run: LoginRun,
  signal?: AbortSignal,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    let settled = false;
    const abort = () => finish(new Error("Sign-in cancelled."));
    const timer = setTimeout(
      () =>
        finish(new Error("Sign-in is still stopping. Please retry shortly.")),
      LOGIN_CLEANUP_WAIT_MS,
    );
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
      if (error) reject(error);
      else resolve();
    };
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) abort();
    void run.promise.then(
      () => finish(),
      () => finish(),
    );
  });
}

/**
 * Launch the provider's own login flow. The child owns browser opening and
 * credential storage; MonoCode only supervises its exit status. Duplicate
 * clicks share one run so two OAuth flows cannot race each other.
 */
export function loginHarness(
  harness: HarnessId,
  accountId?: string,
  options?: { signal?: AbortSignal; onUrl?: (url: string) => void },
): Promise<void> {
  if (options?.signal?.aborted)
    return Promise.reject(new Error("Sign-in cancelled."));
  const key = loginKey(harness, accountId);
  let run = inflight.get(key);
  if (run?.controller.signal.aborted) {
    return waitForLoginCleanup(run, options?.signal).then(() =>
      loginHarness(harness, accountId, options),
    );
  }
  if (!run) {
    const next: LoginRun = {
      promise: Promise.resolve(),
      controller: new AbortController(),
      owners: 0,
      listeners: new Set(),
      signals: new Set(),
    };
    next.promise = runHarnessLogin(harness, accountId, next).finally(() => {
      if (inflight.get(key) === next) inflight.delete(key);
    });
    inflight.set(key, next);
    run = next;
  }
  run.owners++;
  if (options?.onUrl) {
    run.listeners.add(options.onUrl);
    if (run.url) options.onUrl(run.url);
  }
  if (!options?.signal) return run.promise;

  const owned = run;
  owned.signals.add(options.signal);
  return new Promise<void>((resolve, reject) => {
    const detach = () => {
      options.signal?.removeEventListener("abort", abort);
      if (options.signal) owned.signals.delete(options.signal);
      if (options.onUrl) owned.listeners.delete(options.onUrl);
    };
    const abort = () => {
      detach();
      if (--owned.owners === 0) owned.controller.abort();
      if (owned.controller.signal.aborted) {
        void waitForLoginCleanup(owned).then(
          () => reject(new Error("Sign-in cancelled.")),
          reject,
        );
      } else reject(new Error("Sign-in cancelled."));
    };
    options.signal?.addEventListener("abort", abort, { once: true });
    owned.promise.then(
      () => {
        detach();
        resolve();
      },
      (error) => {
        detach();
        reject(error);
      },
    );
  });
}

export function submitHarnessLoginCode(
  harness: HarnessId,
  code: string,
  owner: AbortSignal,
  accountId?: string,
): Promise<void> {
  const run = inflight.get(loginKey(harness, accountId));
  if (!run || owner.aborted || !run.signals.has(owner) || !code.trim()) {
    return Promise.reject(new Error("This sign-in is no longer active."));
  }
  return child.writeChild(loginChildId(harness, accountId), code.trim());
}

async function runHarnessLogin(
  harness: HarnessId,
  accountId: string | undefined,
  run: LoginRun,
): Promise<void> {
  const args = harnessLoginArgs(harness);
  const resolve = LOGIN_RESOLVERS[harness];
  if (!args || !resolve) {
    throw new Error(
      `${HARNESS_TITLE[harness]} does not offer a single browser sign-in flow.`,
    );
  }

  let stopWaiting: () => void = () => {};
  const cancelled = new Promise<never>((_, reject) => {
    const abort = () => reject(new Error("Sign-in cancelled."));
    stopWaiting = () =>
      run.controller.signal.removeEventListener("abort", abort);
    run.controller.signal.addEventListener("abort", abort, { once: true });
    if (run.controller.signal.aborted) abort();
  });
  const [{ path }, cwd] = await Promise.race([
    Promise.all([resolve(), homeDir()]),
    cancelled,
  ]).finally(stopWaiting);
  if (run.controller.signal.aborted) throw new Error("Sign-in cancelled.");
  const childId = loginChildId(harness, accountId);
  await child.killChild(childId).catch(() => undefined);
  if (run.controller.signal.aborted) throw new Error("Sign-in cancelled.");

  return new Promise<void>((resolve, reject) => {
    let settled = false;
    let lastError = "";
    let timer: ReturnType<typeof setTimeout> | undefined;
    let spawn: Promise<void>;
    let abort = () => undefined;

    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      run.controller.signal.removeEventListener("abort", abort);
      child.unwatchChild(childId);
      if (error) reject(error);
      else resolve();
    };

    child.watchChild(
      childId,
      (line) => {
        if (harness !== "claude") return;
        const match = line.match(
          /If the browser didn't open, visit: (https:\/\/\S+)/,
        );
        if (!match) return;
        try {
          const url = new URL(match[1]);
          if (
            ![
              "claude.ai",
              "platform.claude.com",
              "console.anthropic.com",
            ].includes(url.hostname)
          )
            return;
          run.url = url.href;
          run.listeners.forEach((listener) => listener(url.href));
        } catch {
          /* Ignore malformed provider output. */
        }
      },
      (code) => {
        if (run.controller.signal.aborted) return;
        if (code === 0) {
          finish();
          return;
        }
        const detail = safeLoginDetail(lastError);
        finish(
          new Error(
            detail ||
              `${HARNESS_TITLE[harness]} sign-in exited${code == null ? " unexpectedly" : ` with code ${code}`}.`,
          ),
        );
      },
      (line) => {
        if (line.trim()) lastError = line.trim();
      },
    );
    if (settled) return;

    timer = setTimeout(() => {
      if (run.controller.signal.aborted) return;
      // Keep the run in `inflight` until the old child is gone. Otherwise a
      // quick retry can be spawned under the same id and then killed by this
      // timeout's late cleanup.
      void child
        .killChild(childId)
        .catch(() => undefined)
        .finally(() => {
          finish(
            new Error(
              `${HARNESS_TITLE[harness]} sign-in timed out. Please try again.`,
            ),
          );
        });
    }, LOGIN_TIMEOUT_MS);

    const account =
      accountId && accountId !== "default" && supportsAccountProfiles(harness)
        ? { provider: harness, id: accountId }
        : undefined;
    spawn = account
      ? child.spawnChild(childId, path, [...args], cwd, account, harness)
      : child.spawnChild(childId, path, [...args], cwd, undefined, harness);
    abort = () => {
      if (timer) clearTimeout(timer);
      void spawn
        .catch(() => undefined)
        .then(() => child.killChild(childId))
        .then(
          () => finish(new Error("Sign-in cancelled.")),
          () => {
            // A failed kill leaves child ownership uncertain; keep the ID reserved.
          },
        );
    };
    run.controller.signal.addEventListener("abort", abort, { once: true });
    if (run.controller.signal.aborted) abort();
    void spawn.catch((error) => {
      if (run.controller.signal.aborted) return;
      const message = error instanceof Error ? error.message : String(error);
      finish(
        new Error(
          `Could not start ${HARNESS_TITLE[harness]} sign-in: ${message}`,
        ),
      );
    });
  });
}

function safeLoginDetail(value: string): string {
  const text = value
    .replace(/https?:\/\/\S+/gi, "sign-in link")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > 240 ? `${text.slice(0, 237)}…` : text;
}

/** Test seam. */
export function resetHarnessLoginState(): void {
  inflight.clear();
}
