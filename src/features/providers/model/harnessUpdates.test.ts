import { describe, expect, it } from "vitest";
import type { HarnessId } from "../../sessions/model/session";
import { findHarnessUpdates } from "./harnessUpdates";

const INSTALLED: Partial<Record<HarnessId, string>> = {
  claude: "2.1.284 (Claude Code)",
  codex: "codex-cli 0.159.2",
  opencode: "1.18.31",
  cursor: "2026.09.01-abc",
};

const LATEST: Partial<Record<HarnessId, string>> = {
  claude: "2.1.285",
  codex: "0.159.2",
  opencode: "1.18.33",
};

function find(harnesses: HarnessId[]) {
  return findHarnessUpdates({
    harnesses,
    installedVersion: async (id) => INSTALLED[id],
    latestVersion: async (id) => {
      const version = LATEST[id];
      if (!version) throw new Error("offline");
      return version;
    },
  });
}

describe("harness update check", () => {
  it("reports only harnesses behind the published release", async () => {
    expect(await find(["claude", "codex", "opencode"])).toEqual([
      {
        harness: "claude",
        installed: "2.1.284",
        latest: "2.1.285",
      },
      {
        harness: "opencode",
        installed: "1.18.31",
        latest: "1.18.33",
      },
    ]);
  });

  it("skips harnesses without a version feed or whose lookup fails", async () => {
    expect(await find(["cursor", "pi"])).toEqual([]);
  });

  it("offers a harness still behind again, at the newest release", async () => {
    expect(await find(["claude"])).toMatchObject([{ latest: "2.1.285" }]);

    LATEST.claude = "2.1.286";
    try {
      expect(await find(["claude"])).toMatchObject([
        { harness: "claude", installed: "2.1.284", latest: "2.1.286" },
      ]);
    } finally {
      LATEST.claude = "2.1.285";
    }
  });
});
