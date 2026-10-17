import { describe, expect, it } from "vitest";
import {
  errorRateLimits,
  unavailableRateLimits,
  idleRateLimits,
} from "./rateLimits";
import { providerAccountConnection } from "./providerAccountConnection";

describe("providerAccountConnection", () => {
  it.each(["Claude not signed in", "Codex not authenticated", "Authentication required"])(
    "gives an explicit sign-out precedence over a cached identity: %s",
    (message) => {
      expect(providerAccountConnection(
        true,
        { email: "cached@example.com", name: "Cached", plan: "Pro" },
        unavailableRateLimits("codex", message),
      )).toBe("not-signed-in");
      expect(providerAccountConnection(
        true,
        { email: "cached@example.com" },
        errorRateLimits("codex", "Network unavailable"),
      )).toBe("connected");
    },
  );
  it("keeps an automatic default CLI profile disconnected until the CLI exists", () => {
    expect(providerAccountConnection(null, undefined, undefined)).toBe(
      "checking",
    );
    expect(
      providerAccountConnection(
        false,
        { email: "user@example.com" },
        undefined,
      ),
    ).toBe("not-installed");
  });

  it("requires actual sign-in evidence instead of treating every profile as connected", () => {
    expect(
      providerAccountConnection(
        true,
        null,
        unavailableRateLimits("claude", "Claude not signed in"),
      ),
    ).toBe("not-signed-in");
    expect(
      providerAccountConnection(
        true,
        null,
        unavailableRateLimits("devin", "Devin not signed in"),
      ),
    ).toBe("not-signed-in");
    expect(
      providerAccountConnection(true, { email: "user@example.com" }, undefined),
    ).toBe("connected");
    expect(providerAccountConnection(true, {}, idleRateLimits("codex"))).toBe(
      "checking",
    );
    expect(
      providerAccountConnection(true, null, {
        ...idleRateLimits("devin"),
        status: "ok",
      }),
    ).toBe("connected");
  });

  it("does not turn missing identity or network and usage errors into a sign-out claim", () => {
    expect(providerAccountConnection(true, undefined, undefined)).toBe(
      "checking",
    );
    expect(
      providerAccountConnection(
        true,
        null,
        errorRateLimits("claude", "Network unavailable"),
      ),
    ).toBe("unverified");
    expect(
      providerAccountConnection(
        true,
        null,
        unavailableRateLimits("codex", "No Codex usage data"),
      ),
    ).toBe("unverified");
  });
});
