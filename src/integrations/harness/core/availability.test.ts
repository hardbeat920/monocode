// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const mock = vi.hoisted(() => ({
  resolveClaudeBinary: vi.fn(async () => ({ path: "/usr/local/bin/claude" })),
}));

vi.mock("./child", () => ({
  resolveAntigravityBinary: async () => ({ path: "/fake/agy", args: [] }),
  resolveClaudeBinary: mock.resolveClaudeBinary,
  resolveCodexBinary: async () => ({ path: "/fake/codex" }),
  resolveCursorBinary: async () => ({ path: "/fake/cursor-agent" }),
  resolveFxBinary: async () => ({ path: "/fake/fx" }),
  resolveGrokBinary: async () => ({ path: "/fake/grok" }),
  resolveHermesBinary: async () => ({ path: "/fake/hermes" }),
  resolveOmpBinary: async () => ({ path: "/fake/omp" }),
  resolveOpenCodeBinary: async () => ({ path: "/fake/opencode" }),
  resolvePiBinary: async () => ({ path: "/fake/pi" }),
  resolveBinaryOverride: async (path: string) => {
    if (path === "/missing/claude") throw new Error("not executable");
    return path;
  },
}));
vi.mock("./registry", () => ({ isLiveHarness: () => true }));

const { isHarnessAvailable, probeHarnessAvailability } = await import(
  "./availability"
);
const { saveHarnessRuntime } = await import(
  "../../../features/settings/model/settings"
);

beforeEach(() => {
  localStorage.clear();
  mock.resolveClaudeBinary.mockReset();
});

describe("probeHarnessAvailability", () => {
  it("marks a harness available when its default binary resolves", async () => {
    mock.resolveClaudeBinary.mockResolvedValue({ path: "/usr/local/bin/claude" });
    await probeHarnessAvailability({ force: true });
    expect(isHarnessAvailable("claude")).toBe(true);
  });

  it("marks a harness unavailable when resolution fails and no override is set", async () => {
    mock.resolveClaudeBinary.mockRejectedValue(new Error("not found"));
    await probeHarnessAvailability({ force: true });
    expect(isHarnessAvailable("claude")).toBe(false);
  });

  it("still marks a harness available when a binary-path override is set, even if the default resolver fails", async () => {
    mock.resolveClaudeBinary.mockRejectedValue(new Error("not found"));
    saveHarnessRuntime("claude", {
      binaryPath: "/opt/custom/claude",
      launchArgs: "",
      env: [],
    });
    await probeHarnessAvailability({ force: true });
    expect(isHarnessAvailable("claude")).toBe(true);
  });

  it("marks a harness unavailable when its binary-path override isn't executable", async () => {
    mock.resolveClaudeBinary.mockResolvedValue({ path: "/usr/local/bin/claude" });
    saveHarnessRuntime("claude", {
      binaryPath: "/missing/claude",
      launchArgs: "",
      env: [],
    });
    await probeHarnessAvailability({ force: true });
    expect(isHarnessAvailable("claude")).toBe(false);
  });
});
