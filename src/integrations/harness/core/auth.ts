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
export type LoginMode = "terminate" | "detach" | "unsupported";

function loginChildId(
  harness: HarnessId,
  accountId?: string,
  mode?: LoginMode,
): string {
  let windowLabel = "main";
  try {
    windowLabel = getCurrentWindow().label || windowLabel;
  } catch {
    // Keep the login helper usable in browser previews and isolated tests.
  }
  const safeWindowLabel = windowLabel.replace(/[^a-zA-Z0-9_-]/g, "-");
  const legacyId = `${LOGIN_CHILD_PREFIX}${mode === "detach" ? "" : `${safeWindowLabel}-`}${harness}`;
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
  retryStop?: () => void;
  stopError?: Error;
  mode?: LoginMode;
  modeListeners: Set<(mode: LoginMode) => void>;
  spawned: boolean;
  childId?: string;
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
        finish(
          run.stopError ??
            new Error(
              "Sign-in exit is not confirmed; another sign-in for this account cannot start while the previous process may still exist.",
            ),
        ),
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
  options?: {
    signal?: AbortSignal;
    onUrl?: (url: string) => void;
    onMode?: (mode: LoginMode) => void;
  },
): Promise<void> {
  if (options?.signal?.aborted)
    return Promise.reject(new Error("Sign-in cancelled."));
  const key = loginKey(harness, accountId);
  let run = inflight.get(key);
  if (run?.controller.signal.aborted) {
    run.retryStop?.();
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
      modeListeners: new Set(),
      spawned: false,
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
  if (options?.onMode) {
    run.modeListeners.add(options.onMode);
    if (run.mode) options.onMode(run.mode);
  }
  if (!options?.signal) return run.promise;

  const owned = run;
  owned.signals.add(options.signal);
  return new Promise<void>((resolve, reject) => {
    const detach = () => {
      options.signal?.removeEventListener("abort", abort);
      if (options.signal) owned.signals.delete(options.signal);
      if (options.onUrl) owned.listeners.delete(options.onUrl);
      if (options.onMode) owned.modeListeners.delete(options.onMode);
    };
    const abort = () => {
      detach();
      if (--owned.owners === 0 && (owned.mode !== "detach" || !owned.spawned))
        owned.controller.abort();
      if (owned.controller.signal.aborted) {
        void waitForLoginCleanup(owned).then(
          () => reject(new Error("Sign-in cancelled.")),
          reject,
        );
      } else
        reject(
          new Error(
            owned.mode === "detach"
              ? "Sign-in view closed; the provider sign-in is still running."
              : "Sign-in cancelled.",
          ),
        );
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

export function hasResumableHarnessLogin(
  harness: HarnessId,
  accountId?: string,
): boolean {
  const run = inflight.get(loginKey(harness, accountId));
  return !!run && run.mode === "detach" && run.spawned;
}

export function onHarnessLoginEnd(
  harness: HarnessId,
  accountId: string | undefined,
  listener: () => void,
): () => void {
  const run = inflight.get(loginKey(harness, accountId));
  if (!run) {
    listener();
    return () => {};
  }
  let active = true;
  void run.promise.then(
    () => {
      if (active) listener();
    },
    () => {
      if (active) listener();
    },
  );
  return () => {
    active = false;
  };
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
  if (!run.childId)
    return Promise.reject(new Error("This sign-in is still starting."));
  return child.writeChild(run.childId, code.trim());
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
  const [{ path }, cwd, mode] = await Promise.race([
    Promise.all([
      resolve(),
      homeDir(),
      child.loginMode().catch(() => {
        throw new Error(
          "Could not determine sign-in process support. Update the host or try again.",
        );
      }),
    ]),
    cancelled,
  ]).finally(stopWaiting);
  if (run.controller.signal.aborted) throw new Error("Sign-in cancelled.");
  run.mode = mode;
  run.modeListeners.forEach((listener) => listener(mode));
  if (mode === "unsupported")
    throw new Error(
      "Interactive sign-in is unavailable on Windows hosts. Sign in with the provider CLI in a terminal on the host.",
    );
  const childId = loginChildId(harness, accountId, mode);
  run.childId = childId;
  if (mode === "terminate") await child.stopChild(childId);
  if (run.controller.signal.aborted) throw new Error("Sign-in cancelled.");

  return new Promise<void>((resolve, reject) => {
    let settled = false;
    let lastError = "";
    let timer: ReturnType<typeof setTimeout> | undefined;
    let spawn: Promise<void>;
    let abort = () => undefined;
    let exited = false;
    let stopPending = false;
    let stopComplete = false;
    let stopReason = "Sign-in cancelled.";

    const maybeFinishStop = () => {
      if (exited && stopComplete) finish(new Error(stopReason));
    };
    const requestStop = () => {
      if (settled || stopPending || stopComplete) return;
      stopPending = true;
      run.stopError = undefined;
      void child.stopChild(childId, true).then(
        () => {
          stopPending = false;
          stopComplete = true;
          run.stopError = undefined;
          maybeFinishStop();
        },
        (error: unknown) => {
          stopPending = false;
          run.stopError = new Error(
            `Could not confirm sign-in exit: ${error instanceof Error ? error.message : String(error)}.`,
          );
        },
      );
    };

    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      run.controller.signal.removeEventListener("abort", abort);
      child.unwatchChild(childId);
      if (error) reject(error);
      else resolve();
    };

    const handleLine = (line: string) => {
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
    };
    child.watchChild(
      childId,
      handleLine,
      (code) => {
        if (run.controller.signal.aborted) {
          exited = true;
          maybeFinishStop();
          return;
        }
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

    if (mode === "terminate")
      timer = setTimeout(() => {
        if (run.controller.signal.aborted) return;
        stopReason = `${HARNESS_TITLE[harness]} sign-in timed out. Please try again.`;
        run.controller.abort();
      }, LOGIN_TIMEOUT_MS);

    const account =
      accountId && accountId !== "default" && supportsAccountProfiles(harness)
        ? { provider: harness, id: accountId }
        : undefined;
    spawn = (async () => {
      if (mode === "detach") {
        const line = await child.attachLogin(childId);
        if (line !== undefined) {
          run.spawned = true;
          if (line) handleLine(line);
          return;
        }
        if (run.controller.signal.aborted)
          throw new Error("Sign-in cancelled.");
      }
      run.spawned = true;
      await (account
        ? child.spawnChild(childId, path, [...args], cwd, account, harness)
        : child.spawnChild(childId, path, [...args], cwd, undefined, harness));
    })();
    abort = () => {
      if (timer) clearTimeout(timer);
      if (mode === "detach") {
        void spawn.then(
          () => finish(new Error("Sign-in cancelled.")),
          () => finish(new Error("Sign-in cancelled.")),
        );
        return;
      }
      void spawn
        .then(() => {
          run.retryStop = requestStop;
          requestStop();
        })
        .then(undefined, () => finish(new Error("Sign-in cancelled.")));
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
