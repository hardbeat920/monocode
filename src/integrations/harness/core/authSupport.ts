import type { Block, HarnessId } from "../../../features/sessions/model/session";

/**
 * Account-level login commands that can run without an interactive provider
 * picker. OpenCode, Pi, and omp authenticate individual upstream providers,
 * so a single generic browser button would be misleading for them.
 */
const LOGIN_ARGS: Partial<Record<HarnessId, readonly string[]>> = {
  claude: ["auth", "login"],
  codex: ["login"],
  cursor: ["login"],
  devin: ["auth", "login"],
  grok: ["login", "--oauth"],
  // MonoCode uses fx through Vercel AI Gateway. Choosing it explicitly avoids
  // leaving `fx login` waiting on a TTY-only provider picker.
  fx: ["login", "vercel"],
};

export function supportsHarnessLogin(harness: HarnessId): boolean {
  return LOGIN_ARGS[harness] != null;
}

/** Exposed for login execution, settings copy, and regression tests. */
export function harnessLoginArgs(harness: HarnessId): readonly string[] | null {
  return LOGIN_ARGS[harness] ?? null;
}

/** True when a provider error is asking the user to authenticate again. */
export function isHarnessAuthError(message: string): boolean {
  return [
    /\bauthentication required\b/i,
    /\bnot (?:authenticated|signed in|logged in)\b/i,
    /\b(?:sign-in|login|session) (?:has )?expired\b/i,
    /\bplease (?:sign|log) in\b/i,
    /\brun [`'"]?\S+ (?:auth )?login\b/i,
  ].some((pattern) => pattern.test(message));
}

/** Whether the active turn ended on a provider authentication failure. */
export function latestTurnNeedsHarnessLogin(
  blocks: readonly Pick<Block, "role" | "text" | "notice">[],
): boolean {
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (!block || block.role === "user") return false;
    if (
      block.role === "system" &&
      block.notice === "error" &&
      isHarnessAuthError(block.text)
    ) {
      return true;
    }
  }
  return false;
}
