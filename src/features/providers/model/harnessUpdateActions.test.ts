import { afterEach, describe, expect, it, vi } from "vitest";

const probe = vi.hoisted(() => {
  const finishers: Array<() => void> = [];
  return {
    finishers,
    probeHarnessAvailability: vi.fn(
      () => new Promise<void>((resolve) => finishers.push(resolve)),
    ),
  };
});
vi.mock("../../../integrations/harness/core/availability", () => ({
  probeHarnessAvailability: probe.probeHarnessAvailability,
  isHarnessAvailable: () => false,
}));
vi.mock("../../../integrations/harness/core/child", () => ({
  inspectHarnessBinary: vi.fn(),
  updateHarnessCli: vi.fn(),
}));
vi.mock("../../../integrations/harness/core/registry", () => ({
  refreshHarnessCatalogs: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn() }));

import { checkInstalledHarnessVersions } from "./harnessUpdateActions";

async function settle() {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
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
