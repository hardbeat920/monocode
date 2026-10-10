import { beforeEach, describe, expect, it, vi } from "vitest";
import { isolatedExtensionArgs } from "./piExtensionIsolation";
import { OMP_FLAVOR, PI_FLAVOR } from "./piFlavor";

const mocks = vi.hoisted(() => ({
  execChild: vi.fn(async (): Promise<string> => "/home/test/.omp/agent"),
}));

vi.mock("../../core/child", () => ({
  execChild: mocks.execChild,
  resolveOmpBinary: async () => ({ path: "/fake/omp" }),
  resolvePiBinary: async () => ({ path: "/fake/pi" }),
}));

/** A fresh flavor object keys its own cache entry, so each case starts cold. */
const ompFlavor = () => ({ ...OMP_FLAVOR });

describe("isolatedExtensionArgs", () => {
  beforeEach(() => {
    mocks.execChild.mockReset().mockResolvedValue("/home/test/.omp/agent");
  });

  it("leaves Pi's trust-gated discovery alone", async () => {
    await expect(isolatedExtensionArgs(PI_FLAVOR)).resolves.toEqual({});
    expect(mocks.execChild).not.toHaveBeenCalled();
  });

  it("turns omp discovery off and names the user's own extension directory", async () => {
    await expect(isolatedExtensionArgs(ompFlavor())).resolves.toEqual({
      noExtensions: true,
      extensionPaths: ["/home/test/.omp/agent/extensions"],
    });
    expect(mocks.execChild).toHaveBeenCalledWith(
      "/fake/omp",
      ["config", "path"],
      undefined,
      "omp",
    );
  });

  it("falls back to discovery-off alone when the CLI reports no agent directory", async () => {
    mocks.execChild.mockResolvedValue("  ");
    await expect(isolatedExtensionArgs(ompFlavor())).resolves.toEqual({
      noExtensions: true,
    });
  });

  it("asks the CLI for its agent directory once per flavor", async () => {
    const flavor = ompFlavor();
    await isolatedExtensionArgs(flavor);
    await isolatedExtensionArgs(flavor);
    expect(mocks.execChild).toHaveBeenCalledTimes(1);
  });
});
