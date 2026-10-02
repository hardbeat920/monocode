import { asRecord } from "../../../integrations/harness/providers/codex/codexProtocol";
import {
  parseResetTimestamp,
  type RateLimitResetCredit,
  type RateLimitResetCredits,
} from "./rateLimits";

const GRANT_ID = /^[a-z0-9_-]{1,40}$/;

/** Normalize the evaluated Cedar and Juniper blocks from Claude Code's usage reads. */
export function parseClaudeResetCredits(
  payload: Record<string, unknown>,
  now = Date.now(),
): RateLimitResetCredits | null {
  const cedar = asRecord(payload.cedar_ember);
  const juniper = asRecord(payload.juniper_tide);
  const credits: RateLimitResetCredit[] = [];
  let notice: string | undefined;
  if (
    cedar?.ineligible_reason === "surface" ||
    juniper?.ineligible_reason === "surface"
  ) {
    notice = "Some reset offers are only available in Claude Web or Desktop.";
  }
  if (payload.reset_discovery_failed === true) {
    notice = "Could not check all reset offers. Refresh usage to try again.";
  }
  const cooldown = parseResetTimestamp(cedar?.cooldown_until);
  const validCooldown = cedar?.cooldown_until == null || cooldown != null;
  const seen = new Set<string>();
  if (Array.isArray(cedar?.grants)) {
    for (const raw of cedar.grants) {
      const grant = asRecord(raw);
      if (
        !grant ||
        typeof grant.id !== "string" ||
        !GRANT_ID.test(grant.id) ||
        seen.has(grant.id)
      )
        continue;
      const remaining = grant.resets_left;
      if (
        typeof remaining !== "number" ||
        !Number.isSafeInteger(remaining) ||
        remaining <= 0
      )
        continue;
      const expiresAt = parseResetTimestamp(grant.ends_at);
      const grantedAt = parseResetTimestamp(grant.starts_at);
      // Invalid dates cannot establish that an offer is still available.
      if (
        (grant.ends_at != null && expiresAt == null) ||
        (grant.starts_at != null && grantedAt == null)
      )
        continue;
      if (expiresAt != null && expiresAt <= now) continue;
      const clears = Array.isArray(grant.clears)
        ? grant.clears.filter(
            (value): value is string => typeof value === "string",
          )
        : [];
      if (clears.length === 0) continue;
      const selected = cedar?.next_grant_id === grant.id;
      const usable =
        cedar?.eligible === true &&
        selected &&
        grant.usable_now === true &&
        grant.paused !== true &&
        validCooldown &&
        (cooldown == null || cooldown <= now) &&
        (grantedAt == null || grantedAt <= now);
      const unavailableReason =
        cedar?.eligible !== true
          ? cedar?.ineligible_reason === "surface"
            ? "This offer is only available in Claude Web or Desktop."
            : "This account is not eligible to use this offer."
          : !validCooldown
            ? "Could not check this offer's cooldown. Refresh usage."
            : grant.paused === true
              ? "This offer is paused."
              : cooldown != null && cooldown > now
                ? "This offer is cooling down."
                : grantedAt != null && grantedAt > now
                  ? "This offer has not started yet."
                  : !selected
                    ? "Another reset must be used first."
                    : grant.use_requires_limit !== false &&
                        cedar?.at_limit !== true
                      ? "Available when you reach a covered usage limit."
                      : "This offer cannot be used for your current limits.";
      seen.add(grant.id);
      const full = clears.includes("five_hour") && clears.includes("seven_day");
      credits.push({
        id: `cedar_ember:${grant.id}`,
        resetType: "claudeCedar",
        status: usable ? "available" : "unavailable",
        remainingCount: remaining,
        grantedAt,
        expiresAt,
        title:
          typeof grant.label === "string" && grant.label.trim()
            ? grant.label.trim()
            : full
              ? "Full reset"
              : clears.length === 1 && clears[0] === "five_hour"
                ? "5-hour reset"
                : "Usage limit reset",
        description: resetDescription(clears),
        ...(!usable ? { unavailableReason } : {}),
      });
    }
  }
  // Juniper is a separate session-only offer; never infer it from a null block.
  if (
    juniper?.eligible === true &&
    juniper.arm === "reset" &&
    juniper.available === true
  ) {
    const expiresAt = parseResetTimestamp(juniper.weekly_resets_at);
    if (
      (juniper.weekly_resets_at == null || expiresAt != null) &&
      (expiresAt == null || expiresAt > now)
    ) {
      credits.push({
        id: "juniper_tide",
        resetType: "claudeJuniper",
        status: "available",
        remainingCount: 1,
        grantedAt: null,
        expiresAt,
        title: "5-hour reset",
        description:
          "Resets your 5-hour session limit. Weekly limits stay unchanged.",
      });
    }
  }
  if (!cedar && !juniper && !notice) return null;
  return {
    availableCount: credits.reduce(
      (count, credit) => count + (credit.remainingCount ?? 1),
      0,
    ),
    credits,
    ...(notice ? { notice } : {}),
  };
}

function resetDescription(clears: string[]): string {
  const limits: string[] = [];
  if (clears.includes("five_hour")) limits.push("5-hour session limit");
  if (clears.includes("seven_day")) limits.push("weekly limit");
  if (clears.some((key) => key !== "five_hour" && key !== "seven_day"))
    limits.push("specified model or feature limits");
  return `Resets your ${limits.join(" and ")}. Your scheduled weekly reset stays unchanged.`;
}
