import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const probe = vi.hoisted(() => {
  const finishers: Array<() => void> = [];
  return {
    finishers,
    available: new Set<string>(),
    probeHarnessAvailability: vi.fn(
      () => new Promise<void>((resolve) => finishers.push(resolve)),
    ),
  };
});
const cli = vi.hoisted(() => ({
  inspectHarnessBinary: vi.fn(),
  updateHarnessCli: vi.fn(),
  invoke: vi.fn(),
}));
vi.mock("../../../integrations/harness/core/availability", () => ({
  probeHarnessAvailability: probe.probeHarnessAvailability,
  isHarnessAvailable: (id: string) => probe.available.has(id),
}));
vi.mock("../../../integrations/harness/core/child", () => ({
  inspectHarnessBinary: cli.inspectHarnessBinary,
  updateHarnessCli: cli.updateHarnessCli,
}));
vi.mock("../../../integrations/harness/core/registry", () => ({
  refreshHarnessCatalogs: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: cli.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(async () => undefined),
  listen: vi.fn(),
}));

import { checkInstalledHarnessVersions } from "./harnessUpdateActions";

async function settle() {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("installed harness check", () => {
  afterEach(() => {
    probe.finishers.length = 0;
    probe.probeHarnessAvailability.mockClear();
  });

  it("shares a running check but runs a forced one after it", async () => {
    const first = checkInstalledHarnessVersions();
    const shared = checkInstalledHarnessVersions();
    const forced = checkInstalledHarnessVersions({ force: true });
    expect(probe.probeHarnessAvailability).toHaveBeenCalledTimes(1);
    expect(probe.probeHarnessAvailability).toHaveBeenLastCalledWith(undefined);

    probe.finishers.shift()!();
    await Promise.all([first, shared]);
    await settle();
    expect(probe.probeHarnessAvailability).toHaveBeenCalledTimes(2);
    expect(probe.probeHarnessAvailability).toHaveBeenLastCalledWith({
      force: true,
    });

    probe.finishers.shift()!();
    await expect(forced).resolves.toEqual([]);
  });
});

describe("check overlapping an update", () => {
  let installed: string;
  let feed: string | ReturnType<typeof deferred<string>>;

  beforeEach(() => {
    vi.resetModules();
    probe.available.add("claude");
    installed = "1.0.0 (Claude Code)";
    feed = "1.1.0";
    cli.inspectHarnessBinary.mockImplementation(async () => ({
      path: "/bin/claude",
      version: installed,
    }));
    cli.invoke.mockImplementation((command: string) => {
      if (command !== "harness_latest_version") throw new Error(command);
      return typeof feed === "string" ? Promise.resolve(feed) : feed.promise;
    });
  });

  afterEach(() => {
    probe.available.clear();
    probe.finishers.length = 0;
    vi.clearAllMocks();
  });

  /** Runs a check to completion, releasing its availability probe. */
  function check(
    actions: typeof import("./harnessUpdateActions"),
  ): Promise<unknown> {
    const result = actions.checkInstalledHarnessVersions({ force: true });
    probe.finishers.shift()!();
    return result;
  }

  it.each([
    ["the update", "1.1.0", "current"],
    ["the update", "1.2.0", "behind"],
    ["the check", "1.1.0", "current"],
    ["the check", "1.2.0", "behind"],
  ] as const)(
    "keeps the updated version when %s finishes last and the feed says %s",
    async (finishesLast, release, status) => {
      const actions = await import("./harnessUpdateActions");
      const updates = await import("./harnessUpdates");
      await check(actions);
      const [offered] = updates.pendingHarnessUpdates(
        actions.getHarnessUpdateSnapshot().checks!,
      );
      expect(offered).toEqual({
        harness: "claude",
        installed: "1.0.0",
        latest: "1.1.0",
      });

      const install = deferred<void>();
      cli.updateHarnessCli.mockReturnValue(install.promise);
      const run = actions.runHarnessUpdate(offered);
      const lookup = deferred<string>();
      feed = lookup;
      // Reads the old binary, since the update has not installed yet.
      const racing = check(actions);
      await settle();
      expect(cli.inspectHarnessBinary).toHaveBeenCalledTimes(2);

      const finishUpdate = async () => {
        installed = "1.1.0 (Claude Code)";
        install.resolve();
        await expect(run).resolves.toEqual({
          status: "updated",
          version: "1.1.0",
        });
      };
      const finishCheck = async () => {
        lookup.resolve(release);
        await racing;
      };
      if (finishesLast === "the update") {
        await finishCheck();
        await finishUpdate();
      } else {
        await finishUpdate();
        await finishCheck();
      }

      expect(actions.getHarnessUpdateSnapshot().checks).toEqual([
        { harness: "claude", status, installed: "1.1.0", latest: release },
      ]);
    },
  );

  it("keeps the updated version when the check could not read the CLI", async () => {
    const actions = await import("./harnessUpdateActions");
    await check(actions);
    const install = deferred<void>();
    cli.updateHarnessCli.mockReturnValue(install.promise);
    const run = actions.runHarnessUpdate({
      harness: "claude",
      installed: "1.0.0",
      latest: "1.1.0",
    });
    // The updater is replacing the binary.
    cli.inspectHarnessBinary.mockResolvedValueOnce({
      path: "/bin/claude",
      error: "spawn ENOENT",
    });
    const racing = check(actions);
    await settle();

    installed = "1.1.0 (Claude Code)";
    install.resolve();
    await run;
    await racing;
    expect(actions.getHarnessUpdateSnapshot().checks).toEqual([
      {
        harness: "claude",
        status: "current",
        installed: "1.1.0",
        latest: "1.1.0",
      },
    ]);
  });

  it.each(["the update", "the check"] as const)(
    "keeps an updated CLI the probe missed when %s finishes last",
    async (finishesLast) => {
      const actions = await import("./harnessUpdateActions");
      await check(actions);
      const install = deferred<void>();
      cli.updateHarnessCli.mockReturnValue(install.promise);
      const run = actions.runHarnessUpdate({
        harness: "claude",
        installed: "1.0.0",
        latest: "1.1.0",
      });
      // The updater has removed the binary when the probe looks for it.
      probe.available.delete("claude");
      const racing = check(actions);
      await settle();

      const finishUpdate = async () => {
        installed = "1.1.0 (Claude Code)";
        install.resolve();
        await run;
      };
      if (finishesLast === "the update") {
        await racing;
        await finishUpdate();
      } else {
        await finishUpdate();
        await racing;
      }
      expect(actions.getHarnessUpdateSnapshot().checks).toEqual([
        {
          harness: "claude",
          status: "current",
          installed: "1.1.0",
          latest: "1.1.0",
        },
      ]);
    },
  );

  it("trusts a check that started after the update finished", async () => {
    const actions = await import("./harnessUpdateActions");
    await check(actions);
    cli.updateHarnessCli.mockImplementation(async () => {
      installed = "1.1.0 (Claude Code)";
    });
    await actions.runHarnessUpdate({
      harness: "claude",
      installed: "1.0.0",
      latest: "1.1.0",
    });

    // Someone reinstalls the old release outside MonoCode.
    installed = "1.0.0 (Claude Code)";
    await check(actions);
    expect(actions.getHarnessUpdateSnapshot().checks).toEqual([
      {
        harness: "claude",
        status: "behind",
        installed: "1.0.0",
        latest: "1.1.0",
      },
    ]);
  });
});
