import { describe, expect, it } from "vitest";
import {
  isLiveHarness,
  listHarnesses,
  refreshHarnessCatalog,
  refreshHarnessCatalogs,
  registerHarness,
  type HarnessAdapter,
} from "./registry";
import type { SendTurnInput, SteerTurnInput } from "./types";

function stub(id: "cursor" | "codex" | "claude", live: boolean): HarnessAdapter {
  return {
    id,
    live,
    async sendTurn(_input: SendTurnInput) {},
    async steerTurn(_input: SteerTurnInput) {},
    async cancelTurn() {},
    respondApproval() {},
    async stopSession() {},
    async forgetSession() {},
    bindSession() {},
  };
}

describe("harness registry", () => {
  it("tracks live adapters", () => {
    registerHarness(stub("cursor", true));
    registerHarness(stub("codex", true));
    registerHarness(stub("claude", true));
    expect(isLiveHarness("cursor")).toBe(true);
    expect(isLiveHarness("codex")).toBe(true);
    expect(isLiveHarness("claude")).toBe(true);
    expect(listHarnesses().map((a) => a.id).sort()).toEqual([
      "claude",
      "codex",
      "cursor",
    ]);
  });
});

describe("catalog refresh", () => {
  it("probes a provider once, and only the ones asked for", async () => {
    const calls: string[] = [];
    const withCatalog = (id: "cursor" | "codex" | "claude") => ({
      ...stub(id, true),
      refreshCatalog: async () => {
        calls.push(id);
      },
    });
    registerHarness(withCatalog("cursor"));
    registerHarness(withCatalog("codex"));

    await refreshHarnessCatalogs(["cursor", "cursor"]);
    await refreshHarnessCatalog("cursor");
    expect(calls).toEqual(["cursor"]);
  });

  it("lets a failed probe run again", async () => {
    let attempts = 0;
    registerHarness({
      ...stub("claude", true),
      refreshCatalog: async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("cli busy");
      },
    });

    await refreshHarnessCatalog("claude");
    await refreshHarnessCatalog("claude");
    await refreshHarnessCatalog("claude");
    expect(attempts).toBe(2);
  });
});
