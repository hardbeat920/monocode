import { getCurrentWindow } from "@tauri-apps/api/window";
import { homeDir } from "../fs";
import { HARNESS_TITLE, type HarnessId } from "../session";
import * as child from "./child";
import { harnessLoginArgs } from "./authSupport";

export {
  harnessLoginArgs,
  isHarnessAuthError,
  latestTurnNeedsHarnessLogin,
  supportsHarnessLogin,
} from "./authSupport";

const LOGIN_TIMEOUT_MS = 10 * 60_000;
const LOGIN_CHILD_PREFIX = "monocode-provider-login-";

function loginChildId(harness: HarnessId): string {
  let windowLabel = "main";
  try {
    windowLabel = getCurrentWindow().label || windowLabel;
  } catch {
    // Keep the login helper usable in browser previews and isolated tests.
  }
  const safeWindowLabel = windowLabel.replace(/[^a-zA-Z0-9_-]/g, "-");
  return `${LOGIN_CHILD_PREFIX}${safeWindowLabel}-${harness}`;
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
};

const inflight = new Map<HarnessId, Promise<void>>();

/**
 * Launch the provider's own login flow. The child owns browser opening and
 * credential storage; MonoCode only supervises its exit status. Duplicate
 * clicks share one run so two OAuth flows cannot race each other.
 */
export function loginHarness(harness: HarnessId): Promise<void> {
  const current = inflight.get(harness);
  if (current) return current;

  const run = runHarnessLogin(harness).finally(() => {
    if (inflight.get(harness) === run) inflight.delete(harness);
  });
  inflight.set(harness, run);
  return run;
}

async function runHarnessLogin(harness: HarnessId): Promise<void> {
  const args = harnessLoginArgs(harness);
  const resolve = LOGIN_RESOLVERS[harness];
  if (!args || !resolve) {
    throw new Error(
      `${HARNESS_TITLE[harness]} does not offer a single browser sign-in flow.`,
    );
  }

  const [{ path }, cwd] = await Promise.all([resolve(), homeDir()]);
  const childId = loginChildId(harness);
  await child.killChild(childId).catch(() => undefined);

  return new Promise<void>((resolve, reject) => {
    let settled = false;
    let lastError = "";
    let timer: ReturnType<typeof setTimeout> | undefined;

    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      child.unwatchChild(childId);
      if (error) reject(error);
      else resolve();
    };

    child.watchChild(
      childId,
      () => undefined,
      (code) => {
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

    timer = setTimeout(() => {
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

    void child.spawnChild(childId, path, [...args], cwd).catch((error) => {
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
