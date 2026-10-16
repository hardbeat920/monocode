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
  grok: ["login", "--oauth"],
  // MonoCode uses fx through Vercel AI Gateway. Choosing it explicitly avoids
  // leaving `fx login` waiting on a TTY-only provider picker.
  fx: ["login", "vercel"],
  devin: ["auth", "login"],
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
    // Claude Code: "Failed to authenticate: OAuth session expired…",
    // "Invalid API key · Please run /login", "OAuth token has expired".
    /\bfailed to authenticate\b/i,
    /\bplease run \/login\b/i,
    /\boauth token (?:has )?(?:expired|(?:been )?revoked)\b/i,
  ].some((pattern) => pattern.test(message));
}

const CLAUDE_AUTH_FAILURE_REPLY = [
  /^failed to authenticate[.:]\s.*(?:\b(?:oauth|token|api key|credentials|401|authentication_error)\b|\/login\b)/i,
  /^[^·]{1,200}·\s*please run \/login\.?$/i,
];

/**
 * Whether a whole reply is Claude Code's own sign-in failure. Older CLIs and
 * saved transcripts carry it as assistant text without a structured error, so
 * the match stays narrow: one short line in Claude's wording, never prose that
 * merely discusses authentication.
 */
export function isClaudeAuthFailureText(text: string): boolean {
  const value = text.trim();
  if (!value || value.length > 1_000 || value.includes("\n")) return false;
  return CLAUDE_AUTH_FAILURE_REPLY.some((pattern) => pattern.test(value));
}

type AuthFailureBlock = Pick<Block, "role" | "text" | "notice" | "streaming">;

/**
 * The block that shows the active turn ended on a provider authentication
 * failure: a provider error from any turn position, or, for Claude, a settled
 * sign-in failure that is the turn's only reply.
 */
export function latestTurnHarnessAuthFailure<T extends AuthFailureBlock>(
  blocks: readonly T[],
  harness?: HarnessId,
): T | undefined {
  let reply: T | undefined;
  let replies = 0;
  let otherOutput = false;
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (!block || block.role === "user") break;
    if (
      block.role === "system" &&
      block.notice === "error" &&
      isHarnessAuthError(block.text)
    ) {
      return block;
    }
    if (block.role === "assistant") {
      reply = block;
      replies += 1;
    } else if (block.role !== "system" || block.notice) {
      otherOutput = true;
    }
  }
  return harness === "claude" &&
    reply &&
    replies === 1 &&
    !otherOutput &&
    !reply.streaming &&
    isClaudeAuthFailureText(reply.text)
    ? reply
    : undefined;
}

/** Whether the active turn ended on a provider authentication failure. */
export function latestTurnNeedsHarnessLogin(
  blocks: readonly AuthFailureBlock[],
  harness?: HarnessId,
): boolean {
  return latestTurnHarnessAuthFailure(blocks, harness) !== undefined;
}
