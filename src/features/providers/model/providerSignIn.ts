import { loginHarness } from "../../../integrations/harness/core/auth";
import { HARNESS_TITLE, type HarnessId } from "../../sessions/model/session";
import { errorRateLimits, type RateLimitProvider } from "./rateLimits";
import {
  getCachedRateLimits,
  loadRateLimits,
  setCachedRateLimits,
} from "./rateLimitsCache";

/** Providers whose usage reading proves a fresh sign-in actually works. */
const VERIFIED_SIGN_IN = new Set<HarnessId>(["claude", "codex", "devin"]);

/**
 * Run a provider's own sign-in for one account. Where MonoCode reads the
 * provider's usage, the fresh reading confirms the sign-in and updates the
 * usage chips; a failure is cached there too, so every surface agrees.
 */
export async function signInProvider(
  harness: HarnessId,
  accountId = "default",
): Promise<void> {
  const provider = VERIFIED_SIGN_IN.has(harness)
    ? (harness as RateLimitProvider)
    : undefined;
  try {
    await (accountId === "default"
      ? loginHarness(harness)
      : loginHarness(harness, accountId));
    if (!provider) return;
    const value = await loadRateLimits(provider, accountId, true);
    if (value.status !== "ok") {
      throw new Error(
        value.error ||
          `${HARNESS_TITLE[harness]} sign-in could not be verified`,
      );
    }
  } catch (error) {
    if (provider) {
      setCachedRateLimits(
        provider,
        accountId,
        errorRateLimits(
          provider,
          error instanceof Error ? error.message : "Could not complete sign-in",
          getCachedRateLimits(provider, accountId),
        ),
      );
    }
    throw error;
  }
}
