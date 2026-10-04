import { describe, expect, it, vi } from "vitest";

vi.mock("./child", () => {
  const found = async () => ({ path: "/usr/bin/claude" });
  const missing = async () => {
    throw new Error("not found");
  };
  return {
    resolveAntigravityBinary: missing,
    resolveClaudeBinary: found,
    resolveCodexBinary: missing,
    resolveCursorBinary: missing,
    resolveFxBinary: missing,
    resolveGrokBinary: missing,
    resolveHermesBinary: missing,
    resolveOmpBinary: missing,
    resolveOpenCodeBinary: missing,
    resolvePiBinary: missing,
  };
});
vi.mock("./registry", () => ({ isLiveHarness: () => true }));

import {
  hasProbedHarnessAvailability,
  isHarnessAvailable,
  probeHarnessAvailability,
  subscribeHarnessAvailability,
} from "./availability";

describe("harness availability probe", () => {
  it("is marked probed before subscribers are notified", async () => {
    const seen: { probed: boolean; claude: boolean; codex: boolean }[] = [];
    const unsubscribe = subscribeHarnessAvailability(() => {
      seen.push({
        probed: hasProbedHarnessAvailability(),
        claude: isHarnessAvailable("claude"),
        codex: isHarnessAvailable("codex"),
      });
    });

    await probeHarnessAvailability();
    unsubscribe();

    expect(seen).toEqual([{ probed: true, claude: true, codex: false }]);
  });
});
